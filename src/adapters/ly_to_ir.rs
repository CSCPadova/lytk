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
    Articulation, DynamicMark, Fermata, LyricSyllable, Placement, SlurEvent, StartStop,
    SyllabicType, TieEvent, TupletDisplay, Wedge,
};
use crate::ir::direction::{Barline, BarlineType, Direction, TempoDirection};
use crate::ir::duration::Duration;
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

/// What a variable definition expands to.
#[derive(Clone)]
enum VarDef {
    /// Variable contained `\new Staff { ... }` — stores the full part(s).
    Parts(Vec<(String, Part)>),
    /// Variable contained bare music — stores just the measures.
    Measures(Vec<Measure>),
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
            prev_pitch: None,
            relative_ref: None,
            in_relative: false,
            pending_arpeggio_type: None,
            pending_glissando_style: None,
            pending_slide: false,
            page_layout: None,
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
                    let part = self.ensure_part();
                    part.measures.extend(measures);
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

    /// Parse LilyPond source text into an IR Score.
    fn parse_source(&self, source: &str) -> Result<Score> {
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

        // Flush any remaining state
        state.flush_measure();

        // Attach pending lyrics to matching parts
        for (voice_name, syllables) in &state.pending_lyrics {
            if let Some(&part_idx) = state.voice_part_map.get(voice_name) {
                if let Some((_, part)) = state.parts.get_mut(part_idx) {
                    attach_lyrics_to_part(part, syllables);
                }
            }
        }

        // Build the Score
        let mut score = Score::new();
        score.metadata = state.metadata;
        score.metadata.pitch_mode = state.mode;
        score.metadata.pitch_language = Some(state.language);
        score.page_layout = state.page_layout;
        for (_, part) in state.parts {
            score.children.push(ScoreChild::Part(part));
        }
        // If no parts were created, create an empty one
        if score.children.is_empty() {
            score.children.push(ScoreChild::Part(Part::new("P1")));
        }

        Ok(score)
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
                        // \score { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                walk_score_block(state, *next);
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
                            // Skip \lyricmode or \new before the expression_block
                            let mut is_lyricmode = false;
                            while j < children.len() {
                                let candidate = children[j];
                                if candidate.kind() == "escaped_word" {
                                    let ew = state.text(candidate);
                                    if ew == "\\lyricmode" || ew == "\\notemode" {
                                        is_lyricmode = ew == "\\lyricmode";
                                        j += 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                            if let Some(next) = children.get(j) {
                                if next.kind() == "expression_block" {
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
                                        state.measure_num = 0;
                                        walk_music_block(state, *next);
                                        state.flush_measure();
                                        // Extract newly created parts
                                        let new_parts: Vec<_> =
                                            state.parts.drain(parts_before..).collect();
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
                                    state.measure_num = 0;
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
/// Consumes tokens after the named_context and stores lyrics for later attachment.
fn walk_lyrics_context(
    state: &mut WalkState,
    children: &[Node],
    mut i: usize,
    _name: &str,
) -> usize {
    // After named_context(Lyrics), we expect:
    // \lyricmode { \lyricsto "voiceName" \variable_or_lyrics }
    let mut is_lyricmode = false;
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
                break;
            }
            "expression_block" => {
                if is_lyricmode {
                    // Parse the lyric block and find the \lyricsto voice name
                    let voice_name = extract_lyricsto_voice(state, node);
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
                state.current_voice.push(VoiceElement::Chord(chord));
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
            state.current_voice.push(VoiceElement::Rest(rest));
        }
        "R" => {
            // Whole-measure rest, possibly with *N multiplier (e.g. R1*3)
            let dur = consume_duration(state, children, &mut i);
            // Check for *N multiplier for multi-measure rests
            let count = consume_duration_multiplier(state, children, &mut i);
            let attachments = consume_attachments(state, children, &mut i);
            let mut rest = Rest::measure_rest(dur.clone());
            apply_rest_attachments(&mut rest, &attachments);
            state.current_voice.push(VoiceElement::Rest(rest));
            // Expand R1*N into N separate measure rests with bar checks
            if count > 1 {
                for _ in 1..count {
                    state.bar_check();
                    let rest = Rest::measure_rest(dur.clone());
                    state.current_voice.push(VoiceElement::Rest(rest));
                }
            }
        }
        "s" => {
            // Spacer rest
            let dur = consume_duration(state, children, &mut i);
            let mut rest = Rest::new(dur);
            rest.is_spacer = true;
            state.current_voice.push(VoiceElement::Rest(rest));
        }
        _ => {
            // Try as pitch name
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                // Consume octave marks
                let octave_marks = consume_octave_marks(state, children, &mut i);
                let dur = consume_duration(state, children, &mut i);
                let attachments = consume_attachments(state, children, &mut i);

                let pitch = state.resolve_pitch(step, alter, octave_marks);
                let mut note = Note::new(pitch, dur);
                apply_note_attachments(state, &mut note, &attachments);
                state.current_voice.push(VoiceElement::Note(Box::new(note)));
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
                        let ts = TimeSignature {
                            beats: num.to_string(),
                            beat_type: den as u8,
                            ..Default::default()
                        };
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
                        state.current_voice.push(VoiceElement::Note(Box::new(note)));
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
                        state.current_voice.push(VoiceElement::Note(Box::new(note)));
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
                                let before = state.current_voice.len();
                                walk_music_block(state, *block);
                                let after = state.current_voice.len();
                                for idx in before..after {
                                    apply_tuplet_to_element(
                                        &mut state.current_voice[idx],
                                        actual,
                                        normal,
                                        idx == before,
                                        idx == after - 1,
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
                        state.current_voice.push(VoiceElement::Note(Box::new(note)));
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
                        state.current_voice.push(VoiceElement::Note(Box::new(note)));
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
                        }
                    }
                }
            }
        }
        "\\unset" | "\\cadenzaOn" | "\\cadenzaOff" | "\\autoBeamOff" | "\\autoBeamOn"
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
            // Slur start: attach to most recent note
            if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
        }
        ")" => {
            // Slur stop: attach to most recent note
            if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
        }
        "~" => {
            // Tie: attach to most recent note
            if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
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
                    "(" | ")" | "~" => {
                        attachments.push(ptext);
                        *i += 1;
                    }
                    _ => break,
                }
            }
            _ => break,
        }
    }
    attachments
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
            | "\\tenuto"
            | "\\accent"
            | "\\marcato"
            | "\\portato"
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
/// (i.e., `\new Staff` or `\new PianoStaff`).
fn block_contains_named_context(_state: &WalkState, block: Node) -> bool {
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "named_context" {
            return true;
        }
    }
    false
}

/// Parse a `\lyricmode { ... }` block into a list of `LyricSyllable`s.
/// Lyrics are symbols separated by `--` (hyphen) or `__` (extend).
fn parse_lyric_block(state: &WalkState, block: Node) -> Vec<LyricSyllable> {
    let mut syllables = Vec::new();
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;
    let mut pending_hyphen = false;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let text = state.text(node).to_string();
                let syllabic = if pending_hyphen {
                    // Check if next is also "--" → middle, else → end
                    let next_is_hyphen = peek_double_hyphen(state, &children, i + 1);
                    if next_is_hyphen {
                        SyllabicType::Middle
                    } else {
                        SyllabicType::End
                    }
                } else {
                    let next_is_hyphen = peek_double_hyphen(state, &children, i + 1);
                    if next_is_hyphen {
                        SyllabicType::Begin
                    } else {
                        SyllabicType::Single
                    }
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

/// Check if position `start` begins a "--" double hyphen.
fn peek_double_hyphen(state: &WalkState, children: &[Node], start: usize) -> bool {
    if let Some(a) = children.get(start) {
        if a.kind() == "punctuation" && state.text(*a) == "-" {
            if let Some(b) = children.get(start + 1) {
                if b.kind() == "punctuation" && state.text(*b) == "-" {
                    return true;
                }
            }
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
                        note.lyrics.push(syllables[syl_idx].clone());
                        syl_idx += 1;
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
            "\\breathe" => {
                note.articulations.push(Articulation {
                    name: "breath-mark".to_string(),
                    placement: Placement::Unspecified,
                });
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
            _ => {}
        }
    }
}

/// Attach a dynamic mark to the most recent note in the current voice.
fn attach_dynamic(state: &mut WalkState, dyn_text: &str) {
    let sign = dyn_text.trim_start_matches('\\').to_string();
    if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
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
        let source = std::fs::read_to_string("example.ly").unwrap();
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
        let source = std::fs::read_to_string("example.ly").unwrap();
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

}

/// Set tuplet duration fields and display markers on a voice element.
fn apply_tuplet_to_element(
    elem: &mut VoiceElement,
    actual: u8,
    normal: u8,
    is_first: bool,
    is_last: bool,
) {
    // Set duration tuplet ratio
    match elem {
        VoiceElement::Note(n) => {
            n.duration.tuplet_actual = actual;
            n.duration.tuplet_normal = normal;
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
            r.duration.tuplet_actual = actual;
            r.duration.tuplet_normal = normal;
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
            c.duration.tuplet_actual = actual;
            c.duration.tuplet_normal = normal;
            if is_first {
                if let Some(first_note) = c.notes.first_mut() {
                    first_note.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Start,
                        bracket: true,
                        show_number: "actual".to_string(),
                    });
                }
            } else if is_last {
                if let Some(first_note) = c.notes.first_mut() {
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
