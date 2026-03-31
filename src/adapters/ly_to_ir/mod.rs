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
mod consume;
mod figured_bass;
mod lyrics;
mod merge;
mod modifiers;
mod music;
mod postprocess;
mod state;
mod walk;

#[cfg(test)]
mod tests;

use std::path::Path;

use num::rational::Ratio;

use crate::ir::direction::Barline;
use crate::ir::duration::Frac;
use crate::ir::harmony::FiguredBass;
use crate::ir::language::PitchLanguage;
use crate::ir::measure::{ClefSign, KeyMode, Measure, MeasureAttributes};
use crate::ir::music::MusicDocument;
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{Score, ScoreChild};
use crate::ir::Part;
use crate::parser::LilyPondParser;

use super::{AdapterError, Result, ToIrAdapter};

use crate::ir::direction::Direction;
use crate::ir::duration::Duration;

// Re-export items needed by sub-modules via `super::`
use figured_bass::distribute_figured_bass;
use merge::{
    apply_tuplet_ratio, beam_level_for_duration, measure_voice_duration, measures_are_spacer_only,
    merge_spacer_by_duration, merge_spacer_measures, resplit_measures_for_time_sig,
    resplit_measures_to_match, voice_element_duration,
};
use postprocess::post_process_beams_and_stems;

/// Tuple of (cumulative position, attributes, directions, left barline, right barline)
/// used when collecting per-measure metadata for re-splitting.
type MeasureMeta = (
    Frac,
    Option<MeasureAttributes>,
    Vec<Direction>,
    Option<Barline>,
    Option<Barline>,
);

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

/// What a variable definition expands to.
#[derive(Clone)]
enum VarDef {
    /// Variable contained `\new Staff { ... }` — stores the full part(s).
    Parts(Vec<(String, Part)>),
    /// Variable contained bare music — stores just the measures.
    /// The `Frac` records the time signature active when the variable was pre-parsed,
    /// so we can re-split if the time sig differs at resolution time.
    Measures(Vec<Measure>, Frac),
    /// Variable contained `\figuremode { ... }` — stores flat stream of entries.
    FiguredBass(Vec<FiguredBassEntry>),
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

        // If walk_program collected scores from \score blocks, return those
        if !state.completed_scores.is_empty() {
            for score in &mut state.completed_scores {
                for part in score.parts_mut() {
                    merge::merge_leading_attribute_measures(part);
                    merge::renumber_measures(part);
                }
                merge::propagate_first_tempo(score);
                post_process_beams_and_stems(score);
                // Mark first measure as implicit if partial_duration is set
                if score.metadata.partial_duration.is_some() {
                    for part in score.parts_mut() {
                        if let Some(m) = part.measures.first_mut() {
                            m.implicit = true;
                        }
                    }
                }
            }
            return Ok(state.completed_scores);
        }

        // Otherwise, build a single score from remaining state (no \score blocks)
        state.flush_measure();

        // Attach pending lyrics to matching parts
        for (voice_name, syllables) in &state.pending_lyrics {
            if let Some(&part_idx) = state.voice_part_map.get(voice_name) {
                if let Some((_, part)) = state.parts.get_mut(part_idx) {
                    lyrics::attach_lyrics_to_part(part, syllables);
                }
            }
        }

        // Merge spacer-only parts (from \new Dynamics) into staff parts
        merge::merge_dynamics_parts(&mut state.parts);

        let mut score = Score::new();
        score.metadata = state.metadata;
        score.metadata.pitch_mode = state.mode;
        score.metadata.pitch_language = Some(state.language);
        score.page_layout = state.page_layout;
        for (_, part) in state.parts {
            score.children.push(ScoreChild::Part(part));
        }
        if score.children.is_empty() {
            score.children.push(ScoreChild::Part(Part::new("P1")));
        }

        for part in score.parts_mut() {
            merge::merge_leading_attribute_measures(part);
            merge::renumber_measures(part);
        }
        merge::synchronize_time_signatures(&mut score);
        merge::propagate_first_tempo(&mut score);
        post_process_beams_and_stems(&mut score);
        // Mark first measure as implicit if partial_duration is set
        if score.metadata.partial_duration.is_some() {
            for part in score.parts_mut() {
                if let Some(m) = part.measures.first_mut() {
                    m.implicit = true;
                }
            }
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
