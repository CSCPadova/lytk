//! Pitch representation.
//!
//! A [`Pitch`] is a language-agnostic value object combining a diatonic step (C–B),
//! chromatic alteration, octave number, and an optional display accidental.
//!
//! # Influences
//! - Step as integer 0–6 (C..B) and alter in fractional semitones from quickly
//!   (python-ly successor, `quickly/pitch.py`).
//! - `PitchStep` / `Accidental` enums from lilypond-rs (`lilypond-rs/src/notation/pitch.rs`).
//! - `Fraction`-based alter for microtone support from lytk-py (`lytk-py/ir/pitch.py`).
//! - MIDI conversion from both lilypond-rs and PDMX.

use num::rational::Ratio;
use serde::{Deserialize, Serialize};

/// Diatonic pitch step (C = 0 through B = 6).
///
/// Modelled as an integer enum to enable diatonic arithmetic.
/// Inspired by `PitchStep(IntEnum)` in lytk-py and `NoteName` in lilypond-rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PitchStep {
    C = 0,
    D = 1,
    E = 2,
    F = 3,
    G = 4,
    A = 5,
    B = 6,
}

impl PitchStep {
    /// Number of semitones from C within one octave.
    ///
    /// Semitone map from quickly's `MAJOR_SCALE` (converted to semitones).
    pub const fn semitones(self) -> i32 {
        match self {
            Self::C => 0,
            Self::D => 2,
            Self::E => 4,
            Self::F => 5,
            Self::G => 7,
            Self::A => 9,
            Self::B => 11,
        }
    }

    /// Create from a diatonic index 0–6, wrapping.
    pub fn from_index(index: i32) -> Self {
        match index.rem_euclid(7) {
            0 => Self::C,
            1 => Self::D,
            2 => Self::E,
            3 => Self::F,
            4 => Self::G,
            5 => Self::A,
            6 => Self::B,
            _ => unreachable!(),
        }
    }

    /// Diatonic index (C=0 .. B=6).
    pub const fn index(self) -> i32 {
        self as i32
    }

    /// Parse from a single-character string ("C", "D", …).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "C" | "c" => Some(Self::C),
            "D" | "d" => Some(Self::D),
            "E" | "e" => Some(Self::E),
            "F" | "f" => Some(Self::F),
            "G" | "g" => Some(Self::G),
            "A" | "a" => Some(Self::A),
            "B" | "b" => Some(Self::B),
            _ => None,
        }
    }

    /// Single-letter uppercase name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::A => "A",
            Self::B => "B",
        }
    }
}

/// Display hint for accidentals (does not affect pitch value).
///
/// From lytk-py's `AccidentalType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AccidentalDisplay {
    /// No explicit accidental displayed.
    #[default]
    None,
    /// Courtesy/cautionary accidental (parenthesised).
    Cautionary,
    /// Forced accidental (always printed).
    Forced,
    /// Editorial accidental (small, above the note).
    Editorial,
}

/// Chromatic alteration stored as a ratio of semitones.
///
/// Using `Ratio<i32>` allows exact representation of quarter-tones and other
/// microtonal alterations. For standard alterations:
/// - sharp = `Ratio::new(1, 1)` (= 1 semitone)
/// - flat  = `Ratio::new(-1, 1)`
/// - quarter-sharp = `Ratio::new(1, 2)`
///
/// Inspired by lytk-py's `Fraction`-based `alter` field and quickly's
/// fractional alter in whole tones (we use semitones for MIDI compatibility).
pub type Alter = Ratio<i32>;

/// A pitch value object.
///
/// Combines a diatonic step, chromatic alteration, octave, and display accidental.
/// Middle C is `Pitch { step: C, alter: 0, octave: 4, .. }` (MIDI 60).
///
/// The pitch is language-agnostic — the LilyPond pitch-name language
/// (nederlands, english, italiano, …) is handled by adapters at parse/emit time,
/// not stored here. This follows quickly's `PitchProcessor` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pitch {
    pub step: PitchStep,
    /// Chromatic alteration in semitones (e.g. 1 = sharp, −1 = flat).
    pub alter: Alter,
    /// Octave number. Middle C octave = 4 (scientific pitch notation).
    pub octave: i32,
    /// Display hint — does not change the sounding pitch.
    pub accidental: AccidentalDisplay,
}

impl Pitch {
    /// Create a natural pitch.
    pub fn new(step: PitchStep, octave: i32) -> Self {
        Self {
            step,
            alter: Alter::from_integer(0),
            octave,
            accidental: AccidentalDisplay::None,
        }
    }

    /// Create a pitch with alteration.
    pub fn with_alter(step: PitchStep, alter: Alter, octave: i32) -> Self {
        Self {
            step,
            alter,
            octave,
            accidental: AccidentalDisplay::None,
        }
    }

    /// MIDI note number (middle C = 60).
    ///
    /// Conversion logic from lilypond-rs (`TryFrom<&Pitch> for MidiNote`)
    /// and lytk-py (`Pitch.midi_number`).
    pub fn midi_number(&self) -> i32 {
        (self.octave + 1) * 12 + self.step.semitones() + *self.alter.numer() / *self.alter.denom()
    }

    /// Create a new pitch transposed by the given number of semitones.
    ///
    /// Transposition algorithm from quickly's `Transposer`: decompose into
    /// diatonic steps and chromatic remainder, then adjust.
    pub fn transposed(&self, semitones: i32) -> Self {
        let current_midi = self.midi_number();
        let target_midi = current_midi + semitones;

        // Find the closest diatonic pitch (natural) to the target MIDI number.
        // Use floor division (div_euclid) so the octave stays consistent with the
        // floored chroma (rem_euclid) for negative MIDI numbers — otherwise low
        // pitches (e.g. a double-flat transposed far down) round-trip incorrectly.
        let target_octave = target_midi.div_euclid(12) - 1;
        let target_chroma = target_midi.rem_euclid(12);

        // Find the nearest step
        let (best_step, best_semitones) = (0..7)
            .map(|i| {
                let s = PitchStep::from_index(i);
                (s, s.semitones())
            })
            .min_by_key(|&(_, semi)| (semi - target_chroma).abs())
            .unwrap();

        let remaining_alter = target_chroma - best_semitones;

        Self {
            step: best_step,
            alter: Alter::from_integer(remaining_alter),
            octave: target_octave,
            accidental: AccidentalDisplay::None,
        }
    }
}

impl Default for Pitch {
    /// Middle C.
    fn default() -> Self {
        Self::new(PitchStep::C, 4)
    }
}

impl std::fmt::Display for Pitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alter_str = if self.alter == Alter::from_integer(0) {
            String::new()
        } else if self.alter == Alter::from_integer(1) {
            "#".to_string()
        } else if self.alter == Alter::from_integer(-1) {
            "b".to_string()
        } else if self.alter == Alter::from_integer(2) {
            "##".to_string()
        } else if self.alter == Alter::from_integer(-2) {
            "bb".to_string()
        } else {
            format!("({}/{})", self.alter.numer(), self.alter.denom())
        };
        write!(f, "{}{}{}", self.step.name(), alter_str, self.octave)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_middle_c() {
        let c = Pitch::default();
        assert_eq!(c.midi_number(), 60);
        assert_eq!(c.step, PitchStep::C);
        assert_eq!(c.octave, 4);
    }

    #[test]
    fn test_a4_concert() {
        let a = Pitch::new(PitchStep::A, 4);
        assert_eq!(a.midi_number(), 69);
    }

    #[test]
    fn test_sharp_flat() {
        let cs = Pitch::with_alter(PitchStep::C, Alter::from_integer(1), 4);
        assert_eq!(cs.midi_number(), 61);

        let bf = Pitch::with_alter(PitchStep::B, Alter::from_integer(-1), 3);
        assert_eq!(bf.midi_number(), 58);
    }

    #[test]
    fn test_transpose_up() {
        let c4 = Pitch::default();
        let result = c4.transposed(7); // perfect fifth up
        assert_eq!(result.midi_number(), 67);
    }

    #[test]
    fn test_step_from_name() {
        assert_eq!(PitchStep::from_name("C"), Some(PitchStep::C));
        assert_eq!(PitchStep::from_name("g"), Some(PitchStep::G));
        assert_eq!(PitchStep::from_name("X"), None);
    }

    #[test]
    fn test_display() {
        let c4 = Pitch::default();
        assert_eq!(format!("{c4}"), "C4");

        let fs = Pitch::with_alter(PitchStep::F, Alter::from_integer(1), 5);
        assert_eq!(format!("{fs}"), "F#5");
    }
}
