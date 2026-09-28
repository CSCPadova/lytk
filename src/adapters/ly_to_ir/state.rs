use std::collections::{HashMap, HashSet};

use num::rational::Ratio;
use num::CheckedAdd;
use tree_sitter::Node;

use crate::diagnostics::{Columns, Diagnostic, Severity};
use crate::ir::articulation::{BeamEvent, LyricSyllable};
use crate::ir::duration::{Duration, Frac};
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::note::{ArpeggioType, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{PageLayout, Score, ScoreMetadata};
use crate::ir::Part;

use super::chord_mode::HarmonyEntry;
use super::{apply_tuplet_ratio, beam_level_for_duration, find_relative_octave, FiguredBassEntry};
use crate::ir::timeline::{Event, Timeline};

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
        /// Byte range of the `{ … }` block that defines it, so a reference
        /// inside `\relative` can read it again there (see `resolve_variable`),
        /// in the pitch language of its definition.
        block: Option<(usize, usize, PitchLanguage)>,
    },
    /// Variable contained `\figuremode { ... }` — stores flat stream of entries.
    FiguredBass(Vec<FiguredBassEntry>),
}

/// Most voice elements (notes, rests, chords) one reading may generate, and
/// the furthest a voice may reach, in whole notes. Real scores stay orders of
/// magnitude below; the bounds keep `s1*4000000000`, nested `\repeat unfold`
/// and self-doubling variables from hanging or exhausting memory. Past either
/// one the reading stops and fails.
pub(super) const MAX_ELEMENTS: u64 = 500_000;
pub(super) const MAX_WHOLE_NOTES: i64 = 100_000;
/// Octaves a pitch may lie in (middle C is octave 4; MIDI reaches -1..=9).
/// Far wider than music: LilyPond's own regression tests climb to octave 22
/// with repeated relative scales. Emitters write one mark per octave, so the
/// bound caps each note's output at about 127 marks.
pub(super) const PITCH_OCTAVES: std::ops::RangeInclusive<i32> = -128..=127;
/// LilyPond's language files (its `ly/` directory), and the language each
/// sets: `arabic.ly` uses Italian note names, with accidentals of its own.
const LANGUAGE_FILES: [(&str, PitchLanguage); 12] = [
    ("arabic", PitchLanguage::Italiano),
    ("catalan", PitchLanguage::Catalan),
    ("deutsch", PitchLanguage::Deutsch),
    ("english", PitchLanguage::English),
    ("espanol", PitchLanguage::Espanol),
    ("italiano", PitchLanguage::Italiano),
    ("nederlands", PitchLanguage::Nederlands),
    ("norsk", PitchLanguage::Norsk),
    ("portugues", PitchLanguage::Portugues),
    ("suomi", PitchLanguage::Suomi),
    ("svenska", PitchLanguage::Svenska),
    ("vlaams", PitchLanguage::Vlaams),
];
/// LilyPond's files of pitch names lytk cannot read (with quarter and
/// smaller tones of their own).
const OTHER_PITCH_NAMES: [&str; 5] = ["bagpipe", "hel-arabic", "makam", "persian", "turkish-makam"];

/// Deepest the walk may recurse: music blocks inside music blocks, counting
/// the variables read again inside `\relative`, which the syntax tree's own
/// depth bound does not see.
pub(super) const MAX_WALK_DEPTH: u32 = crate::parser::MAX_NESTING_DEPTH as u32;

/// State accumulated while walking tree-sitter nodes.
pub(super) struct WalkState<'src> {
    pub(super) source: &'src str,
    /// Root of the syntax tree being walked (variables are read again from it).
    pub(super) root: Option<Node<'src>>,
    /// Nesting of variables being read again, against self-reference.
    pub(super) var_depth: u8,
    /// Voice elements generated so far, against [`MAX_ELEMENTS`].
    pub(super) generated: u64,
    /// Why the reading stopped, once it went past a bound.
    pub(super) over_limit: Option<String>,
    /// Music blocks being walked, one inside the other (see [`MAX_WALK_DEPTH`]).
    pub(super) walk_depth: u32,
    /// Variables being read again inside `\relative`, innermost last.
    pub(super) rewalking: Vec<String>,
    /// Nesting of `\relative { … }` blocks being walked. Unlike `in_relative`
    /// (which can stay set after a `\relative` for the music that follows),
    /// this is exact: it is what decides whether a variable is read again.
    pub(super) relative_depth: u32,
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

    /// What the walk found wrong, or did not read.
    pub(super) diagnostics: Vec<Diagnostic>,
    columns: Columns,
    /// Above 0 while music already walked once is walked for another purpose
    /// (a chord-mode block read as notes): the first walk reported it.
    pub(super) quiet: u32,
    /// The text read was flattened with include paths: an `\include` left
    /// in it names a file not found.
    pub(super) follows_includes: bool,
    /// Names assigned at the top level (`name = …`), whatever their value.
    pub(super) assigned: HashSet<String>,
    /// The top-level music expression being read, if any: LilyPond makes a
    /// score of each, so it is a movement of its own.
    pub(super) open_movement: Option<Node<'src>>,
    /// Octaves `\fixed` adds to absolute pitches (`\fixed c' { c }` is c').
    pub(super) fixed_octaves: i32,
    /// The `\score` blocks after the first, which a single-score reading drops.
    pub(super) later_movements: Vec<Diagnostic>,
    /// Fields of the top-level `\header` blocks, in order: they apply to
    /// every movement that does not set them itself.
    pub(super) book_header: Vec<(String, String)>,
    /// Above 0 inside `\drummode` music: words are drum names.
    pub(super) drum_mode: u32,
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
            root: None,
            var_depth: 0,
            generated: 0,
            over_limit: None,
            walk_depth: 0,
            rewalking: Vec::new(),
            relative_depth: 0,
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
            diagnostics: Vec::new(),
            columns: Columns::default(),
            quiet: 0,
            follows_includes: false,
            assigned: HashSet::new(),
            open_movement: None,
            fixed_octaves: 0,
            later_movements: Vec::new(),
            book_header: Vec::new(),
            drum_mode: 0,
        }
    }

    /// Get the text content of a node.
    pub(super) fn text(&self, node: Node) -> &'src str {
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

    /// Report a finding about `node`.
    pub(super) fn report(
        &mut self,
        node: Node,
        severity: Severity,
        code: &'static str,
        message: String,
    ) {
        if self.quiet == 0 {
            let (source, end) = (self.source, node.end_byte());
            let d = Diagnostic::counted(
                &mut self.columns,
                source,
                node,
                end,
                severity,
                code,
                message,
            );
            self.diagnostics.push(d);
        }
    }

    pub(super) fn warn(&mut self, node: Node, code: &'static str, message: String) {
        self.report(node, Severity::Warning, code, message);
    }

    pub(super) fn error(&mut self, node: Node, code: &'static str, message: String) {
        self.report(node, Severity::Error, code, message);
    }

    /// `\language "name"` (`node` is the string): the pitch names from here
    /// on. An unknown name keeps the current language, and warns.
    pub(super) fn set_language(&mut self, node: Node) {
        let name = super::text::string_value(self.source, node);
        match PitchLanguage::from_str_loose(&name) {
            Some(lang) => self.language = lang,
            None => {
                let current = self.language.as_str();
                self.warn(
                    node,
                    "unknown-language",
                    format!("unknown pitch language `{name}`: {current} stays"),
                );
            }
        }
    }

    /// `\include` (at `node`): LilyPond's language files set the language as
    /// `\language` does; any other file is not followed.
    fn include(&mut self, node: Node) {
        let file = node.next_sibling().filter(|n| n.kind() == "string");
        let path = file.map(|n| super::text::string_value(self.source, n));
        let stem = path
            .as_deref()
            .and_then(|p| p.rsplit(['/', '\\']).next())
            .and_then(|f| f.strip_suffix(".ly"));
        if let Some(&(_, lang)) = LANGUAGE_FILES.iter().find(|(f, _)| Some(*f) == stem) {
            self.language = lang;
        } else if let Some(stem) = stem.filter(|s| OTHER_PITCH_NAMES.contains(s)) {
            self.warn(
                node,
                "unknown-language",
                format!("the pitch names of `{stem}.ly` are not read"),
            );
        } else {
            let shown = file
                .map(|n| format!(" {}", self.text(n)))
                .unwrap_or_default();
            let why = if path
                .as_deref()
                .is_some_and(|p| super::builtins::LILYPOND_FILES.binary_search(&p).is_ok())
            {
                "is one of LilyPond's own files, which lytk does not read"
            } else if self.follows_includes {
                "is not followed: no such file in the include paths"
            } else {
                "is not followed: flatten the file first, or give include paths"
            };
            self.warn(node, "ignored-include", format!("`\\include{shown}` {why}"));
        }
    }

    /// A command at `node` (`\name`) that the walk does not read: warn,
    /// unless LilyPond or the file defines it.
    pub(super) fn unread_command(&mut self, node: Node, name: &str) {
        if name == "include" {
            self.include(node);
        } else if !self.assigned.contains(name)
            && super::builtins::BUILTINS.binary_search(&name).is_err()
        {
            self.warn(
                node,
                "unknown-command",
                format!("unknown command `\\{name}`"),
            );
        }
    }

    /// Stop the reading: it went past a bound. The first reason is kept.
    pub(super) fn refuse(&mut self, reason: String) {
        self.over_limit.get_or_insert(reason);
    }

    /// Whether the reading stopped at a bound; walkers return early then.
    pub(super) fn stopped(&self) -> bool {
        self.over_limit.is_some()
    }

    /// Enter one more nested music block: false (and the reading stops)
    /// past [`MAX_WALK_DEPTH`]. The caller decrements `walk_depth` on exit.
    pub(super) fn enter_block(&mut self) -> bool {
        if self.walk_depth >= MAX_WALK_DEPTH {
            self.refuse(format!(
                "the music nests deeper than {MAX_WALK_DEPTH} levels"
            ));
            return false;
        }
        self.walk_depth += 1;
        true
    }

    /// Where music of length `len` placed at `at` ends, if within the bounds
    /// (the reading stops otherwise).
    pub(super) fn end_within_bounds(&mut self, at: Frac, len: Frac) -> Option<Frac> {
        match at.checked_add(&len) {
            Some(end) if end <= Frac::from_integer(MAX_WHOLE_NOTES) => Some(end),
            Some(_) => {
                self.refuse(format!(
                    "the music is longer than {MAX_WHOLE_NOTES} whole notes"
                ));
                None
            }
            None => {
                self.refuse(
                    "note positions overflow: the durations' denominators are too large"
                        .to_string(),
                );
                None
            }
        }
    }

    /// Count `n` more generated elements: false once past [`MAX_ELEMENTS`].
    pub(super) fn spend(&mut self, n: u64) -> bool {
        if self.stopped() {
            return false;
        }
        self.generated = self.generated.saturating_add(n);
        if self.generated > MAX_ELEMENTS {
            self.refuse(format!(
                "the music expands to more than {MAX_ELEMENTS} notes, rests and chords"
            ));
            return false;
        }
        true
    }

    /// Push a voice element at the current position.
    pub(super) fn push_voice_element(&mut self, mut elem: VoiceElement) {
        if !self.spend(1) {
            return;
        }
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
        let Some(end) = self.end_within_bounds(self.pos, elem.metric_duration()) else {
            return;
        };
        self.pos = end;
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

    /// In drum mode, the pitch whose MIDI key sounds drum `name` (`bd`,
    /// `snare`, …), as MusicXML keeps unpitched notes on a percussion staff.
    pub(super) fn drum_pitch(&self, name: &str) -> Option<Pitch> {
        if self.drum_mode == 0 {
            return None;
        }
        let drums = super::drums::DRUMS;
        let &(_, octave, step, alter) = drums
            .binary_search_by(|(n, ..)| n.cmp(&name))
            .ok()
            .map(|k| &drums[k])?;
        let step = [
            PitchStep::C,
            PitchStep::D,
            PitchStep::E,
            PitchStep::F,
            PitchStep::G,
            PitchStep::A,
            PitchStep::B,
        ][step as usize];
        Some(Pitch::with_alter(step, Ratio::from_integer(alter), octave))
    }

    /// Figured bass from the current position on (a `\figuremode` block or
    /// variable): events, taking no time in the voice.
    pub(super) fn place_figures(&mut self, entries: Vec<FiguredBassEntry>) {
        if !self.spend(entries.len() as u64) {
            return;
        }
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

    /// Resolve a variable reference at the current position.
    pub(super) fn resolve_variable(&mut self, name: &str) -> bool {
        // `\relative` applies to a variable's music where it is used: LilyPond
        // substitutes the variable, then makes the pitches relative. A block
        // written with plain pitches (`cadenza = { fis2 … }` used in
        // `\relative c'' { \cadenza }`) is therefore read again here, in the
        // relative context, rather than spliced as read at its definition.
        // A variable being read again is not read again inside itself (`a =
        // { \a \a }` refers to the previous `a`): its captured music is used.
        if self.relative_depth > 0
            && self.var_depth < 16
            && !self.rewalking.iter().any(|n| n == name)
        {
            if let Some(VarDef::Music {
                block: Some((start, end, language)),
                ..
            }) = self.definitions.get(name)
            {
                let (start, end, language) = (*start, *end, *language);
                let node = self
                    .root
                    .and_then(|r| r.descendant_for_byte_range(start, end));
                let node = std::iter::successors(node, |n| n.parent()).find(|n| {
                    n.kind() == "expression_block" && (n.start_byte(), n.end_byte()) == (start, end)
                });
                // A block that names its own variable (`a = { \a \a }`) meant
                // the previous `a`, which is gone: use the music captured at
                // the definition instead of reading the block again.
                if let Some(block) = node.filter(|b| !mentions(*b, self.source, name)) {
                    self.var_depth += 1;
                    self.rewalking.push(name.to_string());
                    let outer = std::mem::replace(&mut self.language, language);
                    super::music::walk_music_block(self, block);
                    self.language = outer;
                    self.rewalking.pop();
                    self.var_depth -= 1;
                    return true;
                }
            }
        }
        // Positioned music is spliced straight from the definition, without
        // copying it first.
        // Every copy counts against the budget: definitions that double the
        // previous one (`b = { \a \a }`, `c = { \b \b }`, …) grow as 2^n.
        if let Some(VarDef::Music {
            len, main_lane, tl, ..
        }) = self.definitions.get(name)
        {
            let (len, main_lane, count) = (*len, *main_lane, tl.element_count());
            if !self.spend(count as u64) {
                return true;
            }
            self.flush_voice();
            let (at, lane) = (self.pos, self.current_voice_number);
            let Some(end) = self.end_within_bounds(at, len) else {
                return true;
            };
            let uid = self.current_uid();
            let Some(VarDef::Music { tl, voices, .. }) = self.definitions.get(name) else {
                unreachable!()
            };
            let part = self.parts.last_mut().expect("current_uid made a part");
            part.tl.splice(tl, at, main_lane, lane);
            for voice in voices {
                self.voice_part_map.insert(voice.clone(), uid);
            }
            self.pos = end;
            self.voice_start = self.pos;
            return true;
        }
        let Some(def) = self.definitions.get(name).cloned() else {
            return false;
        };
        match def {
            VarDef::Parts(parts, voices) => {
                let count: usize = parts.iter().map(|pb| pb.tl.element_count()).sum();
                if !self.spend(count as u64) {
                    return true;
                }
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
            VarDef::FiguredBass(entries) => self.place_figures(entries),
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
                let octave = self.bounded_octave(inferred_octave.saturating_add(octave_marks));
                let p = Pitch::with_alter(step, alter, octave);
                self.prev_pitch = Some(p);
                p
            } else {
                // First note after \relative: use the reference pitch's octave
                let base_oct = self.relative_ref.as_ref().map(|r| r.octave).unwrap_or(4);
                let octave = self.bounded_octave(base_oct.saturating_add(octave_marks));
                let p = Pitch::with_alter(step, alter, octave);
                self.prev_pitch = Some(p);
                p
            }
        } else {
            // Absolute mode: octave marks relative to LilyPond c (octave 3 in
            // our numbering), and `\fixed`'s octaves.
            let octave = self.bounded_octave(
                octave_marks
                    .saturating_add(3)
                    .saturating_add(self.fixed_octaves),
            );
            Pitch::with_alter(step, alter, octave)
        };
        // Apply any active \transpose intervals
        if !self.transpose_stack.is_empty() {
            let total_semitones: i32 = self
                .transpose_stack
                .iter()
                .map(|(from, to)| to.midi_number() - from.midi_number())
                .fold(0, i32::saturating_add);
            let p = pitch.transposed(total_semitones.clamp(-1200, 1200));
            Pitch {
                octave: self.bounded_octave(p.octave),
                ..p
            }
        } else {
            pitch
        }
    }

    /// `octave` if within [`PITCH_OCTAVES`], else the nearest octave in it
    /// (and the reading stops): a relative passage cannot climb for ever, and
    /// every emitter writes each octave mark out.
    fn bounded_octave(&mut self, octave: i32) -> i32 {
        if !PITCH_OCTAVES.contains(&octave) {
            self.refuse(format!(
                "a pitch lies in octave {octave}, outside {}..={}",
                PITCH_OCTAVES.start(),
                PITCH_OCTAVES.end()
            ));
        }
        octave.clamp(*PITCH_OCTAVES.start(), *PITCH_OCTAVES.end())
    }
}

/// Whether `block` contains a reference to the variable `name` (`\name`).
fn mentions(block: Node, source: &str, name: &str) -> bool {
    let mut cursor = block.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "escaped_word"
            && node
                .utf8_text(source.as_bytes())
                .is_ok_and(|t| t.strip_prefix('\\') == Some(name))
        {
            return true;
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() || cursor.node() == block {
                return false;
            }
        }
    }
}
