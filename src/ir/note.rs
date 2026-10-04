//! Note, rest, chord, forward, and backup IR nodes.
//!
//! Voice-level elements representing actual sounding events (or time
//! positioning adjustments) within a measure.
//!
//! # Influences
//! - Field set and class hierarchy from lytk-py (`lytk-py/ir/note.py`).
//! - Grace-note and cue-note flags from lytk-py's `Note.__slots__`.
//! - Forward/Backup time-shift model from MusicXML via lytk-py.

use super::serde_defaults::{is_default, is_one, is_yes, one, yes};
use serde::{Deserialize, Serialize};

use super::articulation::{
    Articulation, BeamEvent, DynamicMark, Fermata, LyricSyllable, Ornament, SlurEvent, StartStop,
    Technical, TieEvent, TupletDisplay, Wedge,
};
use super::direction::TextDirection;
use super::duration::Duration;
use super::pitch::Pitch;

/// Arpeggio direction for chords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArpeggioType {
    Up,
    Down,
    NonArpeggio,
}

named_enum! {
    /// Which way a note's stem points (MusicXML's `<stem>`).
    pub enum StemDirection {
        Up => "up",
        Down => "down",
        Double => "double",
        /// No stem drawn.
        NoStem => "none",
    }
}

named_enum! {
    /// A notehead's shape (MusicXML's `<notehead>`).
    pub enum Notehead {
        Slash => "slash",
        Triangle => "triangle",
        Diamond => "diamond",
        Square => "square",
        Cross => "cross",
        X => "x",
        CircleX => "circle-x",
        InvertedTriangle => "inverted triangle",
        ArrowDown => "arrow down",
        ArrowUp => "arrow up",
        Circled => "circled",
        Slashed => "slashed",
        BackSlashed => "back slashed",
        Normal => "normal",
        Cluster => "cluster",
        CircleDot => "circle dot",
        LeftTriangle => "left triangle",
        Rectangle => "rectangle",
        /// No notehead drawn.
        NoHead => "none",
        Do => "do",
        Re => "re",
        Mi => "mi",
        Fa => "fa",
        FaUp => "fa up",
        So => "so",
        La => "la",
        Ti => "ti",
        Other => "other",
    }
}

named_enum! {
    /// How a glissando or slide is drawn (MusicXML's `line-type`).
    pub enum LineType {
        Solid => "solid",
        Dashed => "dashed",
        Dotted => "dotted",
        Wavy => "wavy",
    }
}

/// Most tremolo slashes a note carries (LilyPond's `:1024` on a whole note).
pub const MAX_TREMOLO_MARKS: u8 = 10;

/// A single pitched note.
///
/// From lytk-py's `Note(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: Pitch,
    pub duration: Duration,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub voice: u8,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub staff: u8,
    // -- articulations & notation --
    #[serde(default, skip_serializing_if = "is_default")]
    pub ties: Vec<TieEvent>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub slurs: Vec<SlurEvent>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub articulations: Vec<Articulation>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ornaments: Vec<Ornament>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub technicals: Vec<Technical>,
    // Reader scratch, empty in a returned score: a note's dynamics, hairpins
    // and text are the bar's directions, with its voice (`ir::marks`).
    #[serde(skip)]
    pub(crate) dynamics: Vec<DynamicMark>,
    #[serde(skip)]
    pub(crate) wedges: Vec<Wedge>,
    #[serde(skip)]
    pub(crate) text_directions: Vec<TextDirection>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub beams: Vec<BeamEvent>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tuplet: Option<TupletDisplay>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fermata: Option<Fermata>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub lyrics: Vec<LyricSyllable>,
    // -- flags --
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_grace: bool,
    /// Whether the grace note has a slash (acciaccatura). Only meaningful when `is_grace` is true.
    #[serde(default, skip_serializing_if = "is_default")]
    pub grace_slash: bool,
    /// Whether this grace note steals time from the previous note (`\afterGrace`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub after_grace: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_cue: bool,
    /// Glissando start/stop.
    #[serde(default, skip_serializing_if = "is_default")]
    pub glissando: Option<StartStop>,
    /// Slide (portamento) start/stop.
    #[serde(default, skip_serializing_if = "is_default")]
    pub slide: Option<StartStop>,
    /// How the glissando is drawn, when the source says.
    #[serde(default, skip_serializing_if = "is_default")]
    pub glissando_line_type: Option<LineType>,
    /// The stem the source gives, `None` for the engraver's.
    #[serde(default, skip_serializing_if = "is_default")]
    pub stem_direction: Option<StemDirection>,
    /// The notehead the source gives, `None` for a plain one.
    #[serde(default, skip_serializing_if = "is_default")]
    pub notehead: Option<Notehead>,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub print_object: bool,
    /// Number of tremolo slashes (1–4) for single-note tremolo; writers cap
    /// it at [`MAX_TREMOLO_MARKS`].
    #[serde(default, skip_serializing_if = "is_default")]
    pub tremolo_marks: u8,
    /// Whether this note is part of a two-note tremolo (paired with next/prev note).
    #[serde(default, skip_serializing_if = "is_default")]
    pub two_note_tremolo: bool,
    /// For two-note tremolo: true = first note (start), false = second note (stop).
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub tremolo_start: bool,
    /// If true, auto-beaming should skip this note (\autoBeamOff).
    #[serde(default, skip_serializing_if = "is_default")]
    pub no_auto_beam: bool,
    /// If true, this note is inside a \melisma ... \melismaEnd block and does not consume a lyric syllable.
    #[serde(default, skip_serializing_if = "is_default")]
    pub in_melisma: bool,
    /// MIDI velocity (1–127) the note was played with, when known (from a MIDI
    /// file or a MusicXML `<note dynamics>`). MIDI export uses it instead of
    /// the dynamic in force.
    #[serde(default, skip_serializing_if = "is_default")]
    pub velocity: Option<u8>,
    /// While a LilyPond file is read: the Voice context the note was
    /// written in, which `\lyricsto` follows. 0 in every score returned.
    #[serde(skip)]
    pub lyric_voice: u32,
}

impl Note {
    /// Create a note with sensible defaults.
    pub fn new(pitch: Pitch, duration: Duration) -> Self {
        Self {
            pitch,
            duration,
            voice: 1,
            staff: 1,
            ties: Vec::new(),
            slurs: Vec::new(),
            articulations: Vec::new(),
            ornaments: Vec::new(),
            technicals: Vec::new(),
            dynamics: Vec::new(),
            wedges: Vec::new(),
            text_directions: Vec::new(),
            beams: Vec::new(),
            tuplet: None,
            fermata: None,
            lyrics: Vec::new(),
            is_grace: false,
            grace_slash: false,
            after_grace: false,
            is_cue: false,
            glissando: None,
            slide: None,
            glissando_line_type: None,
            stem_direction: None,
            notehead: None,
            print_object: true,
            tremolo_marks: 0,
            two_note_tremolo: false,
            tremolo_start: true,
            no_auto_beam: false,
            in_melisma: false,
            velocity: None,
            lyric_voice: 0,
        }
    }
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<Note {} dur={}>",
            self.pitch,
            self.duration.actual_duration()
        )
    }
}

/// A rest or spacer rest.
///
/// From lytk-py's `Rest(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rest {
    pub duration: Duration,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub voice: u8,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub staff: u8,
    /// Optional display pitch step (for positioned rests in MusicXML).
    #[serde(default, skip_serializing_if = "is_default")]
    pub display_step: Option<super::pitch::PitchStep>,
    /// Optional display octave (for positioned rests).
    #[serde(default, skip_serializing_if = "is_default")]
    pub display_octave: Option<i32>,
    /// Whether this is a whole-measure rest.
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_measure_rest: bool,
    /// Whether this is an invisible spacer (LilyPond `s`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_spacer: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fermata: Option<Fermata>,
    /// Tuplet display bracket / numbering (start or stop).
    #[serde(default, skip_serializing_if = "is_default")]
    pub tuplet: Option<TupletDisplay>,
    // Reader scratch, empty in a returned score (`ir::marks`).
    #[serde(skip)]
    pub(crate) dynamics: Vec<DynamicMark>,
    #[serde(skip)]
    pub(crate) wedges: Vec<Wedge>,
}

impl Rest {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            voice: 1,
            staff: 1,
            display_step: None,
            display_octave: None,
            is_measure_rest: false,
            is_spacer: false,
            fermata: None,
            tuplet: None,
            dynamics: Vec::new(),
            wedges: Vec::new(),
        }
    }

    pub fn measure_rest(duration: Duration) -> Self {
        Self {
            is_measure_rest: true,
            ..Self::new(duration)
        }
    }
}

impl std::fmt::Display for Rest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.is_measure_rest {
            "MeasureRest"
        } else {
            "Rest"
        };
        write!(f, "<{} dur={}>", kind, self.duration.actual_duration())
    }
}

/// A chord — multiple notes sounding simultaneously with shared duration.
///
/// From lytk-py's `Chord(IRNode)`. In the Python prototype, child Note nodes
/// are tree children; here we store them in a `Vec<Note>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chord {
    pub duration: Duration,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub voice: u8,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub staff: u8,
    pub notes: Vec<Note>,
    /// Arpeggio indication.
    #[serde(default, skip_serializing_if = "is_default")]
    pub arpeggio: Option<ArpeggioType>,
}

impl Chord {
    pub fn new(duration: Duration, notes: Vec<Note>) -> Self {
        Self {
            duration,
            voice: 1,
            staff: 1,
            notes,
            arpeggio: None,
        }
    }
}

impl std::fmt::Display for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pitches: Vec<String> = self.notes.iter().map(|n| n.pitch.to_string()).collect();
        write!(
            f,
            "<Chord [{}] dur={}>",
            pitches.join(", "),
            self.duration.actual_duration()
        )
    }
}

/// A voice element — one of the things that can appear inside a voice.
///
/// This enum replaces the dynamic-dispatch `IRNode` children list from the
/// Python prototype with a typed Rust enum, making pattern matching exhaustive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VoiceElement {
    Note(Box<Note>),
    Rest(Rest),
    Chord(Chord),
}

impl VoiceElement {
    /// The staff the element is written on.
    pub fn staff(&self) -> u8 {
        match self {
            VoiceElement::Note(n) => n.staff,
            VoiceElement::Rest(r) => r.staff,
            VoiceElement::Chord(c) => c.staff,
        }
    }

    /// The element's notes: one, a chord's, none for a rest.
    pub fn notes(&self) -> &[Note] {
        match self {
            VoiceElement::Note(n) => std::slice::from_ref(n),
            VoiceElement::Chord(c) => &c.notes,
            VoiceElement::Rest(_) => &[],
        }
    }

    /// The element's notes, to change.
    pub fn notes_mut(&mut self) -> &mut [Note] {
        match self {
            VoiceElement::Note(n) => std::slice::from_mut(n),
            VoiceElement::Chord(c) => &mut c.notes,
            VoiceElement::Rest(_) => &mut [],
        }
    }

    /// An invisible rest (LilyPond `s`, a MusicXML `<forward>`) `len` long.
    pub fn spacer(len: super::duration::Frac, voice: u8, staff: u8) -> VoiceElement {
        let mut r = Rest::new(super::duration::Duration::new(len));
        r.is_spacer = true;
        r.voice = voice;
        r.staff = staff;
        VoiceElement::Rest(r)
    }

    /// Time this element occupies in its measure. Grace notes and grace
    /// chords (their notes carry `is_grace`) take none.
    pub fn metric_duration(&self) -> super::duration::Frac {
        match self {
            VoiceElement::Note(n) if n.is_grace => super::duration::Frac::from_integer(0),
            VoiceElement::Chord(c) if c.notes.first().is_some_and(|n| n.is_grace) => {
                super::duration::Frac::from_integer(0)
            }
            VoiceElement::Note(n) => n.duration.actual_duration(),
            VoiceElement::Rest(r) => r.duration.actual_duration(),
            VoiceElement::Chord(c) => c.duration.actual_duration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::pitch::{Pitch, PitchStep};

    #[test]
    fn note_new() {
        let n = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        assert_eq!(n.voice, 1);
        assert_eq!(n.staff, 1);
        assert!(!n.is_grace);
        assert!(n.articulations.is_empty());
    }

    #[test]
    fn rest_measure_rest() {
        let r = Rest::measure_rest(Duration::whole());
        assert!(r.is_measure_rest);
        assert!(!r.is_spacer);
    }

    #[test]
    fn chord_display() {
        let c = Chord::new(
            Duration::quarter(),
            vec![
                Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter()),
                Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter()),
            ],
        );
        let s = format!("{}", c);
        assert!(s.contains("C4"));
        assert!(s.contains("E4"));
    }

    #[test]
    fn voice_element_pattern_match() {
        let elem = VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::A, 4),
            Duration::quarter(),
        )));
        match elem {
            VoiceElement::Note(n) => assert_eq!(n.pitch.step, PitchStep::A),
            _ => panic!("expected Note"),
        }
    }
}
