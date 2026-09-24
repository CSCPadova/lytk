//! LilyPond → IR parser.
//!
//! Uses the tree-sitter LilyPond grammar to parse `.ly` files and produce
//! an IR [`Score`].
//!
//! # Design
//!
//! The tree-sitter grammar produces a flat structure inside `expression_block`
//! nodes. Music elements (pitch symbols, duration integers, escaped commands
//! like `\key`, `\time`, `\clef`) are sibling children rather than being
//! nested under typed parent nodes.
//!
//! The parser walks children sequentially, using a state machine to:
//! - accumulate pitch symbols + octave marks + duration into Notes
//! - detect `\key`, `\time`, `\clef` command sequences
//! - track measure boundaries at `|` (pipe) bar checks
//! - handle `\relative` pitch context
//! - handle `{ }` and `<< >>` nesting for voices and parallel music
//!
//! # Reference
//! Python prototype: `lytk-py/converters/ly_to_ir.py` (uses python-ly,
//! not tree-sitter, so the tree walk strategy differs).

mod apply;
mod chord_mode;
mod consume;
mod figured_bass;
mod lyrics;
mod merge;
mod modifiers;
mod music;
mod postprocess;
mod state;
mod timeline;
mod walk;

#[cfg(test)]
mod tests;

use std::path::Path;

use num::rational::Ratio;

use crate::ir::harmony::FiguredBass;
use crate::ir::language::PitchLanguage;
use crate::ir::measure::{ClefSign, KeyMode};
use crate::ir::music::MusicDocument;
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{Score, ScoreChild};
use crate::ir::Part;
use crate::parser::LilyPondParser;

use super::{AdapterError, Result, ToIrAdapter};

use crate::ir::duration::{Duration, Frac};

// Re-export items needed by sub-modules via `super::`
use merge::{apply_tuplet_ratio, beam_level_for_duration};
use postprocess::{
    assign_slur_numbers, ensure_staff_clefs, post_process_beams_and_stems, resolve_ties,
};

// ---------------------------------------------------------------------------
// Clef name → (sign, line)
// ---------------------------------------------------------------------------

fn parse_clef_name(name: &str) -> Option<(ClefSign, u8, i8)> {
    // Strip octave suffixes like ^8, _8, ^15, _15
    let (base, oct_change) = if let Some(b) = name.strip_suffix("^15") {
        (b, 2i8)
    } else if let Some(b) = name.strip_suffix("_15") {
        (b, -2)
    } else if let Some(b) = name.strip_suffix("^8") {
        (b, 1)
    } else if let Some(b) = name.strip_suffix("_8") {
        (b, -1)
    } else {
        (name, 0)
    };

    let (sign, line) = match base {
        "treble" | "violin" | "G" => (ClefSign::G, 2u8),
        "french" => (ClefSign::G, 1),
        "soprano" => (ClefSign::C, 1),
        "mezzosoprano" => (ClefSign::C, 2),
        "alto" | "C" => (ClefSign::C, 3),
        "tenor" => (ClefSign::C, 4),
        "baritone" => (ClefSign::C, 5),
        "bass" | "F" => (ClefSign::F, 4),
        "varbaritone" => (ClefSign::F, 3),
        "subbass" => (ClefSign::F, 5),
        "percussion" => (ClefSign::Percussion, 0),
        "tab" | "moderntab" => (ClefSign::Tab, 5),
        _ => return None,
    };
    Some((sign, line, oct_change))
}

/// Map key mode string to KeyMode enum.
fn parse_key_mode(mode_str: &str) -> KeyMode {
    match mode_str {
        "\\major" | "major" => KeyMode::Major,
        "\\minor" | "minor" => KeyMode::Minor,
        "\\dorian" | "dorian" => KeyMode::Dorian,
        "\\phrygian" | "phrygian" => KeyMode::Phrygian,
        "\\lydian" | "lydian" => KeyMode::Lydian,
        "\\mixolydian" | "mixolydian" => KeyMode::Mixolydian,
        "\\aeolian" | "aeolian" => KeyMode::Aeolian,
        "\\locrian" | "locrian" => KeyMode::Locrian,
        "\\ionian" | "ionian" => KeyMode::Ionian,
        _ => KeyMode::Major,
    }
}

/// Convert a pitch step + alter to circle-of-fifths position,
/// then adjust for the key mode to produce the `fifths` value.
fn pitch_to_fifths(step: PitchStep, alter: Ratio<i32>, mode: KeyMode) -> i32 {
    // Natural step → fifths (major key)
    let base = match step {
        PitchStep::C => 0,
        PitchStep::D => 2,
        PitchStep::E => 4,
        PitchStep::F => -1,
        PitchStep::G => 1,
        PitchStep::A => 3,
        PitchStep::B => 5,
    };
    // Each semitone of alteration shifts by 7 fifths
    let alter_int = *alter.numer() / *alter.denom();
    let fifths_major = base + alter_int * 7;
    match mode {
        KeyMode::Minor | KeyMode::Aeolian => fifths_major - 3,
        KeyMode::Dorian => fifths_major - 2,
        KeyMode::Phrygian => fifths_major - 4,
        KeyMode::Lydian => fifths_major + 1,
        KeyMode::Mixolydian => fifths_major - 1,
        KeyMode::Locrian => fifths_major - 5,
        _ => fifths_major,
    }
}

// ---------------------------------------------------------------------------
// Parser state types
// ---------------------------------------------------------------------------

/// A figured bass entry with its duration, used during figuremode parsing.
#[derive(Clone)]
enum FiguredBassEntry {
    /// An actual figure group (e.g. <6 4>).
    Figure(FiguredBass),
    /// A skip/spacer with a duration (no figure produced).
    Skip(Duration),
}

/// Find the octave that LilyPond would infer in relative mode
/// (the closest octave of `step` to `prev`'s pitch).
fn find_relative_octave(prev: &Pitch, step: PitchStep) -> i32 {
    let prev_abs = prev.octave * 7 + prev.step.index();
    let mut best_octave = prev.octave;
    let mut best_diff = i32::MAX;

    for oct_try in [prev.octave - 1, prev.octave, prev.octave + 1] {
        let try_abs = oct_try * 7 + step.index();
        let diff = (try_abs - prev_abs).abs();
        if diff < best_diff {
            best_diff = diff;
            best_octave = oct_try;
        }
    }
    best_octave
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// LilyPond → IR adapter.
///
/// Parses LilyPond source text using tree-sitter and produces an IR `Score`.
pub struct LyToIrAdapter {
    language: PitchLanguage,
}

impl LyToIrAdapter {
    pub fn new() -> Self {
        Self {
            language: PitchLanguage::Nederlands,
        }
    }

    /// Set the default pitch language (overridden by `\language` in the source).
    pub fn with_language(mut self, lang: PitchLanguage) -> Self {
        self.language = lang;
        self
    }

    /// Parse LilyPond source text into one or more IR Scores (one per `\score` block).
    fn parse_source_multi(&self, source: &str) -> Result<Vec<Score>> {
        let mut parser = LilyPondParser::new().map_err(|e| AdapterError::Parse(e.to_string()))?;
        let tree = parser
            .parse(source)
            .map_err(|e| AdapterError::Parse(e.to_string()))?;

        let root = tree.root_node();
        if root.has_error() {
            // Still attempt to extract what we can; tree-sitter is error-tolerant
        }

        let mut state = state::WalkState::new(source);
        state.language = self.language;

        walk::walk_program(&mut state, root);

        // `\score` blocks were assembled as they closed (one per movement).
        if !state.completed_scores.is_empty() {
            return Ok(state.completed_scores);
        }

        // Otherwise the file's top-level music is one implicit score.
        let mut score = assemble_score(&mut state).unwrap_or_default();
        if score.children.is_empty() {
            score.children.push(ScoreChild::Part(Part::new("P1")));
        }
        Ok(vec![score])
    }

    /// Parse LilyPond source text into an IR Score.
    /// If there are multiple `\score` blocks, returns only the first one.
    fn parse_source(&self, source: &str) -> Result<Score> {
        let scores = self.parse_source_multi(source)?;
        Ok(scores.into_iter().next().unwrap_or_else(|| {
            let mut s = Score::new();
            s.children.push(ScoreChild::Part(Part::new("P1")));
            s
        }))
    }

    /// Parse LilyPond source into multiple scores (one per movement).
    pub fn convert_file_multi(&self, path: &Path) -> Result<Vec<Score>> {
        let source = std::fs::read_to_string(path)?;
        self.parse_source_multi(&source)
    }

    /// Parse LilyPond source string into multiple scores (one per movement).
    pub fn convert_str_multi(&self, text: &str) -> Result<Vec<Score>> {
        self.parse_source_multi(text)
    }
}

/// Turn the parts walked so far into a [`Score`]: fold spacer lanes and
/// Dynamics contexts into directions, place chord names, bar every part on one
/// score-wide grid, attach lyrics, then run the measure-level passes.
/// `None` when nothing was walked.
fn assemble_score(state: &mut state::WalkState) -> Option<Score> {
    use timeline::{split, Grid};

    state.flush_voice();
    let mut parts = std::mem::take(&mut state.parts);
    if parts.is_empty() {
        return None;
    }
    for pb in &mut parts {
        pb.tl.fold_spacer_lanes();
    }

    // Chord names attach to the first part with music, from its start.
    let harmonies = std::mem::take(&mut state.pending_harmonies);
    if !harmonies.is_empty() {
        if let Some(pb) = parts.iter_mut().find(|pb| pb.tl.has_lane_content()) {
            chord_mode::place_harmonies(&mut pb.tl, Frac::from_integer(0), &harmonies);
        }
    }

    // A Dynamics context outside a PianoStaff folds into the nearest staff:
    // the one before it, else the one after.
    if parts.len() > 1 {
        let mut i = 0;
        while i < parts.len() {
            let is_staff = |pb: &state::PartBuild| !merge::part_is_dynamics_only(pb);
            // An empty `\new Staff` is an empty staff, not a Dynamics lane.
            let has_content = parts[i].tl.has_lane_content() || !parts[i].tl.events.is_empty();
            let target = if merge::part_is_dynamics_only(&parts[i]) && has_content {
                (0..i)
                    .rev()
                    .find(|&j| is_staff(&parts[j]))
                    .or_else(|| (i + 1..parts.len()).find(|&j| is_staff(&parts[j])))
            } else {
                None
            };
            match target {
                Some(t) => {
                    let dyn_part = parts.remove(i);
                    let t = if t > i { t - 1 } else { t };
                    state.part_alias.insert(dyn_part.uid, parts[t].uid);
                    parts[t].tl.events.extend(dyn_part.tl.events);
                }
                None => i += 1,
            }
        }
    }

    let grid = Grid::build(parts.iter().map(|pb| &pb.tl));
    let mut built: Vec<(u32, Part)> = parts
        .into_iter()
        .map(|pb| {
            let mut part = pb.part;
            part.measures = split(
                pb.tl,
                &grid,
                chord_mode::HARMONY_DIVISIONS,
                figured_bass::FIGURED_BASS_DIVISIONS,
            );
            if part.staves > 1 {
                if let Some(m) = part.measures.first_mut() {
                    m.attributes.get_or_insert_with(Default::default).staves = Some(part.staves);
                }
            }
            (pb.uid, part)
        })
        .collect();

    // Lyrics, now that every part has its notes in measures.
    let lyrics_for_voices = std::mem::take(&mut state.pending_lyrics);
    let mut lyric_jobs: Vec<(u32, Vec<crate::ir::articulation::LyricSyllable>)> =
        std::mem::take(&mut state.added_lyrics);
    for (voice, syllables) in lyrics_for_voices {
        if let Some(&uid) = state.voice_part_map.get(&voice) {
            lyric_jobs.push((uid, syllables));
        }
    }
    for (uid, syllables) in lyric_jobs {
        let uid = state.resolve_uid(uid);
        if let Some((_, part)) = built.iter_mut().find(|(u, _)| *u == uid) {
            lyrics::attach_lyrics_to_part(part, &syllables);
        }
    }

    let mut score = Score::new();
    score.metadata = state.metadata.clone();
    score.metadata.pitch_mode = state.mode;
    score.metadata.pitch_language = Some(state.language);
    score.page_layout = state.page_layout.clone();
    score.children = built
        .into_iter()
        .map(|(_, part)| ScoreChild::Part(part))
        .collect();

    merge::synchronize_barlines(&mut score);
    merge::propagate_first_tempo(&mut score);
    post_process_beams_and_stems(&mut score);
    resolve_ties(&mut score);
    assign_slur_numbers(&mut score);
    ensure_staff_clefs(&mut score);
    // Mark first measure as implicit if partial_duration is set
    if score.metadata.partial_duration.is_some() {
        for part in score.parts_mut() {
            if let Some(m) = part.measures.first_mut() {
                m.implicit = true;
            }
        }
    }
    Some(score)
}

impl Default for LyToIrAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ToIrAdapter for LyToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score> {
        let source = std::fs::read_to_string(path)?;
        self.parse_source(&source)
    }

    fn convert_str(&self, text: &str) -> Result<Score> {
        self.parse_source(text)
    }
}

impl super::ToMusicAdapter for LyToIrAdapter {
    fn convert_file_to_music(&self, path: &Path) -> Result<MusicDocument> {
        let score = self.convert_file(path)?;
        Ok(crate::ir::lift::lift_to_music(&score))
    }

    fn convert_str_to_music(&self, text: &str) -> Result<MusicDocument> {
        let score = self.convert_str(text)?;
        Ok(crate::ir::lift::lift_to_music(&score))
    }
}
