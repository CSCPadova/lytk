//! Measure-level IR nodes and attribute value objects.
//!
//! # Influences
//! - Field set and MeasureAttributes/Measure classes from lytk-py
//!   (`lytk-py/ir/measure.py`).
//! - Key/Time/Clef/Transpose frozen dataclasses from lytk-py.
//! - `beats_fraction` compound time handling from lytk-py's `TimeSignature`.

use std::collections::HashMap;

use num::rational::Ratio;
use serde::{Deserialize, Serialize};

use super::direction::{Barline, Direction};
use super::harmony::{FiguredBass, Harmony};
use super::voice::Voice;

/// Key signature.
///
/// From lytk-py's `KeySignature` frozen dataclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeySignature {
    /// Number of fifths on the circle of fifths (-7 to 7).
    pub fifths: i8,
    /// Key mode identifier.
    pub mode: KeyMode,
}

impl Default for KeySignature {
    fn default() -> Self {
        Self {
            fifths: 0,
            mode: KeyMode::Major,
        }
    }
}

/// Key mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum KeyMode {
    #[default]
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Ionian,
    Locrian,
}

impl KeyMode {
    /// Parse a mode string.
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "major" => Self::Major,
            "minor" => Self::Minor,
            "dorian" => Self::Dorian,
            "phrygian" => Self::Phrygian,
            "lydian" => Self::Lydian,
            "mixolydian" => Self::Mixolydian,
            "aeolian" => Self::Aeolian,
            "ionian" => Self::Ionian,
            "locrian" => Self::Locrian,
            _ => Self::Major,
        }
    }

    /// Mode name as a lowercase string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Dorian => "dorian",
            Self::Phrygian => "phrygian",
            Self::Lydian => "lydian",
            Self::Mixolydian => "mixolydian",
            Self::Aeolian => "aeolian",
            Self::Ionian => "ionian",
            Self::Locrian => "locrian",
        }
    }
}

/// Time signature.
///
/// From lytk-py's `TimeSignature` frozen dataclass.
/// Compound beats (e.g. "3+2") are stored as a string to preserve the
/// grouping; use [`beats_fraction`](Self::beats_fraction) for arithmetic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeSignature {
    /// Numerator, possibly compound (e.g. "3+2").
    pub beats: String,
    /// Denominator.
    pub beat_type: u8,
    /// Display symbol: "common", "cut", "single-number", or None for numeric.
    pub symbol: Option<String>,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            beats: "4".to_string(),
            beat_type: 4,
            symbol: None,
        }
    }
}

impl TimeSignature {
    /// Total beats as a rational, handling compound signatures like "3+2".
    ///
    /// From lytk-py's `TimeSignature.beats_fraction` property.
    pub fn beats_fraction(&self) -> Ratio<i64> {
        let numerator: i64 = self
            .beats
            .split('+')
            .filter_map(|b| b.trim().parse::<i64>().ok())
            .sum();
        Ratio::new(numerator, self.beat_type as i64)
    }
}

/// Clef specification.
///
/// From lytk-py's `Clef` frozen dataclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Clef {
    /// Clef sign: G, F, C, percussion, TAB.
    pub sign: ClefSign,
    /// Staff line number (1 = bottom).
    pub line: u8,
    /// Octave transposition (-2 to 2).
    pub octave_change: i8,
}

impl Default for Clef {
    fn default() -> Self {
        Self {
            sign: ClefSign::G,
            line: 2,
            octave_change: 0,
        }
    }
}

/// Clef sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ClefSign {
    #[default]
    G,
    F,
    C,
    Percussion,
    Tab,
}

impl ClefSign {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "G" => Self::G,
            "F" => Self::F,
            "C" => Self::C,
            "PERCUSSION" => Self::Percussion,
            "TAB" => Self::Tab,
            _ => Self::G,
        }
    }
}

/// Transposition information (from MusicXML `<transpose>`).
///
/// From lytk-py's `Transpose` frozen dataclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Transpose {
    pub diatonic: i8,
    pub chromatic: i8,
    pub octave_change: i8,
}

/// Attributes that may change at the start of a measure.
///
/// From lytk-py's `MeasureAttributes(IRNode)`. In the Rust IR this is a plain
/// struct stored as an `Option<MeasureAttributes>` on [`Measure`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasureAttributes {
    /// Divisions per quarter note (for MusicXML duration math).
    pub divisions: u16,
    /// Key signature, if changed at this measure.
    pub key: Option<KeySignature>,
    /// Time signature, if changed at this measure.
    pub time: Option<TimeSignature>,
    /// Clefs per staff number, if changed.
    pub clefs: HashMap<u8, Clef>,
    /// Transposition info, if present.
    pub transpose: Option<Transpose>,
    /// Number of staves, if changed.
    pub staves: Option<u8>,
    /// Number of staff lines (default 5). E.g. 1-line percussion, 6-line TAB.
    pub staff_lines: Option<u8>,
}

impl Default for MeasureAttributes {
    fn default() -> Self {
        Self {
            divisions: 1,
            key: None,
            time: None,
            clefs: HashMap::new(),
            transpose: None,
            staves: None,
            staff_lines: None,
        }
    }
}

/// A single measure / bar.
///
/// Contains [`Voice`] children and optional measure-level attributes,
/// barlines, and directions.
///
/// From lytk-py's `Measure(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    /// 1-based measure number (numeric value used for re-barring / arithmetic).
    pub number: u32,
    /// Original measure label when it is not a plain integer (e.g. MusicXML
    /// `number="3A"` or `"X1"`). `None` means the label is exactly
    /// [`number`](Self::number) rendered as a decimal. The MusicXML exporter
    /// emits this verbatim when present, so non-numeric labels round-trip
    /// instead of collapsing to `0`. Omitted from serialization when `None`
    /// so existing numeric-measure JSON is byte-for-byte unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_label: Option<String>,
    /// Whether this is an implicit measure (e.g. anacrusis / pickup).
    pub implicit: bool,
    /// Senza misura (free time, e.g. inside `\cadenzaOn … \cadenzaOff`). Such a
    /// measure has no fixed length and must not be re-barred to a time signature.
    #[serde(default)]
    pub senza_misura: bool,
    /// Optional width hint.
    pub width: Option<f32>,
    /// Attributes that take effect at this measure.
    pub attributes: Option<MeasureAttributes>,
    /// Left barline.
    pub left_barline: Option<Barline>,
    /// Right barline.
    pub right_barline: Option<Barline>,
    /// Directions attached to this measure.
    pub directions: Vec<Direction>,
    /// Chord symbols (harmony) in this measure.
    pub harmonies: Vec<Harmony>,
    /// Figured bass indications in this measure.
    pub figured_bass: Vec<FiguredBass>,
    /// Whether this measure should be printed (MusicXML `print-object`).
    pub print_object: bool,
    /// Multi-measure rest count (e.g. 4 = rest spanning 4 measures).
    pub multi_measure_rest: Option<u16>,
    /// Voices within this measure.
    pub voices: Vec<Voice>,
}

impl Measure {
    pub fn new(number: u32) -> Self {
        Self {
            number,
            number_label: None,
            implicit: false,
            senza_misura: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: Vec::new(),
            harmonies: Vec::new(),
            figured_bass: Vec::new(),
            print_object: true,
            multi_measure_rest: None,
            voices: Vec::new(),
        }
    }
}

impl std::fmt::Display for Measure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<Measure {} voices={}>", self.number, self.voices.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_signature_default() {
        let ks = KeySignature::default();
        assert_eq!(ks.fifths, 0);
        assert_eq!(ks.mode, KeyMode::Major);
    }

    #[test]
    fn time_signature_compound_beats() {
        let ts = TimeSignature {
            beats: "3+2".to_string(),
            beat_type: 8,
            symbol: None,
        };
        assert_eq!(ts.beats_fraction(), Ratio::new(5, 8));
    }

    #[test]
    fn time_signature_simple() {
        let ts = TimeSignature::default();
        assert_eq!(ts.beats_fraction(), Ratio::new(4, 4));
    }

    #[test]
    fn clef_default_is_treble() {
        let c = Clef::default();
        assert_eq!(c.sign, ClefSign::G);
        assert_eq!(c.line, 2);
    }

    #[test]
    fn measure_new() {
        let m = Measure::new(1);
        assert_eq!(m.number, 1);
        assert!(!m.implicit);
        assert!(m.voices.is_empty());
    }

    #[test]
    fn key_mode_roundtrip() {
        assert_eq!(KeyMode::from_str_loose("Dorian"), KeyMode::Dorian);
        assert_eq!(KeyMode::Dorian.as_str(), "dorian");
    }
}
