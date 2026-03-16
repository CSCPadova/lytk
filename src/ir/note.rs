//! Note, rest, chord, forward, and backup IR nodes.
//!
//! Voice-level elements representing actual sounding events (or time
//! positioning adjustments) within a measure.
//!
//! # Influences
//! - Field set and class hierarchy from lytk-py (`lytk-py/ir/note.py`).
//! - Grace-note and cue-note flags from lytk-py's `Note.__slots__`.
//! - Forward/Backup time-shift model from MusicXML via lytk-py.

use serde::{Deserialize, Serialize};

use super::articulation::{
    Articulation, BeamEvent, DynamicMark, Fermata, LyricSyllable, Ornament, SlurEvent,
    StartStop, Technical, TieEvent, TupletDisplay, Wedge,
};
use super::duration::Duration;
use super::pitch::Pitch;

/// Arpeggio direction for chords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArpeggioType {
    Up,
    Down,
    NonArpeggio,
}

/// A single pitched note.
///
/// From lytk-py's `Note(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: Pitch,
    pub duration: Duration,
    pub voice: u8,
    pub staff: u8,
    // -- articulations & notation --
    pub ties: Vec<TieEvent>,
    pub slurs: Vec<SlurEvent>,
    pub articulations: Vec<Articulation>,
    pub ornaments: Vec<Ornament>,
    pub technicals: Vec<Technical>,
    pub dynamics: Vec<DynamicMark>,
    pub wedges: Vec<Wedge>,
    pub beams: Vec<BeamEvent>,
    pub tuplet: Option<TupletDisplay>,
    pub fermata: Option<Fermata>,
    pub lyrics: Vec<LyricSyllable>,
    // -- flags --
    pub is_grace: bool,
    /// Whether the grace note has a slash (acciaccatura). Only meaningful when `is_grace` is true.
    pub grace_slash: bool,
    /// Whether this grace note steals time from the previous note (`\afterGrace`).
    pub after_grace: bool,
    pub is_cue: bool,
    /// Glissando start/stop.
    pub glissando: Option<StartStop>,
    /// Slide (portamento) start/stop.
    pub slide: Option<StartStop>,
    /// Glissando line type: "solid", "dashed", "dotted", "wavy".
    pub glissando_line_type: Option<String>,
    pub stem_direction: String,
    pub notehead: String,
    pub print_object: bool,
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
            stem_direction: String::new(),
            notehead: String::new(),
            print_object: true,
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
    pub voice: u8,
    pub staff: u8,
    /// Optional display pitch step (for positioned rests in MusicXML).
    pub display_step: Option<String>,
    /// Optional display octave (for positioned rests).
    pub display_octave: Option<i32>,
    /// Whether this is a whole-measure rest.
    pub is_measure_rest: bool,
    /// Whether this is an invisible spacer (LilyPond `s`).
    pub is_spacer: bool,
    pub fermata: Option<Fermata>,
    /// Tuplet display bracket / numbering (start or stop).
    pub tuplet: Option<TupletDisplay>,
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
    pub voice: u8,
    pub staff: u8,
    pub notes: Vec<Note>,
    /// Arpeggio indication.
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

/// A forward movement in time (MusicXML `<forward>`).
///
/// From lytk-py's `Forward(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forward {
    pub duration: Duration,
    pub voice: u8,
    pub staff: u8,
}

impl Forward {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            voice: 1,
            staff: 1,
        }
    }
}

/// A backward movement in time (MusicXML `<backup>`).
///
/// From lytk-py's `Backup(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub duration: Duration,
}

impl Backup {
    pub fn new(duration: Duration) -> Self {
        Self { duration }
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
    Forward(Forward),
    Backup(Backup),
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
        let elem = VoiceElement::Note(Box::new(Note::new(Pitch::new(PitchStep::A, 4), Duration::quarter())));
        match elem {
            VoiceElement::Note(n) => assert_eq!(n.pitch.step, PitchStep::A),
            _ => panic!("expected Note"),
        }
    }
}
