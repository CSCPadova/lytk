use std::collections::HashMap;

use num::rational::Ratio;
use tree_sitter::Node;

use crate::ir::articulation::{BeamEvent, LyricSyllable};
use crate::ir::duration::{Duration, Frac};
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::measure::Measure;
use crate::ir::note::{ArpeggioType, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{PageLayout, Score, ScoreMetadata};
use crate::ir::voice::Voice;
use crate::ir::Part;

use super::chord_mode::HarmonyEntry;
use super::{
    apply_tuplet_ratio, beam_level_for_duration, distribute_figured_bass, find_relative_octave,
    measure_voice_duration, measures_are_spacer_only, merge_spacer_by_duration,
    merge_spacer_measures, resplit_measures_for_time_sig, resplit_measures_to_match,
    voice_element_duration, VarDef,
};

/// State accumulated while walking tree-sitter nodes.
pub(super) struct WalkState<'src> {
    pub(super) source: &'src str,
    pub(super) language: PitchLanguage,
    pub(super) mode: PitchMode,

    // Current score being built
    pub(super) metadata: ScoreMetadata,
    pub(super) parts: Vec<(String, Part)>, // (context_name, part)
    pub(super) part_counter: u32,

    // Variable definitions: name → either full parts (from \new Staff) or bare measures
    pub(super) definitions: HashMap<String, VarDef>,
    // Lyric variable definitions: name → list of syllables
    pub(super) lyric_definitions: HashMap<String, Vec<LyricSyllable>>,
    // Pending lyrics: voice_name → syllables (from \lyricsto)
    pub(super) pending_lyrics: HashMap<String, Vec<LyricSyllable>>,
    // Chordmode variable definitions: name → harmony entries (from `\chordmode`)
    pub(super) harmony_definitions: HashMap<String, Vec<HarmonyEntry>>,
    // Pending harmonies (from ChordNames contexts) to attach to the melody part
    pub(super) pending_harmonies: Vec<HarmonyEntry>,
    // Voice name → part index mapping (for attaching lyrics)
    pub(super) voice_part_map: HashMap<String, usize>,
    // Per-variable voice maps: var_name → { voice_name → local_part_index }
    pub(super) var_voice_maps: HashMap<String, HashMap<String, usize>>,

    // Measure/voice state for the current part
    pub(super) measure_num: u32,
    pub(super) current_measure: Option<Measure>,
    pub(super) current_voice: Vec<VoiceElement>,

    // Duration state: last explicit duration carries forward
    pub(super) last_duration: Duration,

    // Time-signature–based automatic bar splitting
    /// Duration of one full measure under the current time signature (as fraction of whole note).
    pub(super) current_time_sig: Frac,
    /// Accumulated duration of voice elements in the current measure.
    pub(super) elapsed_in_measure: Frac,
    /// Inside a `\cadenzaOn … \cadenzaOff` (senza misura) region. Measures
    /// flushed while set are flagged `senza_misura`; bars still auto-split as
    /// usual (so they stay aligned with non-cadenza staves) — the cadenza
    /// bridging pass at assembly collapses the flagged run into one free bar.
    pub(super) cadenza_active: bool,
    /// Onset (within the current measure) of the most recently pushed voice
    /// element. LilyPond post-events like `\sustainOn`/`\sustainOff` attach to
    /// the note they follow and occur at that note's onset, not after its
    /// duration — so they read this rather than `elapsed_in_measure`.
    pub(super) last_element_onset: Frac,

    /// Resolved pitches of the most recently parsed chord, for the `q`
    /// chord-repetition shorthand (repeats those pitches with a fresh duration).
    pub(super) last_chord_pitches: Vec<Pitch>,

    // Relative pitch state
    pub(super) prev_pitch: Option<Pitch>,
    pub(super) relative_ref: Option<Pitch>, // The pitch given after \relative
    pub(super) in_relative: bool,

    // Pending overrides
    /// Arpeggio direction set by `\arpeggioArrowUp/Down`, `\arpeggioBracket`.
    pub(super) pending_arpeggio_type: Option<ArpeggioType>,
    /// Glissando line style set by `\once \override Glissando.style = #'...`.
    pub(super) pending_glissando_style: Option<String>,
    /// Whether pending glissando style is "trill" (→ slide instead of glissando).
    pub(super) pending_slide: bool,
    /// Page layout accumulated from `\paper { ... }`.
    pub(super) page_layout: Option<PageLayout>,
    /// Completed scores from previous `\score` blocks (multi-movement).
    pub(super) completed_scores: Vec<Score>,

    // Beam/stem state
    /// Current stem direction override: "up", "down", or "" (auto).
    pub(super) stem_direction: String,
    /// Whether we are inside a manual beam group (after `[`, before `]`).
    pub(super) in_beam_group: bool,
    /// Stack of active tuplet ratios (actual, normal). Innermost is last.
    pub(super) tuplet_stack: Vec<(u8, u8)>,
    /// Whether automatic beaming is disabled (\autoBeamOff).
    pub(super) auto_beam_off: bool,
    /// Whether a manual \melisma block is active (notes inside don't consume lyrics).
    pub(super) melisma_active: bool,
    /// Stack of \transpose intervals (from, to). Pitches are transposed by the
    /// cumulative interval before being stored in the IR.
    pub(super) transpose_stack: Vec<(Pitch, Pitch)>,
    /// Current voice number (set by \voiceOne=1, \voiceTwo=2, etc.; default 1).
    pub(super) current_voice_number: u8,
}

impl<'src> WalkState<'src> {
    pub(super) fn new(source: &'src str) -> Self {
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
            harmony_definitions: HashMap::new(),
            pending_harmonies: Vec::new(),
            voice_part_map: HashMap::new(),
            var_voice_maps: HashMap::new(),
            measure_num: 0,
            current_measure: None,
            current_voice: Vec::new(),
            last_duration: Duration::quarter(),
            current_time_sig: Frac::new(4, 4), // default 4/4 = 1 whole note
            elapsed_in_measure: Frac::from_integer(0),
            cadenza_active: false,
            last_element_onset: Frac::from_integer(0),
            last_chord_pitches: Vec::new(),
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
            melisma_active: false,
            transpose_stack: Vec::new(),
            current_voice_number: 1,
        }
    }

    /// Get the text content of a node.
    pub(super) fn text(&self, node: Node) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// Flush current voice elements into the current measure.
    pub(super) fn flush_voice(&mut self) {
        if self.current_voice.is_empty() {
            return;
        }
        let voice = Voice {
            number: self.current_voice_number,
            elements: std::mem::take(&mut self.current_voice),
        };
        let measure = self.ensure_measure();
        measure.voices.push(voice);
    }

    /// Ensure there's a current measure, creating one if needed.
    pub(super) fn ensure_measure(&mut self) -> &mut Measure {
        if self.current_measure.is_none() {
            self.measure_num += 1;
            self.current_measure = Some(Measure::new(self.measure_num));
        }
        self.current_measure.as_mut().unwrap()
    }

    /// Flush the current measure into the current part.
    pub(super) fn flush_measure(&mut self) {
        self.flush_voice();
        if let Some(mut measure) = self.current_measure.take() {
            if self.cadenza_active {
                measure.senza_misura = true;
            }
            self.ensure_part().measures.push(measure);
        }
    }

    /// Start a new measure (bar check encountered).
    pub(super) fn bar_check(&mut self) {
        self.flush_measure();
        self.elapsed_in_measure = Frac::from_integer(0);
    }

    /// Push a voice element and auto-split the measure if it's full.
    pub(super) fn push_voice_element(&mut self, mut elem: VoiceElement) {
        // Apply active tuplet ratio to the element's duration. For nested
        // tuplets the effective scaling is the product of every enclosing
        // ratio, not just the innermost — so fold the whole stack.
        if !self.tuplet_stack.is_empty() {
            let (mut actual, mut normal): (u32, u32) = (1, 1);
            for &(a, n) in &self.tuplet_stack {
                actual = actual.saturating_mul(a as u32);
                normal = normal.saturating_mul(n as u32);
            }
            apply_tuplet_ratio(&mut elem, actual.min(255) as u8, normal.min(255) as u8);
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
            self.elapsed_in_measure -= self.current_time_sig;
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

        // Mark notes inside a \melisma ... \melismaEnd block
        if self.melisma_active {
            if let VoiceElement::Note(n) = &mut elem {
                n.in_melisma = true
            }
        }

        // Grace notes don't consume time in the measure
        let is_grace = matches!(&elem, VoiceElement::Note(n) if n.is_grace);
        // Record this element's onset so a following post-event (pedal, etc.)
        // attaches at the note rather than after its duration.
        self.last_element_onset = self.elapsed_in_measure;
        self.current_voice.push(elem);
        if !is_grace {
            self.elapsed_in_measure += dur;
        }
    }

    /// Update the current time signature (called when \time is parsed).
    pub(super) fn set_time_signature(&mut self, beats: u32, beat_type: u32) {
        self.current_time_sig = Frac::new(beats as i64, beat_type as i64);
    }

    /// Get or create the current part.
    pub(super) fn ensure_part(&mut self) -> &mut Part {
        if self.parts.is_empty() {
            self.part_counter += 1;
            let id = format!("P{}", self.part_counter);
            let part = Part::new(&id);
            self.parts.push(("Staff".to_string(), part));
        }
        &mut self.parts.last_mut().unwrap().1
    }

    /// Start a new part for a named context.
    pub(super) fn new_part(&mut self, context: &str, name: &str) {
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

    /// Total musical duration of a (music) variable, summed across its
    /// pre-parsed measures. Used for the Scheme `#(skip-of-length VAR)` idiom,
    /// which emits a spacer the same length as `VAR` to align a parallel voice.
    pub(super) fn variable_total_duration(&self, name: &str) -> Option<Frac> {
        match self.definitions.get(name)? {
            VarDef::Measures(measures, _) => Some(
                measures
                    .iter()
                    .map(measure_voice_duration)
                    .fold(Frac::from_integer(0), |a, b| a + b),
            ),
            _ => None,
        }
    }

    /// Resolve a variable reference: look up stored measures and add them
    /// to the current part.
    pub(super) fn resolve_variable(&mut self, name: &str) -> bool {
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
                VarDef::Measures(measures, def_time_sig) => {
                    if measures.is_empty() {
                        return true; // empty variable, no-op
                    }
                    // Save the external time sig (before variable's own time sigs)
                    // for the resplit check below.
                    let external_time_sig = self.current_time_sig;
                    // Propagate time signature from the resolved measures.
                    // If any measure in the variable has a time signature attribute,
                    // update current_time_sig so subsequent variable resolutions
                    // use the correct time sig (e.g. \global sets \time 6/8,
                    // then \lowerStaff needs to be re-split to 6/8).
                    let mut has_own_time_sigs = false;
                    for m in &measures {
                        if let Some(ref attrs) = m.attributes {
                            if let Some(ref ts) = attrs.time {
                                self.current_time_sig = ts.beats_fraction();
                                has_own_time_sigs = true;
                            }
                        }
                    }
                    // If the variable was pre-parsed with a different time signature
                    // than the external one, re-split the measures to match.
                    // But skip resplit if the variable has its own time sig changes
                    // (it already knows its own measure boundaries).
                    let measures = if !has_own_time_sigs
                        && def_time_sig != external_time_sig
                        && external_time_sig > Frac::from_integer(0)
                        && !measures_are_spacer_only(&measures)
                    {
                        resplit_measures_for_time_sig(&measures, external_time_sig)
                    } else {
                        measures
                    };
                    // Flush any in-progress measure before adding pre-split measures
                    self.flush_measure();
                    self.elapsed_in_measure = Frac::from_integer(0);
                    // If this variable had voice name mappings, apply them to the current part
                    {
                        let _ = self.ensure_part(); // ensure part exists
                        let part_idx = self.parts.len() - 1;
                        if let Some(voice_map) = self.var_voice_maps.get(name) {
                            for voice_name in voice_map.keys() {
                                self.voice_part_map.insert(voice_name.clone(), part_idx);
                            }
                        }
                    }
                    let part = self.ensure_part();
                    // If the part already has measures and the incoming measures
                    // contain only spacer rests (e.g. from a \forma variable in
                    // parallel music), merge attributes into existing measures
                    // rather than appending.
                    if !part.measures.is_empty() && measures_are_spacer_only(&measures) {
                        // Check if this is parallel spacer (should merge) or
                        // sequential spacer (should append). Compare total durations:
                        // parallel spacer spans the same time as the real music;
                        // sequential spacer is much shorter.
                        let existing_dur: Frac = part
                            .measures
                            .iter()
                            .map(measure_voice_duration)
                            .fold(Frac::from_integer(0), |a, b| a + b);
                        let incoming_dur: Frac = measures
                            .iter()
                            .map(measure_voice_duration)
                            .fold(Frac::from_integer(0), |a, b| a + b);

                        if measures_are_spacer_only(&part.measures) {
                            // Both existing and incoming are spacer-only: they're
                            // sequential (e.g. \barRest | \barRest), not parallel.
                            // Just append.
                            part.measures.extend(measures);
                        } else if incoming_dur > Frac::from_integer(0)
                            && incoming_dur * Frac::from_integer(2) < existing_dur
                            && !has_own_time_sigs
                        {
                            // Incoming spacer is much shorter than existing music
                            // (less than half the duration) — this is sequential content
                            // (e.g. \barRest after notes), not parallel spacer.
                            // Just append.
                            part.measures.extend(measures);
                        } else if measures.len() != part.measures.len() {
                            if has_own_time_sigs {
                                // Spacer has authoritative time signatures (like \forma).
                                // Resplit the existing real music to match the spacer's
                                // measure boundaries, then copy over spacer attributes.
                                part.measures =
                                    resplit_measures_to_match(&part.measures.clone(), &measures);
                            } else {
                                // No authoritative time sig — merge directions by
                                // cumulative duration, preserving note measure boundaries.
                                merge_spacer_by_duration(&mut part.measures, &measures);
                            }
                        } else {
                            // Counts match — index-based merge is safe
                            merge_spacer_measures(&mut part.measures, &measures);
                        }
                    } else if part.measures.is_empty() || !measures_are_spacer_only(&part.measures)
                    {
                        // Before extending, check if the last existing measure is
                        // attribute-only (e.g. from a \key between sections). If so,
                        // merge its attributes onto the first incoming measure.
                        let mut measures = measures;
                        if !part.measures.is_empty() && !measures.is_empty() {
                            let last_dur = measure_voice_duration(part.measures.last().unwrap());
                            if last_dur == Frac::from_integer(0) {
                                let attr_m = part.measures.pop().unwrap();
                                if let Some(ref a) = attr_m.attributes {
                                    let fa = measures[0].attributes.get_or_insert_with(
                                        crate::ir::measure::MeasureAttributes::default,
                                    );
                                    if a.key.is_some() && fa.key.is_none() {
                                        fa.key = a.key;
                                    }
                                    if a.time.is_some() && fa.time.is_none() {
                                        fa.time = a.time.clone();
                                    }
                                    if !a.clefs.is_empty() && fa.clefs.is_empty() {
                                        fa.clefs = a.clefs.clone();
                                    }
                                }
                                if !attr_m.directions.is_empty() {
                                    measures[0]
                                        .directions
                                        .extend(attr_m.directions.iter().cloned());
                                }
                            }
                        }
                        part.measures.extend(measures);
                    } else {
                        // Existing measures are spacer-only, incoming are real music.
                        // Check if existing measures are truly spacer (have voice duration)
                        // or just attribute-only (zero voice duration from \time \key \clef).
                        let existing_total_dur: Frac = part
                            .measures
                            .iter()
                            .map(measure_voice_duration)
                            .fold(Frac::from_integer(0), |a, b| a + b);
                        if existing_total_dur == Frac::from_integer(0) {
                            // Attribute-only measures (no real spacer content) —
                            // merge their attributes onto the first incoming measure,
                            // then replace with incoming.
                            let mut incoming = measures;
                            if !incoming.is_empty() {
                                for attr_m in &part.measures {
                                    if let Some(ref a) = attr_m.attributes {
                                        let fa = incoming[0].attributes.get_or_insert_with(
                                            crate::ir::measure::MeasureAttributes::default,
                                        );
                                        if a.key.is_some() && fa.key.is_none() {
                                            fa.key = a.key;
                                        }
                                        if a.time.is_some() && fa.time.is_none() {
                                            fa.time = a.time.clone();
                                        }
                                        if !a.clefs.is_empty() && fa.clefs.is_empty() {
                                            fa.clefs = a.clefs.clone();
                                        }
                                    }
                                    // Also transfer any directions
                                    if !attr_m.directions.is_empty() {
                                        incoming[0]
                                            .directions
                                            .extend(attr_m.directions.iter().cloned());
                                    }
                                }
                            }
                            part.measures = incoming;
                        } else {
                            // Real spacer measures (have voice duration from \skip etc.)
                            let incoming_multi_voice = measures.iter().any(|m| m.voices.len() > 1);
                            if measures.len() != part.measures.len() && !incoming_multi_voice {
                                part.measures =
                                    resplit_measures_to_match(&measures, &part.measures);
                            } else if measures.len() == part.measures.len() {
                                // Counts match — index-based merge is safe
                                let mut incoming = measures;
                                merge_spacer_measures(&mut incoming, &part.measures);
                                part.measures = incoming;
                            } else {
                                // Counts don't match (multi-voice prevents resplit) —
                                // use duration-aware merge so barlines/directions align
                                // by time position rather than by index.
                                let mut incoming = measures;
                                merge_spacer_by_duration(&mut incoming, &part.measures);
                                part.measures = incoming;
                            }
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
    pub(super) fn resolve_pitch(
        &mut self,
        step: PitchStep,
        alter: Ratio<i32>,
        octave_marks: i32,
    ) -> Pitch {
        let pitch = if self.in_relative {
            if let Some(ref prev) = self.prev_pitch {
                // In relative mode: find closest pitch within a fourth, then apply marks
                let inferred_octave = find_relative_octave(prev, step);
                let octave = inferred_octave + octave_marks;
                let p = Pitch::with_alter(step, alter, octave);
                self.prev_pitch = Some(p);
                p
            } else {
                // First note after \relative: use the reference pitch's octave
                let base_oct = self.relative_ref.as_ref().map(|r| r.octave).unwrap_or(4);
                let octave = base_oct + octave_marks;
                let p = Pitch::with_alter(step, alter, octave);
                self.prev_pitch = Some(p);
                p
            }
        } else {
            // Absolute mode: octave marks relative to LilyPond c (octave 3 in our numbering)
            let octave = 3 + octave_marks;
            Pitch::with_alter(step, alter, octave)
        };
        // Apply any active \transpose intervals
        if !self.transpose_stack.is_empty() {
            let total_semitones: i32 = self
                .transpose_stack
                .iter()
                .map(|(from, to)| to.midi_number() - from.midi_number())
                .sum();
            pitch.transposed(total_semitones)
        } else {
            pitch
        }
    }
}
