use std::collections::HashMap;

use num::rational::Ratio;
use tree_sitter::Node;

use crate::ir::articulation::{BeamEvent, LyricSyllable};
use crate::ir::duration::{Duration, Frac};
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::note::{ArpeggioType, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{PageLayout, Score, ScoreMetadata};
use crate::ir::Part;

use super::chord_mode::HarmonyEntry;
use super::timeline::{Event, Timeline};
use super::{apply_tuplet_ratio, beam_level_for_duration, find_relative_octave, FiguredBassEntry};

/// A part under construction: its metadata, and its music as a [`Timeline`]
/// (measures are made only when the score is assembled).
#[derive(Clone)]
pub(super) struct PartBuild {
    /// LilyPond context that created it (`Staff`, `Dynamics`, …).
    pub(super) context: String,
    /// Name, instrument and staff count; `measures` stays empty until assembly.
    pub(super) part: Part,
    pub(super) tl: Timeline,
    /// Stable identity: voice names and `\addlyrics` point here, and survive
    /// the part being folded into a PianoStaff.
    pub(super) uid: u32,
}

/// What a variable definition expands to.
#[derive(Clone)]
pub(super) enum VarDef {
    /// Variable contained `\new Staff { ... }` — the parts it built, and the
    /// voice names defined inside (name → index into the parts).
    Parts(Vec<PartBuild>, HashMap<String, usize>),
    /// Bare music, positioned from 0 and `len` long. `main_lane` is the lane its
    /// music was written in; `voices` are `\new Voice = "…"` names inside it.
    Music {
        tl: Timeline,
        len: Frac,
        main_lane: u8,
        voices: Vec<String>,
    },
    /// Variable contained `\figuremode { ... }` — stores flat stream of entries.
    FiguredBass(Vec<FiguredBassEntry>),
}

/// State accumulated while walking tree-sitter nodes.
pub(super) struct WalkState<'src> {
    pub(super) source: &'src str,
    pub(super) language: PitchLanguage,
    pub(super) mode: PitchMode,

    // Current score being built
    pub(super) metadata: ScoreMetadata,
    pub(super) parts: Vec<PartBuild>,
    pub(super) part_counter: u32,
    next_uid: u32,
    /// Parts folded into another (PianoStaff staves): uid → the uid they joined.
    pub(super) part_alias: HashMap<u32, u32>,

    // Variable definitions: name → either full parts (from \new Staff) or bare music
    pub(super) definitions: HashMap<String, VarDef>,
    // Lyric variable definitions: name → list of syllables
    pub(super) lyric_definitions: HashMap<String, Vec<LyricSyllable>>,
    // Pending lyrics: voice_name → syllables (from \lyricsto)
    pub(super) pending_lyrics: HashMap<String, Vec<LyricSyllable>>,
    /// `MUSIC \addlyrics { … }`: (part uid, syllables), attached at assembly.
    pub(super) added_lyrics: Vec<(u32, Vec<LyricSyllable>)>,
    // Chordmode variable definitions: name → harmony entries (from `\chordmode`)
    pub(super) harmony_definitions: HashMap<String, Vec<HarmonyEntry>>,
    // Pending harmonies (from ChordNames contexts) to attach to the melody part
    pub(super) pending_harmonies: Vec<HarmonyEntry>,
    /// Voice name → uid of the part holding it (for attaching lyrics).
    pub(super) voice_part_map: HashMap<String, u32>,

    // Position state
    /// Where the next element goes.
    pub(super) pos: Frac,
    /// Where the buffered run in `current_voice` starts.
    pub(super) voice_start: Frac,
    /// Where a context opened now starts: the start of the enclosing `<< >>`.
    pub(super) origin: Frac,
    /// Elements not yet placed in the part's timeline. Post-events (`(`, `~`,
    /// dynamics…) attach to its last element, so it is only flushed at
    /// structural points.
    pub(super) current_voice: Vec<VoiceElement>,

    // Duration state: last explicit duration carries forward
    pub(super) last_duration: Duration,

    /// Onset of the most recently pushed voice element. LilyPond post-events
    /// like `\sustainOn`/`\sustainOff` attach to the note they follow and occur
    /// at that note's onset, not after its duration.
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
    /// Current voice number (set by \voiceOne=1, \voiceTwo=2, etc.; default 1):
    /// the lane new music goes to.
    pub(super) current_voice_number: u8,
    /// The context just read was `\context X` rather than `\new X`: it
    /// re-enters an existing X instead of creating one.
    pub(super) context_reentry: std::cell::Cell<bool>,
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
            next_uid: 0,
            part_alias: HashMap::new(),
            definitions: HashMap::new(),
            lyric_definitions: HashMap::new(),
            pending_lyrics: HashMap::new(),
            added_lyrics: Vec::new(),
            harmony_definitions: HashMap::new(),
            pending_harmonies: Vec::new(),
            voice_part_map: HashMap::new(),
            pos: Frac::from_integer(0),
            voice_start: Frac::from_integer(0),
            origin: Frac::from_integer(0),
            current_voice: Vec::new(),
            last_duration: Duration::quarter(),
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
            context_reentry: std::cell::Cell::new(false),
        }
    }

    /// Get the text content of a node.
    pub(super) fn text(&self, node: Node) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// Place the buffered run in the current part's timeline.
    pub(super) fn flush_voice(&mut self) {
        if !self.current_voice.is_empty() {
            let run = std::mem::take(&mut self.current_voice);
            let (lane, start) = (self.current_voice_number, self.voice_start);
            self.ensure_build().tl.place_run(lane, start, run);
        }
        self.voice_start = self.pos;
    }

    /// Move the write position (e.g. back to the start of a `<< >>` branch).
    pub(super) fn set_pos(&mut self, pos: Frac) {
        self.flush_voice();
        self.pos = pos;
        self.voice_start = pos;
    }

    /// Record an event at the current position.
    pub(super) fn add_event(&mut self, ev: Event) {
        let pos = self.pos;
        self.add_event_at(pos, ev);
    }

    pub(super) fn add_event_at(&mut self, pos: Frac, ev: Event) {
        self.ensure_build().tl.add(pos, ev);
    }

    /// Push a voice element at the current position.
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

        // Record this element's onset so a following post-event (pedal, etc.)
        // attaches at the note rather than after its duration.
        self.last_element_onset = self.pos;
        // Grace notes take no time.
        self.pos += elem.metric_duration();
        self.current_voice.push(elem);
    }

    /// The current part, created if there is none yet.
    pub(super) fn ensure_build(&mut self) -> &mut PartBuild {
        if self.parts.is_empty() {
            self.push_part("Staff", Part::new(""));
        }
        self.parts.last_mut().unwrap()
    }

    /// The current part's metadata, created if there is none yet.
    pub(super) fn ensure_part(&mut self) -> &mut Part {
        &mut self.ensure_build().part
    }

    fn push_part(&mut self, context: &str, mut part: Part) {
        self.part_counter += 1;
        part.part_id = format!("P{}", self.part_counter);
        self.next_uid += 1;
        self.parts.push(PartBuild {
            context: context.to_string(),
            part,
            tl: Timeline::default(),
            uid: self.next_uid,
        });
    }

    /// Fresh identity for a part copied in from a variable.
    pub(super) fn fresh_uid(&mut self) -> u32 {
        self.next_uid += 1;
        self.next_uid
    }

    /// Start a new part for a named context, at the current context origin.
    pub(super) fn new_part(&mut self, context: &str, name: &str) {
        self.flush_voice();
        let mut part = Part::new("");
        if !name.is_empty() {
            part.name = name.to_string();
        }
        self.push_part(context, part);
        let origin = self.origin;
        self.pos = origin;
        self.voice_start = origin;
        self.prev_pitch = self.relative_ref;
    }

    /// uid of the current part (creating it if needed).
    pub(super) fn current_uid(&mut self) -> u32 {
        self.ensure_build().uid
    }

    /// Follow PianoStaff folds to the part a uid now lives in.
    pub(super) fn resolve_uid(&self, mut uid: u32) -> u32 {
        while let Some(&to) = self.part_alias.get(&uid) {
            uid = to;
        }
        uid
    }

    /// Total musical duration of a (music) variable. Used for the Scheme
    /// `#(skip-of-length VAR)` idiom, which emits a spacer the same length as
    /// `VAR` to align a parallel voice.
    pub(super) fn variable_total_duration(&self, name: &str) -> Option<Frac> {
        match self.definitions.get(name)? {
            VarDef::Music { len, .. } => Some(*len),
            _ => None,
        }
    }

    /// Resolve a variable reference at the current position.
    pub(super) fn resolve_variable(&mut self, name: &str) -> bool {
        // Positioned music is spliced straight from the definition, without
        // copying it first.
        if let Some(VarDef::Music { len, main_lane, .. }) = self.definitions.get(name) {
            let (len, main_lane) = (*len, *main_lane);
            self.flush_voice();
            let (at, lane) = (self.pos, self.current_voice_number);
            let uid = self.current_uid();
            let Some(VarDef::Music { tl, voices, .. }) = self.definitions.get(name) else {
                unreachable!()
            };
            let part = self.parts.last_mut().expect("current_uid made a part");
            part.tl.splice(tl, at, main_lane, lane);
            for voice in voices {
                self.voice_part_map.insert(voice.clone(), uid);
            }
            self.pos = at + len;
            self.voice_start = self.pos;
            return true;
        }
        let Some(def) = self.definitions.get(name).cloned() else {
            return false;
        };
        match def {
            VarDef::Parts(parts, voices) => {
                self.flush_voice();
                let at = self.pos;
                let mut uids = Vec::new();
                for mut pb in parts {
                    pb.uid = self.fresh_uid();
                    pb.tl = std::mem::take(&mut pb.tl).shifted(at);
                    self.part_counter += 1;
                    pb.part.part_id = format!("P{}", self.part_counter);
                    uids.push(pb.uid);
                    self.parts.push(pb);
                }
                for (voice, idx) in voices {
                    if let Some(&uid) = uids.get(idx) {
                        self.voice_part_map.insert(voice, uid);
                    }
                }
            }
            VarDef::Music { .. } => unreachable!("handled above"),
            VarDef::FiguredBass(entries) => {
                let mut at = self.pos;
                for entry in entries {
                    match entry {
                        FiguredBassEntry::Figure(fb) => {
                            let d = fb.duration.actual_duration();
                            self.add_event_at(at, Event::FiguredBass(fb));
                            at += d;
                        }
                        FiguredBassEntry::Skip(dur) => at += dur.actual_duration(),
                    }
                }
            }
        }
        true
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
