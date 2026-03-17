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

use std::collections::HashMap;
use std::path::Path;

use num::rational::Ratio;
use tree_sitter::Node;

use crate::ir::articulation::{
    Articulation, BeamEvent, DynamicMark, Fermata, LyricSyllable, Placement, SlurEvent, StartStop,
    SyllabicType, Technical, TieEvent, TupletDisplay, Wedge,
};
use crate::ir::direction::{Barline, BarlineType, Direction, TempoDirection, TextDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::{FiguredBass, Figure};
use crate::ir::language::{parse_pitch_name, PitchLanguage, PitchMode};
use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, Measure, MeasureAttributes, TimeSignature};
use crate::ir::note::{ArpeggioType, Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{PageLayout, Score, ScoreChild, ScoreMetadata};
use crate::ir::voice::Voice;
use crate::ir::Part;
use crate::parser::LilyPondParser;

use super::{AdapterError, Result, ToIrAdapter};

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
// Parser state
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
    Measures(Vec<Measure>),
    /// Variable contained `\figuremode { ... }` — stores flat stream of entries.
    FiguredBass(Vec<FiguredBassEntry>),
}

/// State accumulated while walking tree-sitter nodes.
struct WalkState<'src> {
    source: &'src str,
    language: PitchLanguage,
    mode: PitchMode,

    // Current score being built
    metadata: ScoreMetadata,
    parts: Vec<(String, Part)>, // (context_name, part)
    part_counter: u32,

    // Variable definitions: name → either full parts (from \new Staff) or bare measures
    definitions: HashMap<String, VarDef>,
    // Lyric variable definitions: name → list of syllables
    lyric_definitions: HashMap<String, Vec<LyricSyllable>>,
    // Pending lyrics: voice_name → syllables (from \lyricsto)
    pending_lyrics: HashMap<String, Vec<LyricSyllable>>,
    // Voice name → part index mapping (for attaching lyrics)
    voice_part_map: HashMap<String, usize>,
    // Per-variable voice maps: var_name → { voice_name → local_part_index }
    var_voice_maps: HashMap<String, HashMap<String, usize>>,

    // Measure/voice state for the current part
    measure_num: u32,
    current_measure: Option<Measure>,
    current_voice: Vec<VoiceElement>,

    // Duration state: last explicit duration carries forward
    last_duration: Duration,

    // Time-signature–based automatic bar splitting
    /// Duration of one full measure under the current time signature (as fraction of whole note).
    current_time_sig: Frac,
    /// Accumulated duration of voice elements in the current measure.
    elapsed_in_measure: Frac,

    // Relative pitch state
    prev_pitch: Option<Pitch>,
    relative_ref: Option<Pitch>, // The pitch given after \relative
    in_relative: bool,

    // Pending overrides
    /// Arpeggio direction set by `\arpeggioArrowUp/Down`, `\arpeggioBracket`.
    pending_arpeggio_type: Option<ArpeggioType>,
    /// Glissando line style set by `\once \override Glissando.style = #'...`.
    pending_glissando_style: Option<String>,
    /// Whether pending glissando style is "trill" (→ slide instead of glissando).
    pending_slide: bool,
    /// Page layout accumulated from `\paper { ... }`.
    page_layout: Option<PageLayout>,
    /// Completed scores from previous `\score` blocks (multi-movement).
    completed_scores: Vec<Score>,

    // Beam/stem state
    /// Current stem direction override: "up", "down", or "" (auto).
    stem_direction: String,
    /// Whether we are inside a manual beam group (after `[`, before `]`).
    in_beam_group: bool,
    /// Stack of active tuplet ratios (actual, normal). Innermost is last.
    tuplet_stack: Vec<(u8, u8)>,
    /// Whether automatic beaming is disabled (\autoBeamOff).
    auto_beam_off: bool,
}

impl<'src> WalkState<'src> {
    fn new(source: &'src str) -> Self {
        Self {
            source,
            language: PitchLanguage::Nederlands,
            mode: PitchMode::Absolute,
            metadata: ScoreMetadata::default(),
            parts: Vec::new(),
            part_counter: 0,
            definitions: HashMap::new(),
            lyric_definitions: HashMap::new(),
            pending_lyrics: HashMap::new(),
            voice_part_map: HashMap::new(),
            var_voice_maps: HashMap::new(),
            measure_num: 0,
            current_measure: None,
            current_voice: Vec::new(),
            last_duration: Duration::quarter(),
            current_time_sig: Frac::new(4, 4), // default 4/4 = 1 whole note
            elapsed_in_measure: Frac::from_integer(0),
            prev_pitch: None,
            relative_ref: None,
            in_relative: false,
            pending_arpeggio_type: None,
            pending_glissando_style: None,
            pending_slide: false,
            page_layout: None,
            completed_scores: Vec::new(),
            stem_direction: String::new(),
            in_beam_group: false,
            tuplet_stack: Vec::new(),
            auto_beam_off: false,
        }
    }

    /// Get the text content of a node.
    fn text(&self, node: Node) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// Flush current voice elements into the current measure.
    fn flush_voice(&mut self) {
        if self.current_voice.is_empty() {
            return;
        }
        let voice = Voice {
            number: 1,
            elements: std::mem::take(&mut self.current_voice),
        };
        let measure = self.ensure_measure();
        measure.voices.push(voice);
    }

    /// Ensure there's a current measure, creating one if needed.
    fn ensure_measure(&mut self) -> &mut Measure {
        if self.current_measure.is_none() {
            self.measure_num += 1;
            self.current_measure = Some(Measure::new(self.measure_num));
        }
        self.current_measure.as_mut().unwrap()
    }

    /// Flush the current measure into the current part.
    fn flush_measure(&mut self) {
        self.flush_voice();
        if let Some(measure) = self.current_measure.take() {
            self.ensure_part().measures.push(measure);
        }
    }

    /// Start a new measure (bar check encountered).
    fn bar_check(&mut self) {
        self.flush_measure();
        self.elapsed_in_measure = Frac::from_integer(0);
    }

    /// Push a voice element and auto-split the measure if it's full.
    fn push_voice_element(&mut self, mut elem: VoiceElement) {
        // Apply active tuplet ratio to the element's duration
        if let Some(&(actual, normal)) = self.tuplet_stack.last() {
            apply_tuplet_ratio(&mut elem, actual, normal);
        }

        let dur = voice_element_duration(&elem);

        // Before pushing, check if the current measure is already full.
        // If adding this element would start a new beat cycle, flush first.
        // Use a loop in case a single element spans multiple measures
        // (e.g. a whole rest in 2/4 time).
        while self.current_time_sig > Frac::from_integer(0)
            && self.elapsed_in_measure >= self.current_time_sig
        {
            self.flush_measure();
            self.elapsed_in_measure = self.elapsed_in_measure - self.current_time_sig;
        }

        // Apply beam "continue" for notes inside a manual beam group
        // (notes with explicit [/] already have begin/end set by apply_note_attachments)
        if self.in_beam_group {
            match &mut elem {
                VoiceElement::Note(n) if n.beams.is_empty() => {
                    let level = beam_level_for_duration(&n.duration);
                    if level > 0 {
                        n.beams.push(BeamEvent {
                            beam_type: "continue".to_string(),
                            number: 1,
                        });
                    }
                }
                VoiceElement::Chord(c) if !c.notes.is_empty() => {
                    let level = beam_level_for_duration(&c.duration);
                    if level > 0 && c.notes[0].beams.is_empty() {
                        c.notes[0].beams.push(BeamEvent {
                            beam_type: "continue".to_string(),
                            number: 1,
                        });
                    }
                }
                _ => {}
            }
        }

        // Apply current stem direction override to notes
        if !self.stem_direction.is_empty() {
            match &mut elem {
                VoiceElement::Note(n) => {
                    if n.stem_direction.is_empty() {
                        n.stem_direction = self.stem_direction.clone();
                    }
                }
                VoiceElement::Chord(c) => {
                    for n in &mut c.notes {
                        if n.stem_direction.is_empty() {
                            n.stem_direction = self.stem_direction.clone();
                        }
                    }
                }
                _ => {}
            }
        }

        // Mark notes with no_auto_beam when \autoBeamOff is active
        if self.auto_beam_off {
            match &mut elem {
                VoiceElement::Note(n) => n.no_auto_beam = true,
                VoiceElement::Chord(c) => {
                    for n in &mut c.notes {
                        n.no_auto_beam = true;
                    }
                }
                _ => {}
            }
        }

        // Grace notes don't consume time in the measure
        let is_grace = matches!(&elem, VoiceElement::Note(n) if n.is_grace);
        self.current_voice.push(elem);
        if !is_grace {
            self.elapsed_in_measure = self.elapsed_in_measure + dur;
        }
    }

    /// Update the current time signature (called when \time is parsed).
    fn set_time_signature(&mut self, beats: u32, beat_type: u32) {
        self.current_time_sig = Frac::new(beats as i64, beat_type as i64);
    }

    /// Get or create the current part.
    fn ensure_part(&mut self) -> &mut Part {
        if self.parts.is_empty() {
            self.part_counter += 1;
            let id = format!("P{}", self.part_counter);
            let part = Part::new(&id);
            self.parts.push(("Staff".to_string(), part));
        }
        &mut self.parts.last_mut().unwrap().1
    }

    /// Start a new part for a named context.
    fn new_part(&mut self, context: &str, name: &str) {
        self.flush_measure();
        self.part_counter += 1;
        let id = format!("P{}", self.part_counter);
        let mut part = Part::new(&id);
        if !name.is_empty() {
            part.name = name.to_string();
        }
        self.parts.push((context.to_string(), part));
        self.measure_num = 0;
        self.elapsed_in_measure = Frac::from_integer(0);
        self.prev_pitch = self.relative_ref;
    }

    /// Resolve a variable reference: look up stored measures and add them
    /// to the current part.
    fn resolve_variable(&mut self, name: &str) -> bool {
        if let Some(def) = self.definitions.get(name) {
            let def = def.clone();
            match def {
                VarDef::Parts(parts) => {
                    // Flush current state and add the stored parts directly
                    self.flush_measure();
                    let base_idx = self.parts.len();
                    self.parts.extend(parts);
                    // Update voice_part_map: if any stored voice names pointed
                    // to indices within the variable's local parts, remap them
                    // to the new global indices.
                    if let Some(voice_map) = self.var_voice_maps.get(name) {
                        for (voice_name, local_idx) in voice_map {
                            self.voice_part_map
                                .insert(voice_name.clone(), base_idx + local_idx);
                        }
                    }
                }
                VarDef::Measures(measures) => {
                    if measures.is_empty() {
                        return true; // empty variable, no-op
                    }
                    // Flush any in-progress measure before adding pre-split measures
                    self.flush_measure();
                    self.elapsed_in_measure = Frac::from_integer(0);
                    // If this variable had voice name mappings, apply them to the current part
                    {
                        let _ = self.ensure_part(); // ensure part exists
                        let part_idx = self.parts.len() - 1;
                        if let Some(voice_map) = self.var_voice_maps.get(name) {
                            for (voice_name, _) in voice_map {
                                self.voice_part_map
                                    .insert(voice_name.clone(), part_idx);
                            }
                        }
                    }
                    let part = self.ensure_part();
                    // If the part already has measures and the incoming measures
                    // contain only spacer rests (e.g. from a \forma variable in
                    // parallel music), merge attributes into existing measures
                    // rather than appending.
                    if !part.measures.is_empty() && measures_are_spacer_only(&measures) {
                        if measures.len() != part.measures.len() {
                            // Measure counts differ — re-split music to match spacer boundaries
                            part.measures = resplit_measures_to_match(&part.measures, &measures);
                        } else {
                            merge_spacer_measures(&mut part.measures, &measures);
                        }
                    } else if part.measures.is_empty()
                        || !measures_are_spacer_only(&part.measures)
                    {
                        part.measures.extend(measures);
                    } else {
                        // Existing measures are spacer-only, incoming are real music —
                        // re-split to match spacer boundaries if needed, then replace.
                        if measures.len() != part.measures.len() {
                            part.measures = resplit_measures_to_match(&measures, &part.measures);
                        } else {
                            let mut incoming = measures;
                            merge_spacer_measures(&mut incoming, &part.measures);
                            part.measures = incoming;
                        }
                    }
                }
                VarDef::FiguredBass(entries) => {
                    // Distribute figured bass entries across measures by tracking
                    // cumulative duration. Each measure's duration is determined
                    // by its time signature (from attributes).
                    let part = self.ensure_part();
                    if part.measures.is_empty() {
                        // No measures to attach to — skip
                    } else {
                        distribute_figured_bass(&mut part.measures, &entries);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Resolve a pitch from a symbol node, handling relative mode.
    fn resolve_pitch(&mut self, step: PitchStep, alter: Ratio<i32>, octave_marks: i32) -> Pitch {
        if self.in_relative {
            if let Some(ref prev) = self.prev_pitch {
                // In relative mode: find closest pitch within a fourth, then apply marks
                let inferred_octave = find_relative_octave(prev, step);
                let octave = inferred_octave + octave_marks;
                let pitch = Pitch::with_alter(step, alter, octave);
                self.prev_pitch = Some(pitch);
                pitch
            } else {
                // First note after \relative: use the reference pitch's octave
                let base_oct = self
                    .relative_ref
                    .as_ref()
                    .map(|r| r.octave)
                    .unwrap_or(4);
                let octave = base_oct + octave_marks;
                let pitch = Pitch::with_alter(step, alter, octave);
                self.prev_pitch = Some(pitch);
                pitch
            }
        } else {
            // Absolute mode: octave marks relative to LilyPond c (octave 3 in our numbering)
            let octave = 3 + octave_marks;
            Pitch::with_alter(step, alter, octave)
        }
    }
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
        let mut parser =
            LilyPondParser::new().map_err(|e| AdapterError::Parse(e.to_string()))?;
        let tree = parser
            .parse(source)
            .map_err(|e| AdapterError::Parse(e.to_string()))?;

        let root = tree.root_node();
        if root.has_error() {
            // Still attempt to extract what we can; tree-sitter is error-tolerant
        }

        let mut state = WalkState::new(source);
        state.language = self.language;

        walk_program(&mut state, root);

        // If walk_program collected scores from \score blocks, return those
        if !state.completed_scores.is_empty() {
            for score in &mut state.completed_scores {
                post_process_beams_and_stems(score);
            }
            return Ok(state.completed_scores);
        }

        // Otherwise, build a single score from remaining state (no \score blocks)
        state.flush_measure();

        // Attach pending lyrics to matching parts
        for (voice_name, syllables) in &state.pending_lyrics {
            if let Some(&part_idx) = state.voice_part_map.get(voice_name) {
                if let Some((_, part)) = state.parts.get_mut(part_idx) {
                    attach_lyrics_to_part(part, syllables);
                }
            }
        }

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

        post_process_beams_and_stems(&mut score);
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

// ---------------------------------------------------------------------------
// Tree walk
// ---------------------------------------------------------------------------

/// Walk the `lilypond_program` root node.
fn walk_program(state: &mut WalkState, root: Node) {
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "escaped_word" => {
                let text = state.text(node);
                match text {
                    "\\version" => {
                        // Skip version string; \version "2.24.0" consumes next string
                        i += 1; // skip string
                    }
                    "\\language" => {
                        // \language "english"
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "string" {
                                let lang_str = extract_string_value(state, *next);
                                state.language = match lang_str.as_str() {
                                    "english" => PitchLanguage::English,
                                    "deutsch" | "german" => PitchLanguage::Deutsch,
                                    "italiano" | "italian" => PitchLanguage::Italiano,
                                    "espanol" | "español" | "spanish" => PitchLanguage::Espanol,
                                    "français" | "francais" | "french" => PitchLanguage::Nederlands, // no dedicated French; default
                                    "portugues" | "português" | "portuguese" => {
                                        PitchLanguage::Portugues
                                    }
                                    "vlaams" | "flemish" => PitchLanguage::Vlaams,
                                    "norsk" | "norwegian" => PitchLanguage::Norsk,
                                    "suomi" | "finnish" => PitchLanguage::Suomi,
                                    "svenska" | "swedish" => PitchLanguage::Svenska,
                                    "catalan" => PitchLanguage::Catalan,
                                    _ => PitchLanguage::Nederlands,
                                };
                                i += 1; // skip string
                            }
                        }
                    }
                    "\\header" => {
                        // \header { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                walk_header(state, *next);
                                i += 1;
                            }
                        }
                    }
                    "\\score" => {
                        // \score { ... } — each score block becomes a separate movement
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                // Save and reset parts for this score block
                                state.flush_measure();
                                let saved_parts = std::mem::take(&mut state.parts);
                                let saved_counter = state.part_counter;
                                let saved_pending_lyrics = std::mem::take(&mut state.pending_lyrics);
                                let saved_voice_map = std::mem::take(&mut state.voice_part_map);
                                state.part_counter = 0;
                                state.measure_num = 0;
                                state.elapsed_in_measure = Frac::from_integer(0);

                                walk_score_block(state, *next);
                                state.flush_measure();

                                // Attach pending lyrics
                                for (voice_name, syllables) in &state.pending_lyrics {
                                    if let Some(&part_idx) = state.voice_part_map.get(voice_name) {
                                        if let Some((_, part)) = state.parts.get_mut(part_idx) {
                                            attach_lyrics_to_part(part, syllables);
                                        }
                                    }
                                }

                                // Build a Score from the parts created by this score block
                                if !state.parts.is_empty() {
                                    let mut score = Score::new();
                                    score.metadata = state.metadata.clone();
                                    score.metadata.pitch_mode = state.mode;
                                    score.metadata.pitch_language = Some(state.language);
                                    score.page_layout = state.page_layout.clone();
                                    for (_, part) in state.parts.drain(..) {
                                        score.children.push(ScoreChild::Part(part));
                                    }
                                    state.completed_scores.push(score);
                                }

                                // Restore saved state
                                state.parts = saved_parts;
                                state.part_counter = saved_counter;
                                state.pending_lyrics = saved_pending_lyrics;
                                state.voice_part_map = saved_voice_map;
                                state.measure_num = 0;

                                i += 1;
                            }
                        }
                    }
                    "\\relative" => {
                        // Top-level \relative c' { ... }
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        i = consume_relative(state, &children, i);
                        continue;
                    }
                    "\\paper" => {
                        // Top-level \paper { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                parse_paper_block(state, *next);
                                i += 1;
                            }
                        }
                    }
                    _ => {
                        // Top-level escaped words we don't handle
                    }
                }
            }
            "expression_block" => {
                // Bare { ... } at top level: treat as a single anonymous part
                walk_music_block(state, node);
            }
            "assignment_lhs" => {
                // Variable definition: name = { ... }
                // Extract the variable name from the assignment_lhs node
                let var_name = {
                    let mut c = node.walk();
                    let result = node
                        .children(&mut c)
                        .find(|n| n.kind() == "symbol")
                        .map(|n| state.text(n).to_string())
                        .unwrap_or_default();
                    result
                };
                if !var_name.is_empty() {
                    // Skip the "=" punctuation, then capture the body
                    if let Some(eq) = children.get(i + 1) {
                        if eq.kind() == "punctuation" && state.text(*eq) == "=" {
                            let mut j = i + 2;
                            // Skip \lyricmode, \notemode, \relative, \figuremode etc. before the expression_block
                            let mut is_lyricmode = false;
                            let mut is_figuremode = false;
                            while j < children.len() {
                                let candidate = children[j];
                                if candidate.kind() == "escaped_word" {
                                    let ew = state.text(candidate);
                                    if ew == "\\lyricmode" || ew == "\\notemode" {
                                        is_lyricmode = ew == "\\lyricmode";
                                        j += 1;
                                        continue;
                                    }
                                    if ew == "\\relative" {
                                        state.in_relative = true;
                                        state.mode = PitchMode::Relative;
                                        j += 1;
                                        // Consume optional reference pitch and octave marks
                                        let mut octave_marks = 0i32;
                                        while j < children.len() {
                                            let n = children[j];
                                            if n.kind() == "symbol" {
                                                let sym = state.text(n).to_string();
                                                if let Some((step, alter)) = parse_pitch_name(&sym, state.language) {
                                                    let mut rp = Pitch::with_alter(step, alter, 3);
                                                    j += 1;
                                                    while j < children.len() {
                                                        let m = children[j];
                                                        if m.kind() == "punctuation" {
                                                            let t = state.text(m);
                                                            if t == "'" { octave_marks += 1; j += 1; }
                                                            else if t == "," { octave_marks -= 1; j += 1; }
                                                            else { break; }
                                                        } else { break; }
                                                    }
                                                    rp.octave = 3 + octave_marks;
                                                    state.relative_ref = Some(rp);
                                                    state.prev_pitch = Some(rp);
                                                }
                                                break;
                                            } else if n.kind() == "punctuation" {
                                                let t = state.text(n);
                                                if t == "'" || t == "," { j += 1; continue; }
                                                break;
                                            } else {
                                                break;
                                            }
                                        }
                                        continue;
                                    }
                                    if ew == "\\figuremode" || ew == "\\figures" {
                                        is_figuremode = true;
                                        j += 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                            if let Some(next) = children.get(j) {
                                if next.kind() == "expression_block" && is_figuremode {
                                    // Parse figuremode block into figured bass entries
                                    let fb_measures = parse_figuremode_block(state, *next);
                                    state.definitions.insert(var_name, VarDef::FiguredBass(fb_measures));
                                    i = j + 1;
                                    continue;
                                } else if next.kind() == "expression_block" {
                                    if is_lyricmode {
                                        // Parse lyrics variable
                                        let lyrics = parse_lyric_block(state, *next);
                                        state.lyric_definitions.insert(var_name, lyrics);
                                    } else {
                                        // Check if the block contains a \new Staff/PianoStaff
                                        let has_named_context =
                                            block_contains_named_context(state, *next);
                                        // Walk the block but capture the parts it creates
                                        let parts_before = state.parts.len();
                                        let old_measure_num = state.measure_num;
                                        let old_relative = state.in_relative;
                                        let old_mode = state.mode;
                                        let old_relative_ref = state.relative_ref;
                                        let old_prev_pitch = state.prev_pitch;
                                        let old_elapsed = state.elapsed_in_measure;
                                        let old_time_sig = state.current_time_sig;
                                        state.measure_num = 0;
                                        state.elapsed_in_measure = Frac::from_integer(0);
                                        walk_music_block(state, *next);
                                        state.flush_measure();
                                        // Restore state so variable definitions
                                        // don't leak context
                                        state.in_relative = old_relative;
                                        state.mode = old_mode;
                                        state.relative_ref = old_relative_ref;
                                        state.prev_pitch = old_prev_pitch;
                                        state.elapsed_in_measure = old_elapsed;
                                        state.current_time_sig = old_time_sig;
                                        // Extract newly created parts
                                        let new_parts: Vec<_> =
                                            state.parts.drain(parts_before..).collect();
                                        // Capture voice→part mappings created during this variable def
                                        let local_voice_map: HashMap<String, usize> = state
                                            .voice_part_map
                                            .iter()
                                            .filter(|(_, idx)| **idx >= parts_before)
                                            .map(|(name, idx)| (name.clone(), idx - parts_before))
                                            .collect();
                                        if !local_voice_map.is_empty() {
                                            state.var_voice_maps.insert(var_name.clone(), local_voice_map);
                                        }
                                        // Remove stale entries from voice_part_map
                                        state.voice_part_map.retain(|_, idx| *idx < parts_before);
                                        // If the block explicitly contained \new Staff,
                                        // store as full Parts to preserve metadata
                                        let def = if has_named_context {
                                            VarDef::Parts(new_parts)
                                        } else {
                                            let measures: Vec<Measure> = new_parts
                                                .into_iter()
                                                .flat_map(|(_, part)| part.measures)
                                                .collect();
                                            VarDef::Measures(measures)
                                        };
                                        state.definitions.insert(var_name, def);
                                        state.measure_num = old_measure_num;
                                    }
                                    i = j + 1; // skip to after block
                                    continue;
                                } else if next.kind() == "named_context" {
                                    // Variable is `name = \new Staff { ... }`
                                    // The named_context is followed by expression_block
                                    let parts_before = state.parts.len();
                                    let old_measure_num = state.measure_num;
                                    let old_elapsed = state.elapsed_in_measure;
                                    let old_time_sig = state.current_time_sig;
                                    state.measure_num = 0;
                                    state.elapsed_in_measure = Frac::from_integer(0);
                                    let (context, ctx_name) =
                                        extract_named_context(state, *next);
                                    j += 1;
                                    j = walk_context_body(
                                        state, &children, j, &context, &ctx_name,
                                    );
                                    state.flush_measure();
                                    let new_parts: Vec<_> =
                                        state.parts.drain(parts_before..).collect();
                                    // Capture voice→part mappings created during this variable def
                                    let local_voice_map: HashMap<String, usize> = state
                                        .voice_part_map
                                        .iter()
                                        .filter(|(_, idx)| **idx >= parts_before)
                                        .map(|(name, idx)| (name.clone(), idx - parts_before))
                                        .collect();
                                    if !local_voice_map.is_empty() {
                                        state.var_voice_maps.insert(var_name.clone(), local_voice_map);
                                    }
                                    // Remove stale entries from voice_part_map
                                    state.voice_part_map.retain(|_, idx| *idx < parts_before);
                                    state
                                        .definitions
                                        .insert(var_name, VarDef::Parts(new_parts));
                                    state.measure_num = old_measure_num;
                                    state.elapsed_in_measure = old_elapsed;
                                    state.current_time_sig = old_time_sig;
                                    i = j;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Walk a `\header { ... }` block.
fn walk_header(state: &mut WalkState, block: Node) {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        if node.kind() == "assignment_lhs" {
            let key = {
                // assignment_lhs has a child symbol
                let mut c = node.walk();
                let result = node.children(&mut c)
                    .find(|n| n.kind() == "symbol")
                    .map(|n| state.text(n).to_string())
                    .unwrap_or_default();
                result
            };

            // Skip the "=" punctuation
            // Then find the next string value
            let mut j = i + 1;
            while j < children.len() {
                let val_node = children[j];
                if val_node.kind() == "string" {
                    let val = extract_string_value(state, val_node);
                    match key.as_str() {
                        "title" => state.metadata.title = Some(val),
                        "subtitle" => state.metadata.subtitle = Some(val),
                        "composer" => state.metadata.composer = Some(val),
                        "arranger" => state.metadata.arranger = Some(val),
                        "poet" | "lyricist" => state.metadata.lyricist = Some(val),
                        k if !k.is_empty() => {
                            state.metadata.extra.insert(k.to_string(), val);
                        }
                        _ => {}
                    }
                    i = j;
                    break;
                } else if val_node.kind() == "assignment_lhs" || val_node.kind() == "}" {
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
}

/// Walk a `\score { ... }` block.
fn walk_score_block(state: &mut WalkState, block: Node) {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "named_context" => {
                // \new Staff ..., \new Voice ..., etc.
                let (context, name) = extract_named_context(state, node);
                i += 1;
                // Check what follows: \relative, expression_block, etc.
                i = walk_context_body(state, &children, i, &context, &name);
                continue; // walk_context_body already advanced i
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                match text.as_str() {
                    "\\relative" => {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        // May be followed by a reference pitch, then expression_block
                        i = consume_relative(state, &children, i);
                        continue;
                    }
                    "\\layout" | "\\midi" => {
                        // Skip these blocks
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                i += 1;
                            }
                        }
                    }
                    "\\paper" => {
                        // \paper { ... } inside \score
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                parse_paper_block(state, *next);
                                i += 1;
                            }
                        }
                    }
                    "\\header" => {
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                walk_header(state, *next);
                                i += 1;
                            }
                        }
                    }
                    _ => {
                        let var_name = text.trim_start_matches('\\');
                        state.resolve_variable(var_name);
                    }
                }
            }
            "expression_block" => {
                // Music inside score block without explicit context
                walk_music_block(state, node);
            }
            "parallel_music" => {
                walk_parallel_music(state, node);
            }
            _ => {}
        }
        i += 1;
    }
}

/// Walk a `<< ... >>` parallel music block.
fn walk_parallel_music(state: &mut WalkState, node: Node) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let child = children[i];
        match child.kind() {
            "named_context" => {
                let (context, name) = extract_named_context(state, child);
                i += 1;
                i = walk_context_body(state, &children, i, &context, &name);
                continue;
            }
            "expression_block" => {
                // Voice within parallel music
                walk_music_block(state, child);
            }
            "parallel_music_separator" => {
                // \\ — separates voices
                state.flush_measure();
            }
            "escaped_word" => {
                let text = state.text(child).to_string();
                if text == "\\relative" {
                    state.in_relative = true;
                    state.mode = PitchMode::Relative;
                    i += 1;
                    i = consume_relative(state, &children, i);
                    continue;
                } else {
                    let var_name = text.trim_start_matches('\\');
                    state.resolve_variable(var_name);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Extract context type and context name from a `named_context` node.
/// For `\new Staff`, returns ("Staff", "").
/// For `\context Voice = "melodySop"`, returns ("Voice", "melodySop").
fn extract_named_context<'a>(state: &WalkState<'a>, node: Node<'a>) -> (String, String) {
    let mut context = String::new();
    let mut name = String::new();
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    let mut found_symbol = false;
    let mut found_eq = false;
    for child in &children {
        match child.kind() {
            "symbol" if !found_symbol => {
                context = state.text(*child).to_string();
                found_symbol = true;
            }
            "punctuation" if found_symbol && state.text(*child) == "=" => {
                found_eq = true;
            }
            "string" if found_eq => {
                name = extract_string_value(state, *child);
            }
            _ => {}
        }
    }
    (context, name)
}

/// After parsing a `named_context` node, consume the body (which might be
/// `\relative { ... }`, `{ ... }`, `\with { ... } { ... }`, etc.).
/// Returns the next index to process.
fn walk_context_body(
    state: &mut WalkState,
    children: &[Node],
    mut i: usize,
    context: &str,
    name: &str,
) -> usize {
    // Lyrics context: don't create a music part, parse lyrics instead
    if context == "Lyrics" {
        return walk_lyrics_context(state, children, i, name);
    }

    // Voice context: don't create a new part, but set voice name on current part
    if context == "Voice" {
        // Don't call new_part — we stay in the current Staff part
        // Just consume the body block
        while i < children.len() {
            let node = children[i];
            match node.kind() {
                "expression_block" => {
                    walk_music_block(state, node);
                    i += 1;
                    break;
                }
                "escaped_word" => {
                    let text = state.text(node).to_string();
                    if text == "\\relative" {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        i = consume_relative(state, children, i);
                        continue;
                    }
                    break;
                }
                _ => break,
            }
        }
        // Store the voice name for lyrics attachment
        if !name.is_empty() {
            let part = state.ensure_part();
            if part.part_id.is_empty() || part.part_id.starts_with('P') {
                // Use the voice name to help identify this part for lyrics
            }
            // Store voice name → part index mapping
            let part_idx = state.parts.len().saturating_sub(1);
            state
                .voice_part_map
                .insert(name.to_string(), part_idx);
        }
        return i;
    }

    // Grouping contexts (ChoirStaff, StaffGroup, etc.) don't produce a part;
    // they just wrap inner staves. Walk their body like a score block.
    let is_grouping = matches!(
        context,
        "ChoirStaff" | "StaffGroup" | "GrandStaff" | "PianoStaff"
    );

    if is_grouping {
        // Skip optional \with { ... }
        while i < children.len() {
            let node = children[i];
            if node.kind() == "escaped_word" && state.text(node) == "\\with" {
                if let Some(next) = children.get(i + 1) {
                    if next.kind() == "expression_block" {
                        i += 2;
                        continue;
                    }
                }
            }
            break;
        }
        // Consume the body block (parallel music or expression_block)
        if let Some(node) = children.get(i) {
            match node.kind() {
                "parallel_music" => {
                    walk_parallel_music(state, *node);
                    i += 1;
                }
                "expression_block" => {
                    walk_score_block(state, *node);
                    i += 1;
                }
                _ => {}
            }
        }
        return i;
    }

    // Remember old relative state
    let was_relative = state.in_relative;
    let old_ref = state.relative_ref;
    let old_prev = state.prev_pitch;

    state.new_part(context, name);

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "escaped_word" => {
                let text = state.text(node).to_string();
                match text.as_str() {
                    "\\relative" => {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        i = consume_relative(state, children, i);
                        continue;
                    }
                    "\\with" => {
                        // Skip \with { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                i += 2;
                                continue;
                            }
                        }
                    }
                    _ => {
                        let var_name = text.trim_start_matches('\\');
                        if state.resolve_variable(var_name) {
                            i += 1;
                        }
                        break;
                    }
                }
            }
            "expression_block" => {
                walk_music_block(state, node);
                i += 1;
                break;
            }
            "parallel_music" => {
                walk_parallel_music(state, node);
                i += 1;
                break;
            }
            _ => break,
        }
        i += 1;
    }

    // Restore relative state for other parts
    state.in_relative = was_relative;
    state.relative_ref = old_ref;
    state.prev_pitch = old_prev;

    i
}

/// Handle `\context Lyrics = "name" \lyricmode { \lyricsto "voice" ... }`
/// or `\new Lyrics \lyricsto "voice" \variable`
/// Consumes tokens after the named_context and stores lyrics for later attachment.
fn walk_lyrics_context(
    state: &mut WalkState,
    children: &[Node],
    mut i: usize,
    _name: &str,
) -> usize {
    // After named_context(Lyrics), we may see:
    //   1. \lyricmode { \lyricsto "voiceName" ... }
    //   2. \lyricsto "voiceName" \variable
    let mut is_lyricmode = false;
    let mut lyricsto_voice: Option<String> = None;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "escaped_word" => {
                let text = state.text(node);
                if text == "\\lyricmode" {
                    is_lyricmode = true;
                    i += 1;
                    continue;
                }
                if text == "\\lyricsto" {
                    // \lyricsto "voiceName" — consume voice name
                    i += 1;
                    if let Some(name_node) = children.get(i) {
                        if name_node.kind() == "string" {
                            lyricsto_voice = Some(extract_string_value(state, *name_node));
                            i += 1;
                        }
                    }
                    continue;
                }
                // Could be a variable reference like \Itesto
                if let Some(voice) = &lyricsto_voice {
                    let var_name = text.trim_start_matches('\\');
                    if let Some(syllables) = state.lyric_definitions.get(var_name) {
                        state.pending_lyrics.insert(voice.clone(), syllables.clone());
                    }
                }
                i += 1;
                break;
            }
            "expression_block" => {
                if is_lyricmode || lyricsto_voice.is_some() {
                    // Parse the lyric block and find the \lyricsto voice name
                    let voice_name = if lyricsto_voice.is_some() {
                        lyricsto_voice.clone()
                    } else {
                        extract_lyricsto_voice(state, node)
                    };
                    let syllables = parse_lyric_block(state, node);
                    if let Some(voice) = voice_name {
                        state.pending_lyrics.insert(voice, syllables);
                    }
                }
                i += 1;
                break;
            }
            _ => break,
        }
    }
    i
}

/// Consume optional reference pitch after `\relative`, then the music block.
/// Returns the next index.
fn consume_relative(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    // After \relative we may see:
    //   - a pitch symbol (reference pitch), octave marks, then expression_block
    //   - directly an expression_block
    let mut octave_marks = 0i32;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let sym = state.text(node).to_string();
                if let Some((step, alter)) = parse_pitch_name(&sym, state.language) {
                    // This is the reference pitch
                    let mut rp = Pitch::with_alter(step, alter, 3); // base octave
                    i += 1;
                    // Consume octave marks
                    while i < children.len() {
                        let n = children[i];
                        if n.kind() == "punctuation" {
                            let t = state.text(n);
                            if t == "'" {
                                octave_marks += 1;
                                i += 1;
                            } else if t == "," {
                                octave_marks -= 1;
                                i += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    rp.octave = 3 + octave_marks;
                    state.relative_ref = Some(rp);
                    state.prev_pitch = Some(rp);
                    continue;
                } else {
                    break;
                }
            }
            "punctuation" => {
                let t = state.text(node);
                if t == "'" || t == "," {
                    // Stray octave marks (shouldn't happen without a pitch before)
                    i += 1;
                    continue;
                }
                break;
            }
            "expression_block" => {
                // Found the music block
                if state.relative_ref.is_none() {
                    // No reference pitch given; default to middle C
                    state.relative_ref = Some(Pitch::new(PitchStep::C, 4));
                    state.prev_pitch = state.relative_ref;
                }
                walk_music_block(state, node);
                i += 1;
                return i;
            }
            _ => break,
        }
    }
    i
}

/// Walk an `expression_block` `{ ... }` containing music.
/// This is the core of the parser: reads symbols, durations, commands
/// and builds notes/rests/chords.
fn walk_music_block(state: &mut WalkState, block: Node) {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let sym = state.text(node).to_string();
                i = handle_symbol(state, &children, i, &sym);
                continue;
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                i = handle_escaped_word(state, &children, i, &text);
                continue;
            }
            "dynamic" => {
                // Attach dynamic to most recent note
                let dyn_text = state.text(node).to_string();
                attach_dynamic(state, &dyn_text);
            }
            "chord" => {
                // < ... >
                let chord_node = node;
                i += 1;
                // Consume duration after chord
                let dur = consume_duration(state, &children, &mut i);
                let attachments = consume_attachments(state, &children, &mut i);
                let chord = build_chord(state, chord_node, dur);
                let mut chord = chord;
                apply_chord_attachments(state, &mut chord, &attachments);
                state.push_voice_element(VoiceElement::Chord(chord));
                continue;
            }
            "punctuation" => {
                let punc = punct_text(state, node);
                handle_punctuation(state, &punc);
            }
            "expression_block" => {
                // Nested block: could be a voice or sub-expression
                walk_music_block(state, node);
            }
            "parallel_music" => {
                walk_parallel_music(state, node);
            }
            "named_context" => {
                let (context, name) = extract_named_context(state, node);
                i += 1;
                i = walk_context_body(state, &children, i, &context, &name);
                continue;
            }
            "fraction" => {
                // Standalone fraction (shouldn't appear without \time, but handle gracefully)
            }
            "{" | "}" | "<<" | ">>" | "comment" | "unsigned_integer" | "string" => {
                // Skip structural tokens and standalone numbers
            }
            _ => {}
        }
        i += 1;
    }
}

/// Handle a symbol node (pitch name, r, R, s, etc.).
/// Returns the next index to process.
fn handle_symbol(state: &mut WalkState, children: &[Node], i: usize, sym: &str) -> usize {
    let mut i = i + 1;

    match sym {
        "r" => {
            // Regular rest
            let dur = consume_duration(state, children, &mut i);
            let attachments = consume_attachments(state, children, &mut i);
            let mut rest = Rest::new(dur);
            apply_rest_attachments(&mut rest, &attachments);
            state.push_voice_element(VoiceElement::Rest(rest));
        }
        "R" => {
            // Whole-measure rest, possibly with *N multiplier (e.g. R1*3)
            let dur = consume_duration(state, children, &mut i);
            // Check for *N multiplier for multi-measure rests
            let count = consume_duration_multiplier(state, children, &mut i);
            let attachments = consume_attachments(state, children, &mut i);
            let mut rest = Rest::measure_rest(dur.clone());
            apply_rest_attachments(&mut rest, &attachments);
            state.push_voice_element(VoiceElement::Rest(rest));
            // Expand R1*N into N separate measure rests with bar checks
            if count > 1 {
                for _ in 1..count {
                    state.bar_check();
                    let rest = Rest::measure_rest(dur.clone());
                    state.push_voice_element(VoiceElement::Rest(rest));
                }
            }
        }
        "s" => {
            // Spacer rest, possibly with *N multiplier (e.g. s1*62)
            let dur = consume_duration(state, children, &mut i);
            let count = consume_duration_multiplier(state, children, &mut i);
            let mut rest = Rest::new(dur.clone());
            rest.is_spacer = true;
            state.push_voice_element(VoiceElement::Rest(rest));
            if count > 1 {
                for _ in 1..count {
                    state.bar_check();
                    let mut rest = Rest::new(dur.clone());
                    rest.is_spacer = true;
                    state.push_voice_element(VoiceElement::Rest(rest));
                }
            }
        }
        _ => {
            // Try as pitch name
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                // Consume octave marks
                let octave_marks = consume_octave_marks(state, children, &mut i);
                // Consume accidental forcing marks (! = forced, ? = cautionary)
                consume_accidental_marks(state, children, &mut i);
                let dur = consume_duration(state, children, &mut i);
                let attachments = consume_attachments(state, children, &mut i);

                let pitch = state.resolve_pitch(step, alter, octave_marks);
                let mut note = Note::new(pitch, dur);
                apply_note_attachments(state, &mut note, &attachments);
                state.push_voice_element(VoiceElement::Note(Box::new(note)));
            }
            // If not a pitch name, ignore (could be a context name etc.)
        }
    }
    i
}

/// Handle an escaped_word node (\key, \time, \clef, \grace, etc.).
/// Returns the next index.
fn handle_escaped_word(
    state: &mut WalkState,
    children: &[Node],
    i: usize,
    text: &str,
) -> usize {
    let mut i = i + 1;

    match text {
        "\\key" => {
            // \key <pitch> \<mode>
            if let Some(pitch_node) = children.get(i) {
                if pitch_node.kind() == "symbol" {
                    let sym = state.text(*pitch_node);
                    if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                        i += 1;
                        // Next: \major, \minor, etc.
                        let mode = if let Some(mode_node) = children.get(i) {
                            if mode_node.kind() == "escaped_word" {
                                let m = state.text(*mode_node);
                                i += 1;
                                parse_key_mode(m)
                            } else {
                                KeyMode::Major
                            }
                        } else {
                            KeyMode::Major
                        };
                        let fifths = pitch_to_fifths(step, alter, mode) as i8;
                        let ks = KeySignature { fifths, mode };
                        let measure = state.ensure_measure();
                        if measure.attributes.is_none() {
                            measure.attributes = Some(MeasureAttributes::default());
                        }
                        measure.attributes.as_mut().unwrap().key = Some(ks);
                    }
                }
            }
        }
        "\\time" => {
            // \time <fraction>
            if let Some(frac_node) = children.get(i) {
                if frac_node.kind() == "fraction" {
                    let frac_text = state.text(*frac_node);
                    if let Some((num, den)) = parse_fraction(frac_text) {
                        let symbol = match (num, den) {
                            (4, 4) => Some("common".to_string()),
                            (2, 2) => Some("cut".to_string()),
                            _ => None,
                        };
                        let ts = TimeSignature {
                            beats: num.to_string(),
                            beat_type: den as u8,
                            symbol,
                        };
                        state.set_time_signature(num, den);
                        let measure = state.ensure_measure();
                        if measure.attributes.is_none() {
                            measure.attributes = Some(MeasureAttributes::default());
                        }
                        measure.attributes.as_mut().unwrap().time = Some(ts);
                    }
                    i += 1;
                }
            }
        }
        "\\clef" => {
            // \clef <symbol> or \clef "<string>"
            if let Some(clef_node) = children.get(i) {
                let clef_name = if clef_node.kind() == "symbol" {
                    let name = state.text(*clef_node).to_string();
                    i += 1;
                    name
                } else if clef_node.kind() == "string" {
                    let name = extract_string_value(state, *clef_node);
                    i += 1;
                    name
                } else {
                    String::new()
                };
                if !clef_name.is_empty() {
                    if let Some((sign, line, oct_change)) = parse_clef_name(&clef_name) {
                        let clef = Clef {
                            sign,
                            line,
                            octave_change: oct_change,
                        };
                        let measure = state.ensure_measure();
                        if measure.attributes.is_none() {
                            measure.attributes = Some(MeasureAttributes::default());
                        }
                        measure.attributes.as_mut().unwrap().clefs.insert(1, clef);
                    }
                }
            }
        }
        "\\tempo" => {
            // \tempo "text" dur = bpm  OR  \tempo dur = bpm  OR  \tempo "text"
            i = consume_tempo(state, children, i);
        }
        "\\grace" | "\\acciaccatura" | "\\appoggiatura" => {
            // \grace { notes }  OR  \grace note (single unbraced note)
            let is_slash = text == "\\acciaccatura";
            if let Some(next_node) = children.get(i) {
                if next_node.kind() == "expression_block" {
                    // Parse grace notes from the block
                    let grace_notes = parse_grace_block(state, *next_node);
                    for mut note in grace_notes {
                        note.is_grace = true;
                        note.grace_slash = is_slash;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                    i += 1;
                } else if next_node.kind() == "symbol" {
                    // Single unbraced grace note, e.g. \acciaccatura d''8
                    let sym = state.text(*next_node);
                    if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                        i += 1;
                        let octave_marks = consume_octave_marks(state, children, &mut i);
                        let dur = consume_duration(state, children, &mut i);
                        let attachments = consume_attachments(state, children, &mut i);
                        let pitch = state.resolve_pitch(step, alter, octave_marks);
                        let mut note = Note::new(pitch, dur);
                        apply_note_attachments(state, &mut note, &attachments);
                        note.is_grace = true;
                        note.grace_slash = is_slash;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                }
            }
        }
        "\\tuplet" | "\\times" => {
            // \tuplet actual/normal { notes }  OR  \times normal/actual { notes }
            if let Some(frac_node) = children.get(i) {
                if frac_node.kind() == "fraction" {
                    let frac_text = state.text(*frac_node);
                    if let Some((num, denom)) = frac_text.split_once('/') {
                        let n: u8 = num.parse().unwrap_or(1);
                        let d: u8 = denom.parse().unwrap_or(1);
                        let (actual, normal) = if text == "\\tuplet" {
                            (n, d)
                        } else {
                            (d, n) // \times has reversed fraction
                        };
                        i += 1;
                        if let Some(block) = children.get(i) {
                            if block.kind() == "expression_block" {
                                // Push tuplet ratio so notes created inside get
                                // the scaling applied immediately (for correct
                                // measure duration tracking).
                                state.tuplet_stack.push((actual, normal));
                                let before = state.current_voice.len();
                                walk_music_block(state, *block);
                                let after = state.current_voice.len();
                                state.tuplet_stack.pop();
                                // Apply tuplet display markers (start/stop brackets)
                                if after > before {
                                    apply_tuplet_display(
                                        &mut state.current_voice[before..after],
                                        actual,
                                    );
                                }
                                i += 1;
                            }
                        }
                    }
                }
            }
        }
        "\\relative" => {
            state.in_relative = true;
            state.mode = PitchMode::Relative;
            i = consume_relative(state, children, i);
        }
        "\\fermata" => {
            // Attach fermata to most recent note/rest
            attach_fermata(state);
        }
        "\\breathe" => {
            // Attach breath mark as articulation to last note
            attach_articulation(state, "breath-mark");
        }
        "\\trill" => attach_articulation(state, "trill-mark"),
        "\\mordent" => attach_articulation(state, "mordent"),
        "\\prall" => attach_articulation(state, "inverted-mordent"),
        "\\turn" => attach_articulation(state, "turn"),
        "\\reverseturn" => attach_articulation(state, "inverted-turn"),
        "\\sustainOn" | "\\sustainOff" => {
            // Pedal events: create a direction
        }
        "\\stemUp" => {
            state.stem_direction = "up".to_string();
        }
        "\\stemDown" => {
            state.stem_direction = "down".to_string();
        }
        "\\stemNeutral" => {
            state.stem_direction.clear();
        }
        "\\repeat" => {
            // \repeat volta N { ... }
            // Skip "volta" and the number, parse the block
            if let Some(next) = children.get(i) {
                if next.kind() == "symbol" && state.text(*next) == "volta" {
                    i += 1; // skip "volta"
                    if let Some(num_node) = children.get(i) {
                        if num_node.kind() == "unsigned_integer" {
                            i += 1; // skip count
                        }
                    }
                    if let Some(block) = children.get(i) {
                        if block.kind() == "expression_block" {
                            walk_music_block(state, *block);
                            i += 1;
                        }
                    }
                }
            }
        }
        "\\bar" => {
            // \bar "||" or \bar "|."
            if let Some(bar_node) = children.get(i) {
                if bar_node.kind() == "string" {
                    let bar_text = extract_string_value(state, *bar_node);
                    let bar_type = match bar_text.as_str() {
                        "|." => BarlineType::Final,
                        "||" => BarlineType::Double,
                        "!" => BarlineType::Dashed,
                        ":|.|:" => BarlineType::RepeatBoth,
                        ":|." | ":|" => BarlineType::RepeatBackward,
                        "|:" | ".|:" => BarlineType::RepeatForward,
                        _ => BarlineType::Regular,
                    };
                    let barline = Barline {
                        style: bar_type,
                        ..Default::default()
                    };
                    let measure = state.ensure_measure();
                    measure.right_barline = Some(barline);
                    i += 1;
                    // \bar always acts as a measure boundary
                    state.bar_check();
                }
            }
        }
        "\\new" => {
            // Standalone \new (already handled in walk_score_block but may appear in blocks)
            if let Some(next) = children.get(i) {
                if next.kind() == "symbol" {
                    let context = state.text(*next).to_string();
                    i += 1;
                    i = walk_context_body(state, children, i, &context, "");
                }
            }
        }
        "\\layout" | "\\midi" => {
            // Skip these blocks
            if let Some(next) = children.get(i) {
                if next.kind() == "expression_block" {
                    i += 1;
                }
            }
        }
        "\\partial" => {
            // \partial <dur>  → anacrusis / pickup
            let dur = consume_duration(state, children, &mut i);
            state.metadata.partial_duration = Some(dur);
        }
        "\\afterGrace" => {
            // \afterGrace { notes }  OR  \afterGrace note
            if let Some(next_node) = children.get(i) {
                if next_node.kind() == "expression_block" {
                    let grace_notes = parse_grace_block(state, *next_node);
                    for mut note in grace_notes {
                        note.is_grace = true;
                        note.after_grace = true;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                    i += 1;
                } else if next_node.kind() == "symbol" {
                    let sym = state.text(*next_node);
                    if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                        i += 1;
                        let octave_marks = consume_octave_marks(state, children, &mut i);
                        let dur = consume_duration(state, children, &mut i);
                        let attachments = consume_attachments(state, children, &mut i);
                        let pitch = state.resolve_pitch(step, alter, octave_marks);
                        let mut note = Note::new(pitch, dur);
                        apply_note_attachments(state, &mut note, &attachments);
                        note.is_grace = true;
                        note.after_grace = true;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                }
            }
        }
        "\\arpeggioArrowUp" => {
            state.pending_arpeggio_type = Some(ArpeggioType::Up);
        }
        "\\arpeggioArrowDown" => {
            state.pending_arpeggio_type = Some(ArpeggioType::Down);
        }
        "\\arpeggioBracket" => {
            state.pending_arpeggio_type = Some(ArpeggioType::NonArpeggio);
        }
        "\\arpeggioNormal" => {
            state.pending_arpeggio_type = None;
        }
        "\\mark" => {
            // \mark \markup { \musicglyph "scripts.coda" }
            // \mark "D.C."  /  \mark "D.S. al Coda"
            i = consume_mark(state, children, i);
        }
        "\\once" => {
            // \once — usually followed by \override; just skip it
            // The \override handler will consume the property setting
        }
        "\\override" => {
            // \override Glissando.style = #'<style>
            i = consume_override(state, children, i);
        }
        "\\paper" => {
            // \paper { ... } — page layout
            if let Some(next) = children.get(i) {
                if next.kind() == "expression_block" {
                    parse_paper_block(state, *next);
                    i += 1;
                }
            }
        }
        "\\set" => {
            // \set Staff.instrumentName = "value"
            // Tree-sitter: assignment_lhs(property_expression(symbol, ".", symbol)) "=" string
            if let Some(lhs) = children.get(i) {
                if lhs.kind() == "assignment_lhs" {
                    let prop_text = state.text(*lhs).to_string();
                    i += 1;
                    // Skip "="
                    if let Some(eq) = children.get(i) {
                        if eq.kind() == "punctuation" && state.text(*eq) == "=" {
                            i += 1;
                        }
                    }
                    // Read value (string or scheme)
                    if let Some(val_node) = children.get(i) {
                        if val_node.kind() == "string" {
                            let val = extract_string_value(state, *val_node);
                            i += 1;
                            apply_set_property(state, &prop_text, &val);
                        } else if val_node.kind() == "embedded_scheme" {
                            // Handle \set Score.measureLength = #(ly:make-moment N D)
                            let scheme_text = state.text(*val_node);
                            if prop_text.contains("measureLength") {
                                if let Some((num, den)) = parse_ly_make_moment(scheme_text) {
                                    state.set_time_signature(num, den);
                                }
                            }
                            i += 1;
                        } else {
                            // Skip unknown value types (scheme booleans, etc.)
                            i += 1;
                        }
                    }
                }
            }
        }
        "\\autoBeamOff" => {
            state.auto_beam_off = true;
        }
        "\\autoBeamOn" => {
            state.auto_beam_off = false;
        }
        "\\unset" | "\\cadenzaOn" | "\\cadenzaOff"
        | "\\dynamicUp" | "\\dynamicDown" | "\\dynamicNeutral" | "\\melisma"
        | "\\melismaEnd" | "\\context" => {
            // Skip these commands; some may consume the next token
            // \context within music blocks is handled by named_context at the
            // walk_music_block level, but if tree-sitter doesn't wrap it as
            // named_context, skip it here.
        }
        _ => {
            // Unknown escaped word — may be a variable reference or dynamic
            let var_name = text.trim_start_matches('\\');
            if !state.resolve_variable(var_name) && is_dynamic_name(text) {
                attach_dynamic(state, text);
            }
        }
    }
    i
}

/// Apply a `\set Context.property = "value"` command to the current part.
fn apply_set_property(state: &mut WalkState, property: &str, value: &str) {
    // property is like "Staff.instrumentName" or "Staff.midiInstrument"
    let prop_name = property
        .split('.')
        .last()
        .unwrap_or(property);
    match prop_name {
        "instrumentName" => {
            let part = state.ensure_part();
            part.name = value.to_string();
        }
        "shortInstrumentName" => {
            let part = state.ensure_part();
            part.abbreviation = value.to_string();
        }
        "midiInstrument" => {
            let part = state.ensure_part();
            part.midi_instrument = value.to_string();
        }
        _ => {} // Ignore other properties
    }
}

/// Whether a `\xxx` string is a known dynamic marking.
fn is_dynamic_name(text: &str) -> bool {
    matches!(
        text,
        "\\ppppp"
            | "\\pppp"
            | "\\ppp"
            | "\\pp"
            | "\\p"
            | "\\mp"
            | "\\mf"
            | "\\f"
            | "\\ff"
            | "\\fff"
            | "\\ffff"
            | "\\fffff"
            | "\\fp"
            | "\\sf"
            | "\\sfz"
            | "\\sff"
            | "\\sp"
            | "\\spp"
            | "\\rfz"
            | "\\fz"
    )
}

/// Handle punctuation tokens: |, (, ), ~, ', ,
fn handle_punctuation(state: &mut WalkState, punc: &str) {
    match punc {
        "|" => {
            state.bar_check();
        }
        "(" => {
            // Slur start: attach to most recent note or chord
            let target = match state.current_voice.last_mut() {
                Some(VoiceElement::Note(note)) => Some(note.as_mut()),
                Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
                _ => None,
            };
            if let Some(note) = target {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
        }
        ")" => {
            // Slur stop: attach to most recent note or chord
            let target = match state.current_voice.last_mut() {
                Some(VoiceElement::Note(note)) => Some(note.as_mut()),
                Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
                _ => None,
            };
            if let Some(note) = target {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
        }
        "~" => {
            // Tie: attach to most recent note or chord
            let target = match state.current_voice.last_mut() {
                Some(VoiceElement::Note(note)) => Some(note.as_mut()),
                Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
                _ => None,
            };
            if let Some(note) = target {
                note.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Consumption helpers (read tokens ahead and advance index)
// ---------------------------------------------------------------------------

/// Consume octave marks (' and ,) after a pitch symbol. Returns net marks.
fn consume_octave_marks(state: &WalkState, children: &[Node], i: &mut usize) -> i32 {
    let mut marks = 0i32;
    while *i < children.len() {
        let node = children[*i];
        if node.kind() == "punctuation" {
            let child_text = punct_text(state, node);
            if child_text == "'" {
                marks += 1;
                *i += 1;
            } else if child_text == "," {
                marks -= 1;
                *i += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    marks
}

/// Consume accidental forcing marks (`!` = forced, `?` = cautionary) after a pitch.
/// These are LilyPond punctuation tokens that appear between octave marks and duration.
fn consume_accidental_marks(state: &WalkState, children: &[Node], i: &mut usize) {
    while *i < children.len() {
        let node = children[*i];
        if node.kind() == "punctuation" {
            let text = punct_text(state, node);
            if text == "!" || text == "?" {
                *i += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

/// Consume an optional duration (unsigned_integer + dot punctuation).
/// If no duration is found, returns the last used duration.
fn consume_duration(state: &mut WalkState, children: &[Node], i: &mut usize) -> Duration {
    // Look for unsigned_integer
    if *i < children.len() && children[*i].kind() == "unsigned_integer" {
        let num_text = state.text(children[*i]).to_string();
        *i += 1;
        if let Ok(num) = num_text.parse::<u32>() {
            // Count dots
            let mut dots = 0u8;
            while *i < children.len() {
                let node = children[*i];
                if node.kind() == "punctuation" {
                    let ptext = punct_text(state, node);
                    if ptext == "." {
                        dots += 1;
                        *i += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            // Convert LilyPond number to fraction: 4 → 1/4, 2 → 1/2, 1 → 1/1
            let base = if num > 0 {
                Ratio::new(1i64, num as i64)
            } else {
                Ratio::new(1, 4)
            };
            let dur = Duration::dotted(base, dots);
            state.last_duration = dur.clone();
            return dur;
        }
    }
    state.last_duration.clone()
}

/// Consume an optional `*N` or `*N/M` duration multiplier after a duration.
/// Returns the integer multiplier count (1 if no multiplier found).
/// For `R1*3` this returns 3; for `R1*3/4` this returns 1 (fraction multipliers
/// are not used for multi-measure rest expansion).
fn consume_duration_multiplier(state: &WalkState, children: &[Node], i: &mut usize) -> u32 {
    if *i < children.len() && children[*i].kind() == "punctuation" {
        let ptext = punct_text(state, children[*i]);
        if ptext == "*" {
            *i += 1;
            // Read the integer multiplier
            if *i < children.len() && children[*i].kind() == "unsigned_integer" {
                let num_text = state.text(children[*i]).to_string();
                *i += 1;
                // Check for fraction: *N/M (skip the /M part)
                if *i + 1 < children.len()
                    && children[*i].kind() == "punctuation"
                    && punct_text(state, children[*i]) == "/"
                {
                    // Fraction multiplier like *3/4 — skip it, not a multi-measure count
                    *i += 1; // skip "/"
                    if *i < children.len() && children[*i].kind() == "unsigned_integer" {
                        *i += 1; // skip denominator
                    }
                    return 1;
                }
                if let Ok(n) = num_text.parse::<u32>() {
                    return n;
                }
            }
        }
    }
    1
}

/// Consume post-note attachments: dynamics, ties, slurs, articulations, etc.
/// Returns a list of attachment tokens.
fn consume_attachments(state: &WalkState, children: &[Node], i: &mut usize) -> Vec<String> {
    let mut attachments = Vec::new();
    while *i < children.len() {
        let node = children[*i];
        match node.kind() {
            "dynamic" => {
                let dyn_text = state.text(node);
                attachments.push(dyn_text.to_string());
                *i += 1;
            }
            "escaped_word" => {
                let text = state.text(node);
                if is_post_note_command(text) {
                    attachments.push(text.to_string());
                    *i += 1;
                } else {
                    break;
                }
            }
            "punctuation" => {
                let ptext = punct_text(state, node);
                match ptext.as_str() {
                    "(" | ")" | "~" | "[" | "]" => {
                        attachments.push(ptext);
                        *i += 1;
                    }
                    "^" | "_" | "-" => {
                        // Direction indicator: check what follows
                        if let Some(next) = children.get(*i + 1) {
                            if next.kind() == "escaped_word" && state.text(*next) == "\\markup" {
                                // \markup { "text" } text direction
                                if let Some(block) = children.get(*i + 2) {
                                    if block.kind() == "expression_block" {
                                        let text = extract_markup_text(state, *block);
                                        if !text.is_empty() {
                                            let placement = match ptext.as_str() {
                                                "^" => "above",
                                                "_" => "below",
                                                _ => "unspecified",
                                            };
                                            attachments.push(format!("text:{placement}:{text}"));
                                        }
                                        *i += 3;
                                        continue;
                                    }
                                }
                            } else if next.kind() == "escaped_word" {
                                // Direction + escaped command, e.g. ^\fermata
                                let ew = state.text(*next);
                                if is_post_note_command(ew) {
                                    attachments.push(ew.to_string());
                                    *i += 2;
                                    continue;
                                }
                            } else if next.kind() == "punctuation" {
                                // Shorthand articulation: -. -> -_ -^ -! -+ --
                                let short = punct_text(state, *next);
                                if let Some(art) = shorthand_articulation(&short) {
                                    attachments.push(art.to_string());
                                    *i += 2;
                                    continue;
                                }
                            }
                        }
                        break;
                    }
                    _ => break,
                }
            }
            _ => break,
        }
    }
    attachments
}

/// Map a LilyPond shorthand articulation character to its long-form escaped command.
fn shorthand_articulation(ch: &str) -> Option<&'static str> {
    match ch {
        "." => Some("\\staccato"),
        ">" => Some("\\accent"),
        "_" => Some("\\tenuto"),
        "^" => Some("\\marcato"),
        "!" => Some("\\staccatissimo"),
        "+" => Some("\\stopped"),
        "-" => Some("\\tenuto"),
        _ => None,
    }
}

/// Whether an escaped_word is a post-note attachment rather than a new command.
fn is_post_note_command(text: &str) -> bool {
    matches!(
        text,
        "\\fermata"
            | "\\trill"
            | "\\mordent"
            | "\\prall"
            | "\\turn"
            | "\\reverseturn"
            | "\\shake"
            | "\\breathe"
            | "\\staccato"
            | "\\staccatissimo"
            | "\\tenuto"
            | "\\accent"
            | "\\marcato"
            | "\\portato"
            | "\\stopped"
            | "\\espressivo"
            | "\\glissando"
            | "\\arpeggio"
    ) || is_dynamic_name(text)
}

/// Get the text of a punctuation node (which may have a child).
fn punct_text(state: &WalkState, node: Node) -> String {
    if node.child_count() > 0 {
        let mut c = node.walk();
        let result = node.children(&mut c)
            .next()
            .map(|n| state.text(n).to_string())
            .unwrap_or_default();
        result
    } else {
        state.text(node).to_string()
    }
}

/// Consume a `\tempo` command (various forms).
fn consume_tempo(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    let mut text_label: Option<String> = None;
    let mut beat_unit: Option<String> = None;
    let mut per_minute: Option<u32> = None;

    // \tempo "Allegro" 4 = 120
    // \tempo 4 = 120
    // \tempo "Allegro"
    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "string" => {
                text_label = Some(extract_string_value(state, node));
                i += 1;
            }
            "unsigned_integer" => {
                let num_text = state.text(node);
                if beat_unit.is_none() {
                    beat_unit = Some(ly_number_to_beat_unit(num_text));
                    i += 1;
                } else {
                    // This is the BPM value
                    per_minute = num_text.parse().ok();
                    i += 1;
                    break;
                }
            }
            "punctuation" => {
                let pt = punct_text(state, node);
                if pt == "=" {
                    i += 1; // skip equals sign
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    if text_label.is_some() || beat_unit.is_some() || per_minute.is_some() {
        let tempo_dir = TempoDirection {
            text: text_label,
            beat_unit,
            dots: 0,
            per_minute: per_minute.map(|v| v as f64),
            placement: Placement::Unspecified,
        };
        let dir = Direction {
            tempo: Some(tempo_dir),
            ..Default::default()
        };
        let measure = state.ensure_measure();
        measure.directions.push(dir);
    }
    i
}

/// Convert LilyPond duration number to beat unit string.
fn ly_number_to_beat_unit(num: &str) -> String {
    match num {
        "1" => "whole",
        "2" => "half",
        "4" => "quarter",
        "8" => "eighth",
        "16" => "16th",
        "32" => "32nd",
        "64" => "64th",
        _ => "quarter",
    }
    .to_string()
}

/// Extract the text content of a `string` node. Returns the unquoted value.
fn extract_string_value(state: &WalkState, node: Node) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_fragment" {
            return state.text(child).to_string();
        }
    }
    String::new()
}

/// Extract text content from a markup expression_block like `{ "pizz." }`.
/// Walks children looking for string nodes and returns the concatenated text.
fn extract_markup_text(state: &WalkState, block: Node) -> String {
    let mut result = String::new();
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "string" {
            let s = extract_string_value(state, child);
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&s);
        }
    }
    result
}

/// Consume a `\mark` command with its argument.
/// Handles:
///   `\mark \markup { \musicglyph "scripts.coda" }`   → coda
///   `\mark \markup { \musicglyph "scripts.segno" }`  → segno
///   `\mark "D.C."` / `\mark "D.S. al Coda"` etc.     → da_capo / dal_segno
fn consume_mark(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    if i >= children.len() {
        return i;
    }
    let node = children[i];
    if node.kind() == "string" {
        // \mark "D.C." or \mark "D.S. al Coda"
        let text = extract_string_value(state, node);
        i += 1;
        let dir = if text.starts_with("D.S.") {
            Direction {
                dal_segno: Some(text),
                ..Default::default()
            }
        } else if text.starts_with("D.C.") {
            Direction {
                da_capo: Some(text),
                ..Default::default()
            }
        } else {
            // Generic text mark — ignore for now
            return i;
        };
        let measure = state.ensure_measure();
        measure.directions.push(dir);
    } else if node.kind() == "escaped_word" && state.text(node) == "\\markup" {
        // \mark \markup { ... }
        i += 1;
        if let Some(block) = children.get(i) {
            if block.kind() == "expression_block" {
                // Walk the markup block looking for \musicglyph "scripts.coda" etc.
                let block_text = state.text(*block);
                let dir = if block_text.contains("scripts.coda") {
                    Some(Direction {
                        coda: true,
                        ..Default::default()
                    })
                } else if block_text.contains("scripts.segno") {
                    Some(Direction {
                        segno: true,
                        ..Default::default()
                    })
                } else {
                    None
                };
                if let Some(d) = dir {
                    let measure = state.ensure_measure();
                    measure.directions.push(d);
                }
                i += 1;
            }
        }
    }
    i
}

/// Consume a `\override` command.
/// Recognises `Glissando.style = #'<style>` and sets pending state.
fn consume_override(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    // Collect tokens to build property path and value
    // Expected pattern: <symbol>.<symbol> = <scheme_value>
    // We read the raw text span from the first symbol through a few tokens.
    let start_i = i;
    let mut symbols: Vec<String> = Vec::new();
    let mut value = String::new();
    let mut saw_eq = false;

    // Consume up to ~10 tokens looking for the pattern
    let limit = (i + 10).min(children.len());
    while i < limit {
        let node = children[i];
        match node.kind() {
            "symbol" if !saw_eq => {
                symbols.push(state.text(node).to_string());
                i += 1;
            }
            "punctuation" => {
                let pt = punct_text(state, node);
                if pt == "." && !saw_eq {
                    i += 1; // skip dot in property path
                } else if pt == "=" {
                    saw_eq = true;
                    i += 1;
                } else {
                    break;
                }
            }
            _ if saw_eq => {
                // Whatever follows the "=" is the value — grab its raw text
                value = state.text(node).to_string();
                i += 1;
                break;
            }
            _ => break,
        }
    }

    // Check for known Glissando.style overrides
    if symbols.len() >= 2 && symbols[0] == "Glissando" && symbols[1] == "style" && saw_eq {
        // value is e.g. "#'dashed-line", "#'dotted-line", "#'trill"
        let style = value
            .trim_start_matches("#'")
            .trim_start_matches("#\u{2018}"); // curly quote edge case
        match style {
            "dashed-line" => {
                state.pending_glissando_style = Some("dashed".to_string());
            }
            "dotted-line" => {
                state.pending_glissando_style = Some("dotted".to_string());
            }
            "trill" => {
                state.pending_slide = true;
            }
            _ => {
                state.pending_glissando_style = Some(style.to_string());
            }
        }
    }

    // If we didn't consume anything useful, at least advance past start
    if i == start_i {
        // Skip unknown override — try to jump past expression_block or next statement
        while i < children.len() {
            let node = children[i];
            if node.kind() == "escaped_word" || node.kind() == "symbol" {
                break;
            }
            i += 1;
        }
    }
    i
}

/// Parse a `\paper { ... }` block and populate `state.page_layout`.
/// Check if an expression_block directly contains a `named_context` child
/// (i.e., `\new Staff`, `\new ChoirStaff`, etc., but NOT `\new Voice` or `\new Lyrics`).
fn block_contains_named_context(state: &WalkState, block: Node) -> bool {
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "named_context" {
            let (context, _) = extract_named_context(state, child);
            // Only part-creating or grouping contexts count
            if matches!(
                context.as_str(),
                "Staff" | "ChoirStaff" | "StaffGroup" | "GrandStaff" | "PianoStaff"
            ) {
                return true;
            }
        }
    }
    false
}

/// Parse a `\lyricmode { ... }` block into a list of `LyricSyllable`s.
/// Lyrics are symbols separated by `--` (hyphen) or `__` (extend).
fn parse_lyric_block(state: &WalkState, block: Node) -> Vec<LyricSyllable> {
    let mut syllables: Vec<LyricSyllable> = Vec::new();
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;
    let mut pending_hyphen = false;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let text = state.text(node).to_string();
                // Handle `_` as melisma extender (skip)
                if text == "_" {
                    // Check for `__` (double underscore = extender line)
                    if let Some(next) = children.get(i + 1) {
                        if next.kind() == "symbol" && state.text(*next) == "_" {
                            // `__` = extender line on previous syllable
                            if let Some(last) = syllables.last_mut() {
                                last.extend = true;
                            }
                            i += 2;
                            continue;
                        }
                    }
                    // Single `_` = melisma skip (note gets no syllable)
                    syllables.push(LyricSyllable {
                        text: String::new(),
                        syllabic: SyllabicType::Single,
                        number: 0, // marker: number=0 means "skip"
                        extend: false,
                        elision: false,
                    });
                    i += 1;
                    continue;
                }
                let next_is_hyphen = peek_lyric_hyphen(state, &children, i + 1);
                let syllabic = if pending_hyphen {
                    if next_is_hyphen { SyllabicType::Middle } else { SyllabicType::End }
                } else {
                    if next_is_hyphen { SyllabicType::Begin } else { SyllabicType::Single }
                };
                syllables.push(LyricSyllable {
                    text,
                    syllabic,
                    number: 1,
                    extend: false,
                    elision: false,
                });
                pending_hyphen = false;
            }
            "punctuation" => {
                let t = state.text(node);
                if t == "-" {
                    // Check for "--" (double hyphen = syllable separator)
                    if let Some(next) = children.get(i + 1) {
                        if next.kind() == "punctuation" && state.text(*next) == "-" {
                            pending_hyphen = true;
                            i += 2;
                            continue;
                        }
                    }
                    // Single "-" is also a syllable separator in LilyPond lyrics
                    pending_hyphen = true;
                }
                if t == "_" {
                    // `_` as punctuation = melisma skip
                    syllables.push(LyricSyllable {
                        text: String::new(),
                        syllabic: SyllabicType::Single,
                        number: 0,
                        extend: false,
                        elision: false,
                    });
                }
            }
            "escaped_word" => {
                let text = state.text(node);
                if text == "\\lyricsto" {
                    // Skip \lyricsto "voiceName" — we handle this at a higher level
                    i += 1;
                    if i < children.len() && children[i].kind() == "string" {
                        i += 1; // skip voice name string
                    }
                    continue;
                }
                // Check for variable reference
                let var_name = text.trim_start_matches('\\');
                if let Some(lyrics) = state.lyric_definitions.get(var_name) {
                    syllables.extend(lyrics.clone());
                }
            }
            "expression_block" => {
                // Nested block — recurse
                let inner = parse_lyric_block(state, node);
                syllables.extend(inner);
            }
            _ => {}
        }
        i += 1;
    }
    syllables
}

/// Check if position `start` begins a hyphen separator ("--" or single "-").
fn peek_lyric_hyphen(state: &WalkState, children: &[Node], start: usize) -> bool {
    if let Some(a) = children.get(start) {
        if a.kind() == "punctuation" && state.text(*a) == "-" {
            return true;
        }
    }
    false
}

/// Extract the voice name from a `\lyricsto "voiceName"` inside a lyric block.
fn extract_lyricsto_voice(state: &WalkState, block: Node) -> Option<String> {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    for i in 0..children.len() {
        let node = children[i];
        if node.kind() == "escaped_word" && state.text(node) == "\\lyricsto" {
            if let Some(next) = children.get(i + 1) {
                if next.kind() == "string" {
                    return Some(extract_string_value(state, *next));
                }
            }
        }
    }
    None
}

/// Attach a list of lyric syllables to the notes of a part, distributing
/// one syllable per note (skipping rests, tied notes, and melisma notes).
/// Syllables with `number == 0` are melisma skips — the note gets no lyric.
fn attach_lyrics_to_part(part: &mut Part, syllables: &[LyricSyllable]) {
    let mut syl_idx = 0;
    for measure in &mut part.measures {
        for voice in &mut measure.voices {
            for elem in &mut voice.elements {
                if syl_idx >= syllables.len() {
                    return;
                }
                match elem {
                    VoiceElement::Note(note) => {
                        // Skip grace notes and tied notes (continuation)
                        if note.is_grace {
                            continue;
                        }
                        let is_tied = note.ties.iter().any(|t| t.tie_type == StartStop::Stop);
                        if is_tied {
                            continue;
                        }
                        let syl = &syllables[syl_idx];
                        if syl.number == 0 {
                            // Melisma skip: advance syllable index, no lyric on this note
                            syl_idx += 1;
                        } else {
                            note.lyrics.push(syl.clone());
                            syl_idx += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn parse_paper_block(state: &mut WalkState, block: Node) {
    let mut layout = state.page_layout.take().unwrap_or(PageLayout {
        page_height: None,
        page_width: None,
        left_margin: None,
        right_margin: None,
        top_margin: None,
        bottom_margin: None,
        system_distance: None,
        top_system_distance: None,
        staff_size: None,
    });

    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        if node.kind() == "assignment_lhs" {
            // Extract the property name (may be compound like
            // "system-system-spacing.basic-distance")
            let key = state.text(node).trim().to_string();
            // Skip "=" punctuation and find the value
            let mut j = i + 1;
            while j < children.len() {
                let val_node = children[j];
                if val_node.kind() == "punctuation" && punct_text(state, val_node) == "=" {
                    j += 1;
                    continue;
                }
                // The value could be an unsigned_integer, a number with \cm suffix,
                // or a scheme expression like #15.0
                // Grab the raw text of the remaining tokens on this "line"
                let raw = state.text(val_node).to_string();
                // Try to extract a float — strip \cm, \mm, #, etc.
                let cleaned = raw
                    .replace("\\cm", "")
                    .replace("\\mm", "")
                    .replace("\\in", "")
                    .trim_start_matches('#')
                    .trim()
                    .to_string();
                if let Ok(v) = cleaned.parse::<f64>() {
                    match key.as_str() {
                        "paper-height" => layout.page_height = Some(v),
                        "paper-width" => layout.page_width = Some(v),
                        "left-margin" => layout.left_margin = Some(v),
                        "right-margin" => layout.right_margin = Some(v),
                        "top-margin" => layout.top_margin = Some(v),
                        "bottom-margin" => layout.bottom_margin = Some(v),
                        _ => {
                            if key.contains("system-system-spacing") {
                                layout.system_distance = Some(v);
                            } else if key.contains("top-system-spacing") {
                                layout.top_system_distance = Some(v);
                            }
                        }
                    }
                }
                i = j;
                break;
            }
        }
        i += 1;
    }

    state.page_layout = Some(layout);
}

/// Parse a fraction string like "4/4" into (numerator, denominator).
fn parse_fraction(text: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<u32>().ok()?;
        let den = parts[1].parse::<u32>().ok()?;
        Some((num, den))
    } else {
        None
    }
}

/// Parse `#(ly:make-moment N D)` from a Scheme expression text.
/// Returns (numerator, denominator) if successful.
fn parse_ly_make_moment(text: &str) -> Option<(u32, u32)> {
    // Text looks like: #(ly:make-moment 3 4) or #(ly:make-moment 3/4)
    let inner = text.trim_start_matches('#').trim();
    let inner = inner.strip_prefix('(')?.strip_suffix(')')?;
    let inner = inner.trim();
    if !inner.starts_with("ly:make-moment") {
        return None;
    }
    let args = inner.strip_prefix("ly:make-moment")?.trim();
    // Try "N D" format first
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<u32>().ok()?;
        let den = parts[1].parse::<u32>().ok()?;
        return Some((num, den));
    }
    // Try "N/D" format
    if let Some((num, den)) = parse_fraction(args) {
        return Some((num, den));
    }
    None
}

/// Build a Chord from a `chord` node (< ... >).
fn build_chord(state: &mut WalkState, chord_node: Node, dur: Duration) -> Chord {
    let mut notes = Vec::new();
    let mut cursor = chord_node.walk();
    let children: Vec<Node> = chord_node.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let child = children[i];
        if child.kind() == "symbol" {
            let sym = state.text(child);
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                i += 1;
                let octave_marks = consume_octave_marks(state, &children, &mut i);
                consume_accidental_marks(state, &children, &mut i);
                let pitch = state.resolve_pitch(step, alter, octave_marks);
                let note = Note::new(pitch, dur.clone());
                notes.push(note);
                continue;
            }
        }
        i += 1;
    }
    Chord::new(dur, notes)
}

/// Parse grace notes from a `{ ... }` block.
fn parse_grace_block(state: &mut WalkState, block: Node) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let child = children[i];
        if child.kind() == "symbol" {
            let sym = state.text(child);
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                i += 1;
                let octave_marks = consume_octave_marks(state, &children, &mut i);
                consume_accidental_marks(state, &children, &mut i);
                let dur = consume_duration(state, &children, &mut i);
                let pitch = state.resolve_pitch(step, alter, octave_marks);
                let note = Note::new(pitch, dur);
                notes.push(note);
                continue;
            }
        }
        i += 1;
    }
    notes
}

// ---------------------------------------------------------------------------
// Attachment application
// ---------------------------------------------------------------------------

fn apply_note_attachments(_state: &mut WalkState, note: &mut Note, attachments: &[String]) {
    for att in attachments {
        match att.as_str() {
            "[" => {
                // Start of manual beam group
                _state.in_beam_group = true;
                // Beam level depends on note duration: 8th=1, 16th=2, 32nd=3, 64th=4
                let level = beam_level_for_duration(&note.duration);
                if level > 0 {
                    note.beams.push(BeamEvent {
                        beam_type: "begin".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "]" => {
                // End of manual beam group
                _state.in_beam_group = false;
                let level = beam_level_for_duration(&note.duration);
                if level > 0 {
                    note.beams.push(BeamEvent {
                        beam_type: "end".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "~" => {
                note.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
            "(" => {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            ")" => {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            "\\fermata" => {
                note.fermata = Some(Fermata {
                    shape: "normal".to_string(),
                    inverted: false,
                });
            }
            "\\glissando" => {
                if _state.pending_slide {
                    note.slide = Some(StartStop::Start);
                    _state.pending_slide = false;
                } else {
                    note.glissando = Some(StartStop::Start);
                    if let Some(style) = _state.pending_glissando_style.take() {
                        note.glissando_line_type = Some(style);
                    }
                }
            }
            "\\arpeggio" => {
                // Arpeggio on a single note — unusual but valid in LilyPond
            }
            s if is_dynamic_name(s) => {
                let sign = s.trim_start_matches('\\').to_string();
                note.dynamics.push(DynamicMark {
                    sign,
                    placement: Placement::Unspecified,
                });
            }
            "\\<" | "\\crescendo" => {
                note.wedges.push(Wedge {
                    wedge_type: "crescendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\>" | "\\diminuendo" | "\\decrescendo" => {
                note.wedges.push(Wedge {
                    wedge_type: "diminuendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\!" => {
                note.wedges.push(Wedge {
                    wedge_type: "stop".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\trill" => {
                note.ornaments
                    .push(crate::ir::articulation::Ornament {
                        name: "trill-mark".to_string(),
                        placement: Placement::Unspecified,
                    });
            }
            "\\mordent" => {
                note.ornaments
                    .push(crate::ir::articulation::Ornament {
                        name: "mordent".to_string(),
                        placement: Placement::Unspecified,
                    });
            }
            "\\prall" => {
                note.ornaments
                    .push(crate::ir::articulation::Ornament {
                        name: "inverted-mordent".to_string(),
                        placement: Placement::Unspecified,
                    });
            }
            "\\turn" => {
                note.ornaments
                    .push(crate::ir::articulation::Ornament {
                        name: "turn".to_string(),
                        placement: Placement::Unspecified,
                    });
            }
            "\\reverseturn" => {
                note.ornaments
                    .push(crate::ir::articulation::Ornament {
                        name: "inverted-turn".to_string(),
                        placement: Placement::Unspecified,
                    });
            }
            "\\staccato" => {
                note.articulations.push(Articulation {
                    name: "staccato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\tenuto" => {
                note.articulations.push(Articulation {
                    name: "tenuto".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\accent" => {
                note.articulations.push(Articulation {
                    name: "accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\marcato" => {
                note.articulations.push(Articulation {
                    name: "strong-accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccatissimo" => {
                note.articulations.push(Articulation {
                    name: "staccatissimo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\portato" => {
                note.articulations.push(Articulation {
                    name: "detached-legato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\stopped" => {
                note.technicals.push(Technical {
                    name: "stopped".to_string(),
                    value: String::new(),
                });
            }
            "\\breathe" => {
                note.articulations.push(Articulation {
                    name: "breath-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            s if s.starts_with("text:") => {
                // "text:above:pizz." or "text:below:arco"
                let rest = &s[5..];
                if let Some((placement_str, text)) = rest.split_once(':') {
                    let placement = match placement_str {
                        "above" => Placement::Above,
                        "below" => Placement::Below,
                        _ => Placement::Unspecified,
                    };
                    note.text_directions.push(TextDirection {
                        text: text.to_string(),
                        placement,
                        font_style: None,
                        font_weight: None,
                    });
                }
            }
            _ => {}
        }
    }
}

fn apply_rest_attachments(rest: &mut Rest, attachments: &[String]) {
    for att in attachments {
        if att == "\\fermata" {
            rest.fermata = Some(Fermata {
                shape: "normal".to_string(),
                inverted: false,
            });
        }
    }
}

fn apply_chord_attachments(
    _state: &mut WalkState,
    chord: &mut Chord,
    attachments: &[String],
) {
    for att in attachments {
        match att.as_str() {
            "\\arpeggio" => {
                // Apply pending arpeggio type, defaulting to Up
                chord.arpeggio = Some(
                    _state.pending_arpeggio_type.take().unwrap_or(ArpeggioType::Up),
                );
                continue;
            }
            "\\glissando" => {
                // Glissando on a chord — apply to first note
                if !chord.notes.is_empty() {
                    if _state.pending_slide {
                        chord.notes[0].slide = Some(StartStop::Start);
                        _state.pending_slide = false;
                    } else {
                        chord.notes[0].glissando = Some(StartStop::Start);
                        if let Some(style) = _state.pending_glissando_style.take() {
                            chord.notes[0].glissando_line_type = Some(style);
                        }
                    }
                }
                continue;
            }
            _ => {}
        }
    }
    if chord.notes.is_empty() {
        return;
    }
    // Apply remaining attachments to first note only (as is convention)
    let first = &mut chord.notes[0];
    for att in attachments {
        match att.as_str() {
            "[" => {
                _state.in_beam_group = true;
                let level = beam_level_for_duration(&chord.duration);
                if level > 0 {
                    first.beams.push(BeamEvent {
                        beam_type: "begin".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "]" => {
                _state.in_beam_group = false;
                let level = beam_level_for_duration(&chord.duration);
                if level > 0 {
                    first.beams.push(BeamEvent {
                        beam_type: "end".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "~" => {
                first.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
            "(" => {
                first.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            ")" => {
                first.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            "\\fermata" => {
                first.fermata = Some(Fermata {
                    shape: "normal".to_string(),
                    inverted: false,
                });
            }
            s if is_dynamic_name(s) => {
                let sign = s.trim_start_matches('\\').to_string();
                first.dynamics.push(DynamicMark {
                    sign,
                    placement: Placement::Unspecified,
                });
            }
            "\\<" | "\\crescendo" => {
                first.wedges.push(Wedge {
                    wedge_type: "crescendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\>" | "\\diminuendo" | "\\decrescendo" => {
                first.wedges.push(Wedge {
                    wedge_type: "diminuendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\!" => {
                first.wedges.push(Wedge {
                    wedge_type: "stop".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\trill" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "trill-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\mordent" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "mordent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\prall" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "inverted-mordent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\turn" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "turn".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\reverseturn" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "inverted-turn".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccato" => {
                first.articulations.push(Articulation {
                    name: "staccato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\tenuto" => {
                first.articulations.push(Articulation {
                    name: "tenuto".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\accent" => {
                first.articulations.push(Articulation {
                    name: "accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\marcato" => {
                first.articulations.push(Articulation {
                    name: "strong-accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccatissimo" => {
                first.articulations.push(Articulation {
                    name: "staccatissimo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\portato" => {
                first.articulations.push(Articulation {
                    name: "detached-legato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\stopped" => {
                first.technicals.push(Technical {
                    name: "stopped".to_string(),
                    value: String::new(),
                });
            }
            "\\breathe" => {
                first.articulations.push(Articulation {
                    name: "breath-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            s if s.starts_with("text:") => {
                // "text:above:pizz." or "text:below:arco"
                let rest = &s[5..];
                if let Some((placement_str, text)) = rest.split_once(':') {
                    let placement = match placement_str {
                        "above" => Placement::Above,
                        "below" => Placement::Below,
                        _ => Placement::Unspecified,
                    };
                    first.text_directions.push(TextDirection {
                        text: text.to_string(),
                        placement,
                        font_style: None,
                        font_weight: None,
                    });
                }
            }
            _ => {}
        }
    }
}

/// Attach a dynamic mark to the most recent note or chord in the current voice.
fn attach_dynamic(state: &mut WalkState, dyn_text: &str) {
    let sign = dyn_text.trim_start_matches('\\').to_string();
    let target = match state.current_voice.last_mut() {
        Some(VoiceElement::Note(note)) => Some(note.as_mut()),
        Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
        _ => None,
    };
    if let Some(note) = target {
        if sign == "<" {
            note.wedges.push(Wedge {
                wedge_type: "crescendo".to_string(),
                placement: Placement::Unspecified,
            });
        } else if sign == ">" {
            note.wedges.push(Wedge {
                wedge_type: "diminuendo".to_string(),
                placement: Placement::Unspecified,
            });
        } else if sign == "!" {
            note.wedges.push(Wedge {
                wedge_type: "stop".to_string(),
                placement: Placement::Unspecified,
            });
        } else {
            note.dynamics.push(DynamicMark {
                sign,
                placement: Placement::Unspecified,
            });
        }
    }
}

/// Attach a fermata to the most recent note or rest.
fn attach_fermata(state: &mut WalkState) {
    match state.current_voice.last_mut() {
        Some(VoiceElement::Note(note)) => {
            note.fermata = Some(Fermata {
                shape: "normal".to_string(),
                inverted: false,
            });
        }
        Some(VoiceElement::Rest(rest)) => {
            rest.fermata = Some(Fermata {
                shape: "normal".to_string(),
                inverted: false,
            });
        }
        _ => {}
    }
}

/// Attach an articulation by name to the most recent note.
fn attach_articulation(state: &mut WalkState, name: &str) {
    if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
        note.articulations.push(Articulation {
            name: name.to_string(),
            placement: Placement::Unspecified,
        });
    }
}

// ===========================================================================
// Post-processing: automatic beaming and stem direction
// ===========================================================================

/// Apply automatic beaming and stem directions to all parts in a score.
/// Only applies to notes that don't already have explicit beams/stems.
fn post_process_beams_and_stems(score: &mut Score) {
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            let mut current_ts: Option<TimeSignature> = None;
            let mut current_clef = Clef::default(); // treble by default
            for measure in &mut part.measures {
                // Track time signature and clef changes
                if let Some(ref attrs) = measure.attributes {
                    if let Some(ref ts) = attrs.time {
                        current_ts = Some(ts.clone());
                    }
                    if let Some(clef) = attrs.clefs.get(&1) {
                        current_clef = clef.clone();
                    }
                }
                let ts = current_ts
                    .clone()
                    .unwrap_or(TimeSignature {
                        beats: "4".to_string(),
                        beat_type: 4,
                        symbol: None,
                    });
                for voice in &mut measure.voices {
                    auto_beam_voice(&mut voice.elements, &ts);
                    auto_stem_voice(&mut voice.elements, &current_clef);
                }
            }
        }
    }
}

/// Apply automatic beaming to a voice's elements.
///
/// Beaming rules:
/// - Group consecutive beamable notes (8th or shorter) within a beam span.
/// - In 4/4: 8th notes group per half note (4 per group); 16ths group per
///   quarter note (4 per group). Mixed groups use the shorter span.
/// - In other simple meters: group per beat (1/beat_type).
/// - In compound meters (6/8, 9/8, 12/8): group per dotted beat (3/beat_type).
/// - Tuplet notes only beam within their own tuplet group.
/// - Don't beam single notes (group must have ≥2 beamable notes).
/// - Skip notes that already have explicit beams.
fn auto_beam_voice(elements: &mut [VoiceElement], ts: &TimeSignature) {
    let (eighth_span, sub_span) = compute_beam_spans(ts);
    if sub_span <= Frac::from_integer(0) {
        return;
    }

    // Collect beamable note indices with position and tuplet group info
    struct NoteInfo {
        idx: usize,
        position: Frac,
        beam_level: u8,
        has_explicit_beam: bool,
        tuplet_group: u32, // 0 = not in tuplet; same non-zero value = same tuplet
    }

    let mut infos: Vec<NoteInfo> = Vec::new();
    let mut pos = Frac::from_integer(0);
    let mut tuplet_counter: u32 = 0;
    let mut current_tuplet: u32 = 0;

    for (idx, elem) in elements.iter().enumerate() {
        match elem {
            VoiceElement::Note(n) => {
                // Track tuplet groups
                if let Some(ref td) = n.tuplet {
                    if td.tuplet_type == StartStop::Start {
                        tuplet_counter += 1;
                        current_tuplet = tuplet_counter;
                    }
                }
                let in_tuplet = if n.duration.tuplet_actual != 1 {
                    current_tuplet
                } else {
                    0
                };

                let level = beam_level_for_duration(&n.duration);
                if level > 0 && !n.is_grace {
                    infos.push(NoteInfo {
                        idx,
                        position: pos,
                        beam_level: level,
                        has_explicit_beam: !n.beams.is_empty() || n.no_auto_beam,
                        tuplet_group: in_tuplet,
                    });
                }
                if !n.is_grace {
                    pos = pos + n.duration.actual_duration();
                }

                if let Some(ref td) = n.tuplet {
                    if td.tuplet_type == StartStop::Stop {
                        current_tuplet = 0;
                    }
                }
            }
            VoiceElement::Rest(r) => {
                // Track tuplet group for rests too
                if let Some(ref td) = r.tuplet {
                    if td.tuplet_type == StartStop::Start {
                        tuplet_counter += 1;
                        current_tuplet = tuplet_counter;
                    }
                }
                pos = pos + r.duration.actual_duration();
                if let Some(ref td) = r.tuplet {
                    if td.tuplet_type == StartStop::Stop {
                        current_tuplet = 0;
                    }
                }
            }
            VoiceElement::Chord(c) => {
                let first_tuplet = c.notes.first().and_then(|n| n.tuplet.as_ref());
                if let Some(td) = first_tuplet {
                    if td.tuplet_type == StartStop::Start {
                        tuplet_counter += 1;
                        current_tuplet = tuplet_counter;
                    }
                }
                let in_tuplet = if c.duration.tuplet_actual != 1 {
                    current_tuplet
                } else {
                    0
                };

                let level = beam_level_for_duration(&c.duration);
                let has_beam = c.notes.first().map_or(false, |n| !n.beams.is_empty() || n.no_auto_beam);
                if level > 0 {
                    infos.push(NoteInfo {
                        idx,
                        position: pos,
                        beam_level: level,
                        has_explicit_beam: has_beam,
                        tuplet_group: in_tuplet,
                    });
                }
                pos = pos + c.duration.actual_duration();

                let last_tuplet = c.notes.first().and_then(|n| n.tuplet.as_ref());
                if let Some(td) = last_tuplet {
                    if td.tuplet_type == StartStop::Stop {
                        current_tuplet = 0;
                    }
                }
            }
            VoiceElement::Forward(f) => {
                pos = pos + f.duration.actual_duration();
            }
            VoiceElement::Backup(b) => {
                pos = pos - b.duration.actual_duration();
            }
        }
    }

    let span_of = |pos: Frac, span: Frac| -> i64 {
        if span <= Frac::from_integer(0) { return 0; }
        (pos / span).to_integer()
    };

    // Group consecutive beamable notes that share the same tuplet group
    // and fall within the same beam span.
    // For tuplet notes: the beam span is the whole tuplet (don't use time-based spans).
    // For non-tuplet notes: use beat-based spans depending on note durations.
    let mut i = 0;
    while i < infos.len() {
        if infos[i].has_explicit_beam {
            i += 1;
            continue;
        }

        let group_start = i;
        let tuplet_g = infos[i].tuplet_group;

        if tuplet_g != 0 {
            // Tuplet group: extend to end of same tuplet
            i += 1;
            while i < infos.len()
                && infos[i].tuplet_group == tuplet_g
                && !infos[i].has_explicit_beam
            {
                i += 1;
            }
        } else {
            // Non-tuplet: determine the effective span for this group.
            // If all notes are 8ths only (level 1), use eighth_span (half note in 4/4).
            // If any note is 16th or shorter, use sub_span (quarter note in 4/4).
            let beat = span_of(infos[i].position, sub_span);
            i += 1;
            while i < infos.len()
                && infos[i].tuplet_group == 0
                && !infos[i].has_explicit_beam
                && span_of(infos[i].position, sub_span) == beat
            {
                i += 1;
            }
        }

        let group_end = i;
        let group_slice = &infos[group_start..group_end];
        let group_len = group_slice.len();

        if group_len < 2 {
            continue;
        }

        // For non-tuplet 8th-only groups, try to merge with the next beat group
        // to form half-note groups (in 4/4). This is done by checking if the next
        // group is also 8th-only and on the same eighth_span.
        let all_eighths = group_slice.iter().all(|n| n.beam_level == 1);
        let mut merged_end = group_end;

        if tuplet_g == 0 && all_eighths && eighth_span > sub_span {
            // Try to extend into adjacent beat groups within the same eighth_span
            let eighth_beat = span_of(infos[group_start].position, eighth_span);
            while merged_end < infos.len()
                && infos[merged_end].tuplet_group == 0
                && !infos[merged_end].has_explicit_beam
                && infos[merged_end].beam_level == 1
                && span_of(infos[merged_end].position, eighth_span) == eighth_beat
            {
                merged_end += 1;
            }
            if merged_end > group_end {
                // We merged; update i to skip past merged notes
                i = merged_end;
            }
        }

        let final_slice = &infos[group_start..merged_end];
        let final_len = final_slice.len();

        if final_len < 2 {
            continue;
        }

        // Build beam assignments
        let max_level = final_slice.iter().map(|n| n.beam_level).max().unwrap_or(1);

        let mut assignments: Vec<(usize, Vec<BeamEvent>)> = Vec::new();
        for info in final_slice {
            assignments.push((info.idx, Vec::new()));
        }

        // Level 1: beam across the entire group
        for (gi, _) in final_slice.iter().enumerate() {
            let bt = if gi == 0 {
                "begin"
            } else if gi == final_len - 1 {
                "end"
            } else {
                "continue"
            };
            assignments[gi].1.push(BeamEvent {
                beam_type: bt.to_string(),
                number: 1,
            });
        }

        // Level 2+: break at sub_span boundaries
        for level in 2..=max_level {
            let mut si = 0;
            while si < final_len {
                let info = &final_slice[si];
                if info.beam_level < level {
                    si += 1;
                    continue;
                }
                let sub_beat = span_of(info.position, sub_span);
                let sub_start = si;
                si += 1;
                while si < final_len {
                    let ni = &final_slice[si];
                    if ni.beam_level < level
                        || span_of(ni.position, sub_span) != sub_beat
                    {
                        break;
                    }
                    si += 1;
                }
                let sub_len = si - sub_start;
                if sub_len < 2 {
                    // Single note at this level: use a hook
                    let is_at_end = sub_start + 1 >= final_len;
                    let hook = if is_at_end { "backward hook" } else { "forward hook" };
                    assignments[sub_start].1.push(BeamEvent {
                        beam_type: hook.to_string(),
                        number: level,
                    });
                    continue;
                }
                for sgi in sub_start..si {
                    let bt = if sgi == sub_start {
                        "begin"
                    } else if sgi == si - 1 {
                        "end"
                    } else {
                        "continue"
                    };
                    assignments[sgi].1.push(BeamEvent {
                        beam_type: bt.to_string(),
                        number: level,
                    });
                }
            }
        }

        // Apply to elements
        for (elem_idx, beams) in assignments {
            if beams.is_empty() { continue; }
            match &mut elements[elem_idx] {
                VoiceElement::Note(n) => n.beams = beams,
                VoiceElement::Chord(c) => {
                    if let Some(first) = c.notes.first_mut() {
                        first.beams = beams;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Compute beam grouping spans for auto-beaming.
/// Returns (primary_span, sub_span):
/// - primary_span: grouping unit for level-1 beams (8ths)
/// - sub_span: grouping unit for level-2+ beams (16ths, 32nds)
///
/// In simple quadruple time (4/4): primary = half note, sub = quarter note.
/// In compound meters (6/8 etc): both = dotted beat.
/// In other simple meters: both = one beat.
fn compute_beam_spans(ts: &TimeSignature) -> (Frac, Frac) {
    let beats: i64 = ts
        .beats
        .split('+')
        .filter_map(|b| b.trim().parse::<i64>().ok())
        .sum();
    let bt = ts.beat_type as i64;

    if bt == 0 || beats == 0 {
        let q = Frac::new(1, 4);
        return (q, q);
    }

    // Compound meters: dotted beat for all levels
    if beats % 3 == 0 && beats > 3 && (bt == 8 || bt == 16) {
        let dotted = Frac::new(3, bt);
        return (dotted, dotted);
    }

    let beat = Frac::new(1, bt);

    // Simple quadruple: 8ths group per half note, sub-beams per beat
    if beats == 4 && bt == 4 {
        let half = Frac::new(1, 2);
        return (half, beat);
    }

    // Other simple meters: both levels group per beat
    (beat, beat)
}

/// Apply automatic stem directions to notes that don't have explicit stems.
///
/// Rule: for notes on or above the middle line (B4 in treble clef),
/// stem goes down; below the middle line, stem goes up.
/// The middle line depends on the clef, but B4 is the default for treble.
fn auto_stem_voice(elements: &mut [VoiceElement], clef: &Clef) {
    let mid = middle_line_midi(clef);
    for elem in elements {
        match elem {
            VoiceElement::Note(n) => {
                if n.stem_direction.is_empty() && !n.is_grace {
                    n.stem_direction = auto_stem_for_midi(n.pitch.midi_number(), mid);
                }
            }
            VoiceElement::Chord(c) => {
                if c.notes.is_empty() {
                    continue;
                }
                let has_explicit = c.notes.iter().any(|n| !n.stem_direction.is_empty());
                if !has_explicit {
                    let avg_midi: f64 = c.notes.iter().map(|n| n.pitch.midi_number() as f64).sum::<f64>()
                        / c.notes.len() as f64;
                    let dir = if avg_midi >= mid as f64 { "down" } else { "up" };
                    for n in &mut c.notes {
                        n.stem_direction = dir.to_string();
                    }
                }
            }
            _ => {}
        }
    }
}

/// MIDI number of the middle staff line for a given clef.
/// Notes at or above this pitch get stem down; below get stem up.
fn middle_line_midi(clef: &Clef) -> i32 {
    let base = match clef.sign {
        ClefSign::G => {
            // G clef: G4 (MIDI 67) on clef.line (default 2). Middle line (3) offset.
            67 + (3 - clef.line as i32) * 2
        }
        ClefSign::F => {
            // F clef: F3 (MIDI 53) on clef.line (default 4). Middle line (3) offset.
            53 + (3 - clef.line as i32) * 2
        }
        ClefSign::C => {
            // C clef: C4 (MIDI 60) on clef.line. Middle line (3) offset.
            60 + (3 - clef.line as i32) * 2
        }
        _ => 71, // default to treble
    };
    base + clef.octave_change as i32 * 12
}

/// Determine automatic stem direction based on MIDI number and middle line.
fn auto_stem_for_midi(midi: i32, middle: i32) -> String {
    if midi >= middle {
        "down".to_string()
    } else {
        "up".to_string()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ToIrAdapter;

    #[test]
    fn test_parse_simple_melody() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' }"#)
            .unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty());
        let part = &parts[0];
        assert!(!part.measures.is_empty());

        // Should have 4 notes
        let notes: Vec<&Note> = part.measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[0].pitch.octave, 4);
        assert_eq!(notes[1].pitch.step, PitchStep::D);
        assert_eq!(notes[2].pitch.step, PitchStep::E);
        assert_eq!(notes[3].pitch.step, PitchStep::F);
    }

    #[test]
    fn test_parse_with_key_time_clef() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
  \new Staff {
    \key g \major
    \time 3/4
    \clef treble
    c'4 d' e' |
  }
}"#,
            )
            .unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty());
        let part = &parts[0];
        let measure = &part.measures[0];

        // Check attributes
        let attrs = measure.attributes.as_ref().unwrap();
        assert_eq!(attrs.key.as_ref().unwrap().fifths, 1); // G major = 1 sharp
        assert_eq!(attrs.time.as_ref().unwrap().beats, "3");
        assert_eq!(attrs.time.as_ref().unwrap().beat_type, 4);
        assert!(attrs.clefs.contains_key(&1));
        assert_eq!(attrs.clefs[&1].sign, ClefSign::G);
    }

    #[test]
    fn test_parse_relative_pitch() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
  \new Staff \relative c' {
    c4 d e f |
    g a b c |
  }
}"#,
            )
            .unwrap();

        let parts = score.parts();
        let part = &parts[0];
        let notes: Vec<&Note> = part.measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        // In relative mode starting from c': c d e f g a b c
        // c=4, d=4, e=4, f=4, g=4, a=4, b=4, c=5
        assert!(notes.len() >= 8, "Expected 8 notes, got {}", notes.len());
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[0].pitch.octave, 4);
        assert_eq!(notes[4].pitch.step, PitchStep::G);
        assert_eq!(notes[4].pitch.octave, 4);
        assert_eq!(notes[7].pitch.step, PitchStep::C);
        assert_eq!(notes[7].pitch.octave, 5);
    }

    #[test]
    fn test_parse_rests_and_spacers() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ r4 R1 s2 }"#).unwrap();

        let parts = score.parts();
        let part = &parts[0];
        let elems: Vec<&VoiceElement> = part.measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 3);
        match &elems[0] {
            VoiceElement::Rest(r) => {
                assert!(!r.is_measure_rest);
                assert!(!r.is_spacer);
            }
            _ => panic!("expected Rest"),
        }
        match &elems[1] {
            VoiceElement::Rest(r) => assert!(r.is_measure_rest),
            _ => panic!("expected measure Rest"),
        }
        match &elems[2] {
            VoiceElement::Rest(r) => assert!(r.is_spacer),
            _ => panic!("expected spacer Rest"),
        }
    }

    #[test]
    fn test_parse_chord() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ <c' e' g'>4 }"#).unwrap();

        let parts = score.parts();
        let part = &parts[0];
        let elems: Vec<&VoiceElement> = part.measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 1);
        match &elems[0] {
            VoiceElement::Chord(c) => {
                assert_eq!(c.notes.len(), 3);
                assert_eq!(c.notes[0].pitch.step, PitchStep::C);
                assert_eq!(c.notes[1].pitch.step, PitchStep::E);
                assert_eq!(c.notes[2].pitch.step, PitchStep::G);
            }
            _ => panic!("expected Chord"),
        }
    }

    #[test]
    fn test_parse_header() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\header {
  title = "My Piece"
  composer = "J. S. Bach"
}
{ c'4 }"#,
            )
            .unwrap();

        assert_eq!(score.metadata.title.as_deref(), Some("My Piece"));
        assert_eq!(score.metadata.composer.as_deref(), Some("J. S. Bach"));
    }

    #[test]
    fn test_parse_dynamics_and_ties() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4\f~ c' d'\< e'\! }"#)
            .unwrap();

        let parts = score.parts();
        let notes: Vec<&Note> = parts[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 4);
        // c' has \f dynamic
        assert!(!notes[0].dynamics.is_empty());
        assert_eq!(notes[0].dynamics[0].sign, "f");
        // c' has tie
        assert!(!notes[0].ties.is_empty());
        // e' has \! (wedge stop)
        assert!(!notes[3].wedges.is_empty());
        assert_eq!(notes[3].wedges[0].wedge_type, "stop");
    }

    #[test]
    fn test_parse_language_english() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\language "english"
{ cs'4 ef' fs' bf' }"#,
            )
            .unwrap();

        let notes: Vec<&Note> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 4);
        // cs = C#
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[0].pitch.alter, Ratio::new(1, 1));
        // ef = Eb
        assert_eq!(notes[1].pitch.step, PitchStep::E);
        assert_eq!(notes[1].pitch.alter, Ratio::new(-1, 1));
    }

    #[test]
    fn test_parse_grace_note() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \grace { e'16 } c'4 }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert!(elems.len() >= 2);
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace);
                assert_eq!(n.pitch.step, PitchStep::E);
            }
            _ => panic!("expected grace Note"),
        }
        match &elems[1] {
            VoiceElement::Note(n) => {
                assert!(!n.is_grace);
                assert_eq!(n.pitch.step, PitchStep::C);
            }
            _ => panic!("expected non-grace Note"),
        }
    }

    #[test]
    fn test_parse_fixture_file() {
        let path = std::path::Path::new("tests/fixtures/ly/rest-dynamic.ly");
        if !path.exists() {
            return;
        }
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_file(path).unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty());
        let part = &parts[0];
        assert!(!part.measures.is_empty());
    }

    #[test]
    fn test_parse_bar_checks_create_measures() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' | g' a' b' c'' | }"#)
            .unwrap();

        let part = &score.parts()[0];
        // Should have at least 2 measures from bar checks
        assert!(
            part.measures.len() >= 2,
            "Expected >= 2 measures, got {}",
            part.measures.len()
        );
    }

    #[test]
    fn test_parse_variable_definition_and_reference() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
melody = { c'4 d' e' f' }

\score {
  \new Staff \melody
}
"#,
            )
            .unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty(), "Should have at least one part");
        let part = &parts[0];

        let notes: Vec<&Note> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 4, "Variable \\melody should resolve to 4 notes");
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[3].pitch.step, PitchStep::F);
    }

    #[test]
    fn test_parse_multiple_variable_references() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
partA = { c'4 d' e' f' }
partB = { g'4 a' b' c'' }

\score {
  <<
    \new Staff \partA
    \new Staff \partB
  >>
}
"#,
            )
            .unwrap();

        let parts = score.parts();
        assert_eq!(parts.len(), 2, "Should have two parts from two \\new Staff");

        let notes_a: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes_a.len(), 4);
        assert_eq!(notes_a[0].pitch.step, PitchStep::C);

        let notes_b: Vec<&Note> = parts[1]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes_b.len(), 4);
        assert_eq!(notes_b[0].pitch.step, PitchStep::G);
    }

    #[test]
    fn test_pitch_mode_preserved_in_metadata() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"\relative c' { c4 d e f }"#)
            .unwrap();

        assert_eq!(
            score.metadata.pitch_mode,
            PitchMode::Relative,
            "PitchMode should be Relative when \\relative is used"
        );
    }

    #[test]
    fn test_pitch_language_preserved_in_metadata() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"\language "english" { cs'4 df' ef' fs' }"#)
            .unwrap();

        assert_eq!(
            score.metadata.pitch_language,
            Some(PitchLanguage::English),
            "PitchLanguage should be English"
        );
    }

    #[test]
    fn test_parse_alphabetic_var_names() {
        let input = r#"pA = { c'4 d' e' f' }
pB = { g4 a b c' }
\score {
  <<
    \new Staff \pA
    \new Staff \pB
  >>
  \layout {}
}
"#;
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(input).unwrap();
        assert_eq!(
            score.parts().len(),
            2,
            "Expected 2 parts from 2 variable references in score block"
        );
    }

    #[test]
    fn test_parse_acciaccatura() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \acciaccatura { e'16 } c'4 }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert!(elems.len() >= 2);
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace);
                assert!(n.grace_slash, "acciaccatura should set grace_slash=true");
            }
            _ => panic!("expected grace Note"),
        }
    }

    #[test]
    fn test_parse_grace_not_slash() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \grace { d'16 } c'4 }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace);
                assert!(!n.grace_slash, "\\grace should set grace_slash=false");
            }
            _ => panic!("expected grace Note"),
        }
    }

    #[test]
    fn test_parse_tuplet() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \tuplet 3/2 { c'4 d' e' } }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 3, "tuplet should produce 3 elements");
        for elem in &elems {
            match elem {
                VoiceElement::Note(n) => {
                    assert_eq!(n.duration.tuplet_actual, 3);
                    assert_eq!(n.duration.tuplet_normal, 2);
                }
                _ => panic!("expected Note in tuplet"),
            }
        }
        // First element should have TupletDisplay::Start
        match &elems[0] {
            VoiceElement::Note(n) => {
                let td = n.tuplet.as_ref().expect("first note should have tuplet display");
                assert_eq!(td.tuplet_type, StartStop::Start);
            }
            _ => {}
        }
        // Last element should have TupletDisplay::Stop
        match &elems[2] {
            VoiceElement::Note(n) => {
                let td = n.tuplet.as_ref().expect("last note should have tuplet display");
                assert_eq!(td.tuplet_type, StartStop::Stop);
            }
            _ => {}
        }
    }

    #[test]
    fn test_parse_times_old_syntax() {
        let adapter = LyToIrAdapter::new();
        // \times has reversed fraction: normal/actual
        let score = adapter
            .convert_str(r#"{ \times 2/3 { c'4 d' e' } }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 3);
        match &elems[0] {
            VoiceElement::Note(n) => {
                // \times 2/3 means normal=2, actual=3 → reversed to actual=3, normal=2
                assert_eq!(n.duration.tuplet_actual, 3);
                assert_eq!(n.duration.tuplet_normal, 2);
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn test_sextuplet_6_4() {
        // \tuplet 6/4 { r16 a'16 b'16 cis''16 d''16 e''16 } = 6 16ths in time of 4 16ths = 1 quarter
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 c'4 r4 r4 \tuplet 6/4 { r16 a'16 b'16 cis''16 d''16 e''16 } }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        // Should be exactly 1 measure (3 quarters + sextuplet = 1 quarter = 4/4)
        assert_eq!(part.measures.len(), 1, "sextuplet should fit in one measure, got {} measures", part.measures.len());

        let elems: Vec<&VoiceElement> = part.measures[0].voices[0].elements.iter().collect();
        // c'4 r4 r4 + 6 tuplet notes = 9 elements
        assert_eq!(elems.len(), 9, "expected 9 elements, got {}", elems.len());

        // Check that the tuplet rest and notes have tuplet_actual=6, tuplet_normal=4
        for elem in &elems[3..9] {
            match elem {
                VoiceElement::Note(n) => {
                    assert_eq!(n.duration.tuplet_actual, 6, "note should have tuplet_actual=6");
                    assert_eq!(n.duration.tuplet_normal, 4, "note should have tuplet_normal=4");
                }
                VoiceElement::Rest(r) => {
                    assert_eq!(r.duration.tuplet_actual, 6, "rest should have tuplet_actual=6");
                    assert_eq!(r.duration.tuplet_normal, 4, "rest should have tuplet_normal=4");
                }
                _ => panic!("unexpected element in tuplet"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Section 10: Extended feature parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_partial() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \partial 4 c'4 | d'2 e'2 }"#)
            .unwrap();

        assert!(score.metadata.partial_duration.is_some());
        let partial = score.metadata.partial_duration.as_ref().unwrap();
        assert_eq!(partial.actual_duration(), Ratio::new(1i64, 4));
    }

    #[test]
    fn test_parse_glissando() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4\glissando d'4 }"#)
            .unwrap();

        let notes: Vec<&Note> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].glissando, Some(StartStop::Start));
        assert!(notes[0].slide.is_none());
    }

    #[test]
    fn test_parse_arpeggio_up() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \arpeggioArrowUp <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::Up));
    }

    #[test]
    fn test_parse_arpeggio_down() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \arpeggioArrowDown <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::Down));
    }

    #[test]
    fn test_parse_arpeggio_bracket() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \arpeggioBracket <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::NonArpeggio));
    }

    #[test]
    fn test_parse_after_grace() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 \afterGrace { d'16 } }"#)
            .unwrap();

        let notes: Vec<&Note> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 2);
        // First note is normal
        assert!(!notes[0].is_grace);
        // Second note is after-grace
        assert!(notes[1].is_grace);
        assert!(notes[1].after_grace);
        assert_eq!(notes[1].pitch.step, PitchStep::D);
    }

    #[test]
    fn test_parse_mark_da_capo() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' \mark "D.C." }"#)
            .unwrap();

        let dirs: Vec<&Direction> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        let dc_dir = dirs.iter().find(|d| d.da_capo.is_some()).expect("expected D.C. direction");
        assert_eq!(dc_dir.da_capo.as_deref(), Some("D.C."));
    }

    #[test]
    fn test_parse_mark_dal_segno() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' \mark "D.S. al Coda" }"#)
            .unwrap();

        let dirs: Vec<&Direction> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        let ds_dir = dirs.iter().find(|d| d.dal_segno.is_some()).expect("expected D.S. direction");
        assert_eq!(ds_dir.dal_segno.as_deref(), Some("D.S. al Coda"));
    }

    #[test]
    fn test_parse_mark_coda() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"{ c'4 \mark \markup { \musicglyph "scripts.coda" } d'4 }"#,
            )
            .unwrap();

        let dirs: Vec<&Direction> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        assert!(dirs.iter().any(|d| d.coda), "expected coda direction");
    }

    #[test]
    fn test_parse_mark_segno() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"{ c'4 \mark \markup { \musicglyph "scripts.segno" } d'4 }"#,
            )
            .unwrap();

        let dirs: Vec<&Direction> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        assert!(dirs.iter().any(|d| d.segno), "expected segno direction");
    }

    #[test]
    fn test_parse_paper_block() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\paper {
  paper-height = 29.70\cm
  paper-width = 21.00\cm
  left-margin = 2.00\cm
}
{ c'4 d' e' f' }"#,
            )
            .unwrap();

        let layout = score.page_layout.as_ref().expect("expected page_layout");
        assert!((layout.page_height.unwrap() - 29.70).abs() < 0.01);
        assert!((layout.page_width.unwrap() - 21.00).abs() < 0.01);
        assert!((layout.left_margin.unwrap() - 2.00).abs() < 0.01);
    }

    #[test]
    fn test_parse_arpeggio_default_up() {
        // Without explicit direction, \arpeggio should default to Up
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::Up));
    }

    #[test]
    fn test_single_note_acciaccatura() {
        // \acciaccatura d''8 should produce a grace note without braces
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \acciaccatura d''8 c''4 }"#)
            .unwrap();

        let part = &score.parts()[0];
        let elems = &part.measures[0].voices[0].elements;

        // First element should be the grace note
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace, "first note should be a grace note");
                assert!(n.grace_slash, "acciaccatura should have slash");
            }
            other => panic!("expected Note, got {:?}", other),
        }
        // Second element should be the main note
        match &elems[1] {
            VoiceElement::Note(n) => {
                assert!(!n.is_grace, "second note should not be grace");
            }
            other => panic!("expected Note, got {:?}", other),
        }
    }

    #[test]
    fn test_single_note_grace() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \grace e'16 c'4 }"#)
            .unwrap();

        let part = &score.parts()[0];
        let elems = &part.measures[0].voices[0].elements;

        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace, "first note should be grace");
                assert!(!n.grace_slash, "\\grace should not have slash");
            }
            other => panic!("expected Note, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Regression tests for example.ly features
    // -----------------------------------------------------------------------

    #[test]
    fn test_staff_variable_with_new_staff() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
staffA = \new Staff {
  \set Staff.instrumentName = "Violin"
  \set Staff.midiInstrument = "violin"
  \key c \major
  \clef treble
  \relative c' { c4 d e f | }
}
staffB = \new Staff {
  \set Staff.instrumentName = "Cello"
  \set Staff.midiInstrument = "cello"
  \key c \major
  \clef bass
  \relative c { c4 d e f | }
}
\score { << \staffA \staffB >> }
"#,
            )
            .unwrap();

        let parts = score.parts();
        assert_eq!(parts.len(), 2, "should have 2 parts from 2 staff variables");
        assert_eq!(parts[0].name, "Violin");
        assert_eq!(parts[0].midi_instrument, "violin");
        assert_eq!(parts[1].name, "Cello");
        assert_eq!(parts[1].midi_instrument, "cello");
        // Check notes exist
        let notes_a: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes_a.len(), 4);
    }

    #[test]
    fn test_set_instrument_name() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\new Staff {
  \set Staff.instrumentName = "Trumpet"
  \set Staff.midiInstrument = "trumpet"
  c'4 d' e' f'
}"#,
            )
            .unwrap();
        let part = &score.parts()[0];
        assert_eq!(part.name, "Trumpet");
        assert_eq!(part.midi_instrument, "trumpet");
    }

    #[test]
    fn test_context_voice_named() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\new Staff {
  \context Voice = "melody" { c'4 d' e' f' }
}"#,
            )
            .unwrap();
        let parts = score.parts();
        assert_eq!(parts.len(), 1, "should be 1 part, Voice doesn't create a new part");
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 4);
    }

    #[test]
    fn test_lyrics_variable_and_lyricsto() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
verse = \lyricmode { hel -- lo world }
staffSop = \new Staff {
  \context Voice = "sop" { c'4 d' e' }
}
\score {
  <<
    \staffSop
    \context Lyrics = "lsop" \lyricmode { \lyricsto "sop" \verse }
  >>
}
"#,
            )
            .unwrap();
        let parts = score.parts();
        assert_eq!(parts.len(), 1, "Lyrics context should not create an extra part");
        // Check that lyrics were attached to notes
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert!(notes.len() >= 3, "should have at least 3 notes");
        // First note should have lyric "hel"
        assert!(!notes[0].lyrics.is_empty(), "first note should have a lyric");
        assert_eq!(notes[0].lyrics[0].text, "hel");
        assert_eq!(notes[0].lyrics[0].syllabic, SyllabicType::Begin);
    }

    #[test]
    fn test_six_part_score_from_variables() {
        let adapter = LyToIrAdapter::new().with_language(PitchLanguage::Deutsch);
        let source = std::fs::read_to_string("tests/fixtures/ly/example.ly").unwrap();
        let score = adapter.convert_str(&source).unwrap();

        let parts = score.parts();
        assert_eq!(parts.len(), 6, "example.ly should produce 6 parts");
        assert_eq!(parts[0].name, "Corno da Caccia");
        assert_eq!(parts[1].name, "Violino I");
        assert_eq!(parts[2].name, "Violino II");
        assert_eq!(parts[3].name, "Viola");
        assert_eq!(parts[4].name, "Soprano");
        assert_eq!(parts[5].name, "Basso");

        // Check MIDI instruments
        assert_eq!(parts[0].midi_instrument, "french horn");
        assert_eq!(parts[1].midi_instrument, "violin");
        assert_eq!(parts[3].midi_instrument, "viola");
        assert_eq!(parts[5].midi_instrument, "harpsichord");

        // Check that each part has measures
        for (i, part) in parts.iter().enumerate() {
            assert!(
                !part.measures.is_empty(),
                "Part {} ({}) should have measures",
                i,
                part.name
            );
        }

        // Check soprano part has lyrics
        let sop_notes: Vec<&Note> = parts[4]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        let notes_with_lyrics = sop_notes.iter().filter(|n| !n.lyrics.is_empty()).count();
        assert!(
            notes_with_lyrics > 0,
            "Soprano part should have notes with lyrics attached"
        );
    }

    #[test]
    fn test_cadenza_and_melisma_dont_crash() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"{
  \cadenzaOn c'2 \bar "|" \cadenzaOff
  c'4\melisma d' e'\melismaEnd f'
  \autoBeamOff c'8 d' e' f'
  \dynamicUp c'4\f d'\p
}"#,
            )
            .unwrap();
        let parts = score.parts();
        assert!(!parts.is_empty());
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        // Should have parsed all notes without crashing
        assert!(notes.len() >= 10, "should parse notes despite cadenza/melisma commands");
    }

    #[test]
    fn test_ly_to_mxml_roundtrip_example() {
        // Test the full ly → IR → MusicXML pipeline doesn't lose parts
        let adapter = LyToIrAdapter::new().with_language(PitchLanguage::Deutsch);
        let source = std::fs::read_to_string("tests/fixtures/ly/example.ly").unwrap();
        let score = adapter.convert_str(&source).unwrap();

        let mxml_adapter = crate::adapters::ir_to_mxml::IrToMxmlAdapter::new();
        let xml = crate::adapters::FromIrAdapter::convert(&mxml_adapter, &score).unwrap();

        // Verify all 6 parts appear in XML
        let part_count = xml.matches("<score-part ").count();
        assert_eq!(part_count, 6, "MusicXML should have 6 <score-part> elements");

        // Verify part names
        assert!(xml.contains("<part-name>Corno da Caccia</part-name>"));
        assert!(xml.contains("<part-name>Violino I</part-name>"));
        assert!(xml.contains("<part-name>Soprano</part-name>"));
        assert!(xml.contains("<part-name>Basso</part-name>"));

        // Verify MIDI instruments
        assert!(xml.contains("<midi-name>french horn</midi-name>"));
        assert!(xml.contains("<midi-name>violin</midi-name>"));

        // Verify lyrics in soprano part
        assert!(xml.contains("<lyric"), "Should contain lyrics in MusicXML output");
        assert!(xml.contains("<text>Men</text>"), "Should contain first lyric syllable");
    }

    #[test]
    fn test_figuremode_basic() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
bc = { c'1 | d'1 }
figs = \figuremode { <6 4>1 | <_+>1 }
\score { \new Staff <<\bc\figs>> }
"#;
        let score = adapter.convert_str(source).unwrap();
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        // Should have figured bass in the measures
        let total_figs: usize = parts[0].measures.iter().map(|m| m.figured_bass.len()).sum();
        assert!(total_figs > 0, "should have figured bass entries, got 0");

        // First measure should have figure [6, 4]
        let m1_figs = &parts[0].measures[0].figured_bass;
        assert_eq!(m1_figs.len(), 1);
        assert_eq!(m1_figs[0].figures.len(), 2);
        assert_eq!(m1_figs[0].figures[0].number, Some(6));
        assert_eq!(m1_figs[0].figures[1].number, Some(4));

        // Second measure should have figure [_+] (sharp on placeholder)
        if parts[0].measures.len() > 1 {
            let m2_figs = &parts[0].measures[1].figured_bass;
            assert_eq!(m2_figs.len(), 1);
            assert_eq!(m2_figs[0].figures.len(), 1);
            assert_eq!(m2_figs[0].figures[0].number, None);
            assert_eq!(m2_figs[0].figures[0].suffix.as_deref(), Some("sharp"));
        }
    }

    #[test]
    fn test_figuremode_accidentals() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
bc = { c'1 }
figs = \figuremode { <6+ 4->1 }
\score { \new Staff <<\bc\figs>> }
"#;
        let score = adapter.convert_str(source).unwrap();
        let parts = score.parts();
        let m1_figs = &parts[0].measures[0].figured_bass;
        assert_eq!(m1_figs.len(), 1);
        assert_eq!(m1_figs[0].figures.len(), 2);
        assert_eq!(m1_figs[0].figures[0].number, Some(6));
        assert_eq!(m1_figs[0].figures[0].suffix.as_deref(), Some("sharp"));
        assert_eq!(m1_figs[0].figures[1].number, Some(4));
        assert_eq!(m1_figs[0].figures[1].suffix.as_deref(), Some("flat"));
    }

    #[test]
    fn test_figuremode_distribution_across_measures() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
bc = { c'1 | d'1 | e'1 }
figs = \figuremode { <6>1 s1 <4 3>1 }
forma = { \time 4/4 \key c\major s1*3 }
\score { \new Staff { \clef bass <<\bc\forma\figs>> } }
"#;
        let score = adapter.convert_str(source).unwrap();
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        assert!(parts[0].measures.len() >= 3, "should have at least 3 measures");
        // Measure 1: <6>
        assert_eq!(parts[0].measures[0].figured_bass.len(), 1);
        assert_eq!(parts[0].measures[0].figured_bass[0].figures[0].number, Some(6));
        // Measure 2: skip (no figures)
        assert_eq!(parts[0].measures[1].figured_bass.len(), 0);
        // Measure 3: <4 3>
        assert_eq!(parts[0].measures[2].figured_bass.len(), 1);
        assert_eq!(parts[0].measures[2].figured_bass[0].figures.len(), 2);
    }

    #[test]
    fn test_cautionary_accidental_measure_split() {
        // Cautionary accidental `!` after pitch should not disrupt bar splitting
        let adapter = LyToIrAdapter::new();
        let src = "\\language \"italiano\"\n{ \\time 4/4 mi''8[mi la8. mi16] fad!8 sol16 la fad8. sol16 sol4 r r2 }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        assert_eq!(part.measures.len(), 2, "Expected 2 measures");
        // Each measure should sum to exactly 1 whole note
        for m in &part.measures {
            let total: Frac = m.voices.iter()
                .flat_map(|v| &v.elements)
                .map(voice_element_duration)
                .fold(Frac::from_integer(0), |a, b| a + b);
            assert_eq!(total, Frac::from_integer(1), "measure {} should be 1 whole note", m.number);
        }
    }

    #[test]
    fn test_multi_movement_scores() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
\version "2.24.0"
melA = { c'4 d' e' f' }
melB = { g'4 a' b' c'' }
\score { \new Staff \melA }
\score { \new Staff \melB }
"#;
        let scores = adapter.convert_str_multi(source).unwrap();
        assert_eq!(scores.len(), 2, "should produce 2 scores (movements)");
        // Each score should have 1 part
        assert_eq!(scores[0].parts().len(), 1);
        assert_eq!(scores[1].parts().len(), 1);
        // Each part should have notes
        let notes0: Vec<&Note> = scores[0].parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e { VoiceElement::Note(n) => Some(n.as_ref()), _ => None })
            .collect();
        let notes1: Vec<&Note> = scores[1].parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e { VoiceElement::Note(n) => Some(n.as_ref()), _ => None })
            .collect();
        assert_eq!(notes0.len(), 4);
        assert_eq!(notes1.len(), 4);
    }

    #[test]
    fn test_multi_movement_example2() {
        let adapter = LyToIrAdapter::new().with_language(PitchLanguage::Nederlands);
        let source = std::fs::read_to_string("tests/fixtures/ly/example2.ly").unwrap();
        let scores = adapter.convert_str_multi(&source).unwrap();
        assert_eq!(scores.len(), 2, "example2.ly has two \\score blocks");
        // Movement 1: 4 parts, G major (1 sharp)
        assert_eq!(scores[0].parts().len(), 4);
        // Movement 2: 4 parts, F major (1 flat)
        assert_eq!(scores[1].parts().len(), 4);
    }

    #[test]
    fn test_explicit_beam_brackets() {
        let adapter = LyToIrAdapter::new();
        let src = "{ c'8[ d' e' f'] }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 4);
        // First note: beam begin at level 1
        assert!(
            notes[0].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1),
            "first note should have beam begin: {:?}",
            notes[0].beams
        );
        // Middle notes: beam continue at level 1
        assert!(
            notes[1].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1),
            "second note should have beam continue: {:?}",
            notes[1].beams
        );
        assert!(
            notes[2].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1),
            "third note should have beam continue: {:?}",
            notes[2].beams
        );
        // Last note: beam end at level 1
        assert!(
            notes[3].beams.iter().any(|b| b.beam_type == "end" && b.number == 1),
            "last note should have beam end: {:?}",
            notes[3].beams
        );
    }

    #[test]
    fn test_auto_beam_eighths_in_4_4() {
        // Eight eighth notes in 4/4 should be beamed in groups of 4 (per half note)
        let adapter = LyToIrAdapter::new();
        let src = "{ \\time 4/4 c'8 d' e' f' g' a' b' c'' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 8, "should have 8 eighth notes");
        // Group 1 (half note 1): c' d' e' f' (begin, continue, continue, end)
        assert!(notes[0].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[1].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[2].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[3].beams.iter().any(|b| b.beam_type == "end" && b.number == 1));
        // Group 2 (half note 2): g' a' b' c'' (begin, continue, continue, end)
        assert!(notes[4].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[5].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[6].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[7].beams.iter().any(|b| b.beam_type == "end" && b.number == 1));
    }

    #[test]
    fn test_auto_beam_compound_6_8() {
        // In 6/8, beam in groups of 3 eighth notes
        let adapter = LyToIrAdapter::new();
        let src = "{ \\time 6/8 c'8 d' e' f' g' a' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 6, "should have 6 eighth notes");
        // Group 1: c' d' e' (begin, continue, end)
        assert!(notes[0].beams.iter().any(|b| b.beam_type == "begin"));
        assert!(notes[1].beams.iter().any(|b| b.beam_type == "continue"));
        assert!(notes[2].beams.iter().any(|b| b.beam_type == "end"));
        // Group 2: f' g' a' (begin, continue, end)
        assert!(notes[3].beams.iter().any(|b| b.beam_type == "begin"));
        assert!(notes[4].beams.iter().any(|b| b.beam_type == "continue"));
        assert!(notes[5].beams.iter().any(|b| b.beam_type == "end"));
    }

    #[test]
    #[allow(clippy::vec_init_then_push)]
    fn test_stem_direction_commands() {
        let adapter = LyToIrAdapter::new();
        let src = "{ \\stemUp c'8 d' \\stemDown e' f' \\stemNeutral g' a' b' c'' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 8);
        // \stemUp applies to first two
        assert_eq!(notes[0].stem_direction, "up");
        assert_eq!(notes[1].stem_direction, "up");
        // \stemDown applies to next two
        assert_eq!(notes[2].stem_direction, "down");
        assert_eq!(notes[3].stem_direction, "down");
        // \stemNeutral → auto-stem for remaining
        // g' (G4, MIDI 67) < 71 → up
        assert_eq!(notes[4].stem_direction, "up", "G4 auto-stem should be up");
    }

    #[test]
    fn test_auto_stem_direction() {
        // Notes above B4 should have stem down, below should have stem up
        let adapter = LyToIrAdapter::new();
        let src = "{ c'4 b' c'' }"; // C4=60, B4=71, C5=72
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes[0].stem_direction, "up", "C4 below middle → up");
        assert_eq!(notes[1].stem_direction, "down", "B4 on middle line → down");
        assert_eq!(notes[2].stem_direction, "down", "C5 above middle → down");
    }

    #[test]
    fn test_acciaccatura_no_measure_duration() {
        // Grace notes should not affect measure duration tracking
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 \acciaccatura d''8 c''2 e''8 d'' c'' b' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        assert_eq!(part.measures.len(), 1, "acciaccatura should not cause extra measure split");
    }

    #[test]
    fn test_auto_beam_16ths_grouped_by_4() {
        // 16th notes in 4/4 should be grouped by 4 (per quarter note)
        let adapter = LyToIrAdapter::new();
        let src = "{ \\time 4/4 c'16 d' e' f' g' a' b' c'' d'' e'' f'' g'' a'' b'' c''' d''' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        assert_eq!(notes.len(), 16);
        // Group 1 (beat 1): notes 0-3
        assert!(notes[0].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[3].beams.iter().any(|b| b.beam_type == "end" && b.number == 1));
        // Group 2 (beat 2): notes 4-7
        assert!(notes[4].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[7].beams.iter().any(|b| b.beam_type == "end" && b.number == 1));
        // Group 3 (beat 3): notes 8-11
        assert!(notes[8].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[11].beams.iter().any(|b| b.beam_type == "end" && b.number == 1));
    }

    #[test]
    fn test_tuplet_beam_isolation() {
        // Tuplet 8ths should beam only within the tuplet, not with adjacent notes
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 c'4 \tuplet 3/2 { d'8 e' f' } g'4 }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        // c'4 d'8 e'8 f'8 g'4 = 5 notes
        assert_eq!(notes.len(), 5);
        // Tuplet notes (1,2,3) should be beamed together
        assert!(notes[1].beams.iter().any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[2].beams.iter().any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[3].beams.iter().any(|b| b.beam_type == "end" && b.number == 1));
        // Non-tuplet quarter notes should have no beams
        assert!(notes[0].beams.is_empty());
        assert!(notes[4].beams.is_empty());
    }

    #[test]
    fn test_alto_clef_auto_stem() {
        // In alto clef, middle line is C4 (MIDI 60)
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \clef "alto" \time 4/4 b4 c' d' e' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        assert_eq!(notes.len(), 4);
        // B3 = MIDI 59, below C4 → up
        assert_eq!(notes[0].stem_direction, "up", "B3 below alto middle → up");
        // C4 = MIDI 60, on middle line → down
        assert_eq!(notes[1].stem_direction, "down", "C4 on alto middle → down");
        // D4 = MIDI 62, above middle → down
        assert_eq!(notes[2].stem_direction, "down", "D4 above alto middle → down");
    }

    #[test]
    fn test_lyric_melisma_skip() {
        // `_` in lyrics should skip a note (melisma extension)
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "melody" { c'4 d' e' f' }
    \new Lyrics \lyricsto "melody" \lyricmode { hello _ world _ }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<&Note> = parts[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        assert!(notes.len() >= 4);
        // Note 0 should have "hello"
        assert_eq!(notes[0].lyrics.len(), 1, "note 0 should have a lyric");
        assert_eq!(notes[0].lyrics[0].text, "hello");
        // Note 1 should have no lyric (melisma skip)
        assert!(notes[1].lyrics.is_empty(), "note 1 should have no lyric (melisma skip)");
        // Note 2 should have "world"
        assert_eq!(notes[2].lyrics.len(), 1, "note 2 should have a lyric");
        assert_eq!(notes[2].lyrics[0].text, "world");
        // Note 3 should have no lyric (melisma skip)
        assert!(notes[3].lyrics.is_empty(), "note 3 should have no lyric (melisma skip)");
    }

    #[test]
    fn test_lyric_single_hyphen_separator() {
        // Single `-` between lyrics should work as syllable separator
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "v" { c'4 d' e' f' }
    \new Lyrics \lyricsto "v" \lyricmode { fi - li - ae rest }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<&Note> = parts[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        assert!(notes.len() >= 4);
        assert_eq!(notes[0].lyrics[0].text, "fi");
        assert_eq!(notes[0].lyrics[0].syllabic, SyllabicType::Begin);
        assert_eq!(notes[1].lyrics[0].text, "li");
        assert_eq!(notes[1].lyrics[0].syllabic, SyllabicType::Middle);
        assert_eq!(notes[2].lyrics[0].text, "ae");
        assert_eq!(notes[2].lyrics[0].syllabic, SyllabicType::End);
        assert_eq!(notes[3].lyrics[0].text, "rest");
        assert_eq!(notes[3].lyrics[0].syllabic, SyllabicType::Single);
    }

    #[test]
    fn test_auto_beam_off() {
        // \autoBeamOff should prevent auto-beaming
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 \autoBeamOff c'8 d' e' f' g' a' b' c'' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        assert_eq!(notes.len(), 8);
        // All notes should have no beams (auto-beaming suppressed)
        for (i, note) in notes.iter().enumerate() {
            assert!(note.beams.is_empty(), "note {} should have no beams with \\autoBeamOff", i);
        }
    }

    #[test]
    fn test_auto_beam_off_with_explicit_brackets() {
        // \autoBeamOff with explicit [] should still beam those notes
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 \autoBeamOff c'8[ d'] e' f' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| if let VoiceElement::Note(n) = e { Some(n.as_ref()) } else { None })
            .collect();
        // First two notes should have explicit beams
        assert!(!notes[0].beams.is_empty(), "note 0 should have beam from [");
        assert!(!notes[1].beams.is_empty(), "note 1 should have beam from ]");
        // Remaining notes should have no beams
        assert!(notes[2].beams.is_empty(), "note 2 should have no beams");
        assert!(notes[3].beams.is_empty(), "note 3 should have no beams");
    }

    #[test]
    fn test_time_sig_change_measure_duration() {
        // When time signature changes, measure durations should match the new time sig
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 c'4 d' e' f' | \time 3/4 g'4 a' b' | c''2. }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        assert!(part.measures.len() >= 3, "should have at least 3 measures");
        // Measure 1: 4/4 = 4 quarter notes
        let m1_dur: Frac = part.measures[0].voices[0].elements.iter()
            .map(|e| voice_element_duration(e)).sum();
        assert_eq!(m1_dur, Frac::new(1, 1), "m1 should be 1 whole");
        // Measure 2: 3/4 = 3 quarter notes
        let m2_dur: Frac = part.measures[1].voices[0].elements.iter()
            .map(|e| voice_element_duration(e)).sum();
        assert_eq!(m2_dur, Frac::new(3, 4), "m2 should be 3/4");
    }

}

/// Parse a `\figuremode { ... }` expression block into a flat stream of figured bass entries.
///
/// Inside figuremode, `<6 4>` is a chord node containing figure numbers and accidentals.
/// `s` is a skip (spacer). `|` is a bar check. Durations follow the same syntax as notes.
/// The flat stream preserves duration information so that figures can be distributed
/// across measures during resolve by matching cumulative durations.
fn parse_figuremode_block(state: &WalkState, block: Node) -> Vec<FiguredBassEntry> {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    let mut entries: Vec<FiguredBassEntry> = Vec::new();
    let mut last_dur = Duration::quarter();

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "chord" => {
                // Parse figure group: <6 4>, <_+>, <6+>, <7 3+ 9>, etc.
                let figures = parse_figure_chord(state, node);
                i += 1;
                // Consume duration after chord
                let dur = consume_duration_stateless(&children, &mut i, &mut last_dur, state.source.as_bytes());
                entries.push(FiguredBassEntry::Figure(FiguredBass {
                    figures,
                    duration: dur,
                    parentheses: false,
                    offset: 0,
                }));
                continue;
            }
            "symbol" => {
                let sym = state.text(node);
                if sym == "s" {
                    // Spacer — skip with duration
                    i += 1;
                    let dur = consume_duration_stateless(&children, &mut i, &mut last_dur, state.source.as_bytes());
                    let count = consume_multiplier_stateless(state, &children, &mut i);
                    for _ in 0..count {
                        entries.push(FiguredBassEntry::Skip(dur.clone()));
                    }
                    continue;
                }
            }
            "escaped_word" => {
                // Skip figuremode commands like \bassFigureExtendersOff
            }
            _ => {}
        }
        i += 1;
    }

    entries
}

/// Parse a single chord node inside figuremode into a list of figures.
///
/// The chord node children are:
/// - `<` / `>` structural tokens
/// - `unsigned_integer` for figure numbers (6, 4, 11, etc.)
/// - `punctuation` `_` for placeholder (no number)
/// - `punctuation` `+` / `-` for accidental modifiers on the preceding figure
fn parse_figure_chord(state: &WalkState, chord_node: Node) -> Vec<Figure> {
    let mut cursor = chord_node.walk();
    let children: Vec<Node> = chord_node.children(&mut cursor).collect();
    let mut figures: Vec<Figure> = Vec::new();

    let mut i = 0;
    while i < children.len() {
        let child = children[i];
        match child.kind() {
            "unsigned_integer" => {
                let num: u8 = state.text(child).parse().unwrap_or(0);
                // Check if next child is an accidental modifier
                let suffix = peek_accidental(state, &children, i + 1);
                if suffix.is_some() {
                    i += 1; // skip the accidental
                }
                figures.push(Figure {
                    number: Some(num),
                    prefix: None,
                    suffix: suffix.map(|s| s.to_string()),
                });
            }
            "punctuation" => {
                let text = state.text(child);
                if text == "_" {
                    // Placeholder figure — check for following accidental
                    let suffix = peek_accidental(state, &children, i + 1);
                    if suffix.is_some() {
                        i += 1;
                    }
                    figures.push(Figure {
                        number: None,
                        prefix: None,
                        suffix: suffix.map(|s| s.to_string()),
                    });
                }
                // Skip `<`, `>`, and other punctuation
            }
            _ => {}
        }
        i += 1;
    }

    figures
}

/// Peek at the next child to see if it's an accidental modifier (`+` or `-`).
fn peek_accidental<'a>(state: &WalkState<'a>, children: &[Node<'a>], idx: usize) -> Option<&'static str> {
    if let Some(next) = children.get(idx) {
        if next.kind() == "punctuation" {
            let text = state.text(*next);
            match text {
                "+" => return Some("sharp"),
                "-" => return Some("flat"),
                _ => {}
            }
        }
    }
    None
}

/// Consume duration tokens without mutating WalkState (for figuremode parsing).
/// Returns the duration found, updating `last_dur` for carry-forward.
fn consume_duration_stateless(children: &[Node], i: &mut usize, last_dur: &mut Duration, source: &[u8]) -> Duration {
    // Look for unsigned_integer (duration value) followed by optional dots
    let mut dur_val: Option<u32> = None;
    let mut dots = 0u8;

    // Check for duration number
    if let Some(node) = children.get(*i) {
        if node.kind() == "unsigned_integer" {
            if let Ok(val) = node.utf8_text(source).unwrap_or("").parse::<u32>() {
                // Only valid duration values: 1, 2, 4, 8, 16, 32, 64, 128
                if matches!(val, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128) {
                    dur_val = Some(val);
                    *i += 1;
                }
            }
        }
    }

    // Consume dots
    while let Some(node) = children.get(*i) {
        if node.kind() == "punctuation" {
            if let Ok(text) = node.utf8_text(source) {
                if text == "." {
                    dots += 1;
                    *i += 1;
                    continue;
                }
            }
        }
        break;
    }

    if let Some(val) = dur_val {
        let mut dur = Duration::from_lilypond_number(val, 0).unwrap_or_else(|| Duration::quarter());
        dur.dots = dots;
        *last_dur = dur.clone();
        dur
    } else if dots > 0 {
        let mut dur = last_dur.clone();
        dur.dots = dots;
        *last_dur = dur.clone();
        dur
    } else {
        last_dur.clone()
    }
}

/// Consume a `*N` multiplier without mutating WalkState.
fn consume_multiplier_stateless(state: &WalkState, children: &[Node], i: &mut usize) -> u32 {
    if let Some(node) = children.get(*i) {
        if node.kind() == "punctuation" && state.text(*node) == "*" {
            *i += 1;
            if let Some(num_node) = children.get(*i) {
                if num_node.kind() == "unsigned_integer" {
                    if let Ok(count) = state.text(*num_node).parse::<u32>() {
                        *i += 1;
                        return count;
                    }
                }
            }
        }
    }
    1
}

/// Check if all measures contain only spacer rests (no real notes).
/// Used to detect "forma"-style variables that carry only attributes.
/// Distribute a flat stream of figured bass entries across measures.
///
/// Walks the entries and measures in parallel, tracking cumulative duration.
/// When the accumulated duration fills a measure (based on the current time
/// signature), advances to the next measure. Figures land in whichever
/// measure their start time falls into.
fn distribute_figured_bass(measures: &mut [Measure], entries: &[FiguredBassEntry]) {
    use crate::ir::duration::Frac;

    if measures.is_empty() {
        return;
    }

    // Track current time signature to know measure duration
    let mut measure_dur = Frac::new(4, 4); // default 4/4
    let mut measure_idx = 0usize;
    let mut elapsed_in_measure = Frac::from_integer(0);

    // Update measure_dur from initial attributes
    if let Some(ref attrs) = measures[0].attributes {
        if let Some(ref ts) = attrs.time {
            measure_dur = ts.beats_fraction().into();
        }
    }

    for entry in entries {
        // Advance to correct measure if we've exceeded current measure duration
        while elapsed_in_measure >= measure_dur && measure_idx + 1 < measures.len() {
            elapsed_in_measure = elapsed_in_measure - measure_dur;
            measure_idx += 1;
            // Check if the new measure changes time signature
            if let Some(ref attrs) = measures[measure_idx].attributes {
                if let Some(ref ts) = attrs.time {
                    measure_dur = ts.beats_fraction().into();
                }
            }
        }

        match entry {
            FiguredBassEntry::Figure(fb) => {
                if measure_idx < measures.len() {
                    measures[measure_idx].figured_bass.push(fb.clone());
                }
                elapsed_in_measure = elapsed_in_measure + fb.duration.actual_duration();
            }
            FiguredBassEntry::Skip(dur) => {
                elapsed_in_measure = elapsed_in_measure + dur.actual_duration();
            }
        }
    }
}

/// Get the sounding duration of a voice element (for auto bar-splitting).
/// Return the beam level for a note duration:
/// 0 = not beamable (quarter or longer), 1 = eighth, 2 = 16th, 3 = 32nd, 4 = 64th.
fn beam_level_for_duration(dur: &Duration) -> u8 {
    let d = *dur.base.denom();
    let n = *dur.base.numer();
    if n != 1 {
        return 0;
    }
    match d {
        8 => 1,
        16 => 2,
        32 => 3,
        64 => 4,
        128 => 5,
        _ => 0,
    }
}

fn voice_element_duration(elem: &VoiceElement) -> Frac {
    match elem {
        VoiceElement::Note(n) => n.duration.actual_duration(),
        VoiceElement::Rest(r) => r.duration.actual_duration(),
        VoiceElement::Chord(c) => c.duration.actual_duration(),
        VoiceElement::Forward(f) => f.duration.actual_duration(),
        VoiceElement::Backup(b) => -b.duration.actual_duration(),
    }
}

fn measures_are_spacer_only(measures: &[Measure]) -> bool {
    for m in measures {
        for voice in &m.voices {
            for elem in &voice.elements {
                match elem {
                    VoiceElement::Rest(r) if r.is_spacer || r.is_measure_rest => {}
                    VoiceElement::Rest(_) | VoiceElement::Note(_) | VoiceElement::Chord(_) => {
                        return false;
                    }
                    _ => {} // Forward/Backup are structural, not music
                }
            }
        }
    }
    true
}

/// Merge attributes, directions, and barlines from spacer-only measures into
/// existing measures. This handles the LilyPond pattern `<<\music \forma>>`
/// where `forma` carries time/key/tempo attributes with spacer rests.
fn merge_spacer_measures(target: &mut Vec<Measure>, spacer: &[Measure]) {
    for (i, sm) in spacer.iter().enumerate() {
        if i < target.len() {
            let tm = &mut target[i];
            // Merge attributes
            if let Some(ref sa) = sm.attributes {
                let ta = tm.attributes.get_or_insert_with(MeasureAttributes::default);
                if sa.key.is_some() && ta.key.is_none() {
                    ta.key = sa.key;
                }
                if sa.time.is_some() && ta.time.is_none() {
                    ta.time = sa.time.clone();
                }
                if !sa.clefs.is_empty() && ta.clefs.is_empty() {
                    ta.clefs = sa.clefs.clone();
                }
            }
            // Merge directions (tempo, etc.)
            if !sm.directions.is_empty() && tm.directions.is_empty() {
                tm.directions = sm.directions.clone();
            }
            // Merge barlines
            if sm.right_barline.is_some() && tm.right_barline.is_none() {
                tm.right_barline = sm.right_barline.clone();
            }
            if sm.left_barline.is_some() && tm.left_barline.is_none() {
                tm.left_barline = sm.left_barline.clone();
            }
        }
        // If spacer has more measures than target, we don't append them
        // (they're just spacers and don't contribute music)
    }
}

/// Re-split note measures to match the measure boundaries defined by spacer measures.
///
/// When a note variable (e.g. `IIvlIn`) was auto-split using the default time
/// signature during variable definition, but the spacer variable (`forma`) has
/// different time signatures, the measure boundaries are misaligned. This function
/// flattens all voice elements from the note measures and redistributes them into
/// new measures matching the spacer measures' actual durations.
fn resplit_measures_to_match(
    note_measures: &[Measure],
    spacer_measures: &[Measure],
) -> Vec<Measure> {
    // 1. Flatten all voice elements from note measures, tracking which
    // element index corresponds to each note measure's start
    let mut elements: Vec<VoiceElement> = Vec::new();
    let mut note_measure_attrs: Vec<(usize, MeasureAttributes)> = Vec::new();
    for m in note_measures {
        if let Some(ref attrs) = m.attributes {
            note_measure_attrs.push((elements.len(), attrs.clone()));
        }
        for v in &m.voices {
            elements.extend(v.elements.iter().cloned());
        }
    }

    // 2. Compute actual duration of each spacer measure from its content
    let spacer_durations: Vec<Frac> = spacer_measures
        .iter()
        .map(|m| {
            let mut dur = Frac::from_integer(0);
            for v in &m.voices {
                for e in &v.elements {
                    dur = dur + voice_element_duration(e);
                }
            }
            dur
        })
        .collect();

    // 3. Re-distribute elements into new measures
    let mut result: Vec<Measure> = Vec::new();
    let mut elem_idx = 0usize;
    let mut note_attr_idx = 0usize; // tracks which note_measure_attrs we've consumed

    for (si, sm) in spacer_measures.iter().enumerate() {
        let measure_dur = spacer_durations[si];
        let mut new_measure = Measure::new(sm.number);
        new_measure.implicit = sm.implicit;
        // Copy attributes from spacer (has correct time sig, key, etc.)
        new_measure.attributes = sm.attributes.clone();
        new_measure.directions = sm.directions.clone();
        new_measure.left_barline = sm.left_barline.clone();
        new_measure.right_barline = sm.right_barline.clone();

        // Fill with voice elements up to this measure's duration
        let mut elapsed = Frac::from_integer(0);
        let mut voice_elements: Vec<VoiceElement> = Vec::new();

        while elem_idx < elements.len() && measure_dur > Frac::from_integer(0) {
            let dur = voice_element_duration(&elements[elem_idx]);
            // If this element would overflow and we already have content, break
            if elapsed + dur > measure_dur && elapsed > Frac::from_integer(0) {
                break;
            }
            voice_elements.push(elements[elem_idx].clone());
            elapsed = elapsed + dur;
            elem_idx += 1;
            if elapsed >= measure_dur {
                break;
            }
        }

        if !voice_elements.is_empty() {
            new_measure.voices.push(Voice {
                number: 1,
                elements: voice_elements,
            });
        }

        // Merge attributes from note measures that fall within this output measure's range
        while note_attr_idx < note_measure_attrs.len()
            && note_measure_attrs[note_attr_idx].0 < elem_idx
        {
            let (_, ref note_attrs) = note_measure_attrs[note_attr_idx];
            let ma = new_measure
                .attributes
                .get_or_insert_with(MeasureAttributes::default);
            // Merge clefs from note measures (spacer measures typically don't have clefs)
            if !note_attrs.clefs.is_empty() && ma.clefs.is_empty() {
                ma.clefs = note_attrs.clefs.clone();
            }
            note_attr_idx += 1;
        }

        result.push(new_measure);
    }

    // 4. Handle remaining elements beyond spacer measures
    if elem_idx < elements.len() {
        // Use last known measure duration or default to 4/4
        let last_dur = spacer_durations.last().copied().unwrap_or(Frac::new(4, 4));
        while elem_idx < elements.len() {
            let mnum = result.len() as u32 + 1;
            let mut m = Measure::new(mnum);
            let mut elapsed = Frac::from_integer(0);
            let mut voice_elements: Vec<VoiceElement> = Vec::new();

            while elem_idx < elements.len() {
                let dur = voice_element_duration(&elements[elem_idx]);
                if elapsed + dur > last_dur && elapsed > Frac::from_integer(0) {
                    break;
                }
                voice_elements.push(elements[elem_idx].clone());
                elapsed = elapsed + dur;
                elem_idx += 1;
                if elapsed >= last_dur {
                    break;
                }
            }

            if !voice_elements.is_empty() {
                m.voices.push(Voice {
                    number: 1,
                    elements: voice_elements,
                });
            }
            result.push(m);
        }
    }

    result
}

/// Apply tuplet ratio to a voice element's duration.
/// Called from `push_voice_element` so that measure duration tracking is correct.
fn apply_tuplet_ratio(elem: &mut VoiceElement, actual: u8, normal: u8) {
    match elem {
        VoiceElement::Note(n) => {
            n.duration.tuplet_actual = actual;
            n.duration.tuplet_normal = normal;
        }
        VoiceElement::Rest(r) => {
            r.duration.tuplet_actual = actual;
            r.duration.tuplet_normal = normal;
        }
        VoiceElement::Chord(c) => {
            c.duration.tuplet_actual = actual;
            c.duration.tuplet_normal = normal;
        }
        _ => {}
    }
}

/// Set tuplet display markers (start/stop brackets) on a slice of voice elements.
fn apply_tuplet_display(elements: &mut [VoiceElement], _actual: u8) {
    let len = elements.len();
    for (i, elem) in elements.iter_mut().enumerate() {
        let is_first = i == 0;
        let is_last = i == len - 1;
        if !is_first && !is_last {
            continue;
        }
        match elem {
            VoiceElement::Note(n) => {
                if is_first {
                    n.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Start,
                        bracket: true,
                        show_number: "actual".to_string(),
                    });
                } else if is_last {
                    n.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Stop,
                        bracket: true,
                        show_number: String::new(),
                    });
                }
            }
            VoiceElement::Rest(r) => {
                if is_first {
                    r.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Start,
                        bracket: true,
                        show_number: "actual".to_string(),
                    });
                } else if is_last {
                    r.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Stop,
                        bracket: true,
                        show_number: String::new(),
                    });
                }
            }
            VoiceElement::Chord(c) => {
                if let Some(first_note) = c.notes.first_mut() {
                    if is_first {
                        first_note.tuplet = Some(TupletDisplay {
                            tuplet_type: StartStop::Start,
                            bracket: true,
                            show_number: "actual".to_string(),
                        });
                    } else if is_last {
                        first_note.tuplet = Some(TupletDisplay {
                            tuplet_type: StartStop::Stop,
                            bracket: true,
                            show_number: String::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}
