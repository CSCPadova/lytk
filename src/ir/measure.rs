//! Measure-level IR nodes and attribute value objects.
//!
//! # Influences
//! - Field set and MeasureAttributes/Measure classes from lytk-py
//!   (`lytk-py/ir/measure.py`).
//! - Key/Time/Clef/Transpose frozen dataclasses from lytk-py.
//! - `beats_fraction` compound time handling from lytk-py's `TimeSignature`.

use std::collections::HashMap;

use super::serde_defaults::{is_default, is_one, is_yes, one, yes};
use num::rational::Ratio;
use serde::{Deserialize, Serialize};

use super::direction::{Barline, Direction};
use super::harmony::{FiguredBass, Harmony};
use super::pitch::{Pitch, PitchStep};
use super::voice::Voice;

/// Key signature.
///
/// From lytk-py's `KeySignature` frozen dataclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeySignature {
    /// Number of fifths on the circle of fifths (-7 to 7).
    #[serde(default, skip_serializing_if = "is_default")]
    pub fifths: i8,
    /// Key mode identifier.
    #[serde(default, skip_serializing_if = "is_default")]
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

impl KeySignature {
    /// The key's tonic as a step and its alteration in semitones: D for
    /// two sharps in major, B in minor, E in dorian. A mode can reach past
    /// the major keys (G♯ minor is 8 fifths up, F♭ lydian 8 down).
    pub fn tonic(&self) -> (PitchStep, i32) {
        let offset = match self.mode {
            KeyMode::Major | KeyMode::Ionian => 0,
            KeyMode::Minor | KeyMode::Aeolian => -3,
            KeyMode::Dorian => -2,
            KeyMode::Phrygian => -4,
            KeyMode::Lydian => 1,
            KeyMode::Mixolydian => -1,
            KeyMode::Locrian => -5,
        };
        // On the circle of fifths from F: F C G D A E B, then sharps.
        let i = i32::from(self.fifths) - offset + 1;
        let step = [
            PitchStep::F,
            PitchStep::C,
            PitchStep::G,
            PitchStep::D,
            PitchStep::A,
            PitchStep::E,
            PitchStep::B,
        ][i.rem_euclid(7) as usize];
        (step, i.div_euclid(7))
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

named_enum! {
    /// How a time signature is drawn (MusicXML's `symbol`).
    pub enum TimeSymbol {
        Common => "common",
        Cut => "cut",
        SingleNumber => "single-number",
        Normal => "normal",
        Note => "note",
        DottedNote => "dotted-note",
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
    /// How it is drawn, when not as numbers.
    #[serde(default, skip_serializing_if = "is_default")]
    pub symbol: Option<TimeSymbol>,
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
    /// The terms of a numerator written as text, `"3+2"`: its `+`-separated
    /// whole numbers from 1 (anything else is skipped). The one parser of
    /// `beats`.
    pub fn parse_terms(beats: &str) -> impl Iterator<Item = i64> + '_ {
        beats
            .split('+')
            .filter_map(|t| t.trim().parse::<i64>().ok())
            .filter(|&t| t > 0)
    }

    /// The numerator's terms: `[3, 2]` for 3+2/8, `[6]` for 6/8.
    pub fn terms(&self) -> Vec<i64> {
        Self::parse_terms(&self.beats).collect()
    }

    /// The numerator, the terms' sum (saturating; 0 without terms).
    pub fn numerator(&self) -> i64 {
        Self::parse_terms(&self.beats).fold(0, i64::saturating_add)
    }

    /// The denominator: no reader makes a 0, and one from a hand-edited IR
    /// counts as 1 rather than dividing by 0.
    pub fn denominator(&self) -> i64 {
        i64::from(self.beat_type.max(1))
    }

    /// Total beats as a rational, handling compound signatures like "3+2".
    ///
    /// From lytk-py's `TimeSignature.beats_fraction` property.
    pub fn beats_fraction(&self) -> Ratio<i64> {
        Ratio::new(self.numerator(), self.denominator())
    }

    /// A compound meter (6/8, 9/8, 12/16): its beat is three of the
    /// denominator's notes.
    pub fn is_compound(&self) -> bool {
        let n = self.numerator();
        n > 3 && n % 3 == 0
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
    #[serde(default, skip_serializing_if = "is_default")]
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub diatonic: i8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub chromatic: i8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub octave_change: i8,
}

impl Transpose {
    /// Semitones from written to sounding pitch.
    pub fn semitones(&self) -> i32 {
        i32::from(self.chromatic) + 12 * i32::from(self.octave_change)
    }

    /// The transposition `n` semitones up (down when negative), spelled as
    /// the usual interval (2 → a major second, -9 → down a major sixth), with
    /// whole octaves in `octave_change`: ABC's `transpose=` says no more.
    pub fn from_semitones(n: i32) -> Self {
        const DIATONIC: [i32; 12] = [0, 1, 1, 2, 2, 3, 3, 4, 5, 5, 6, 6];
        let (octaves, rest) = (n.abs() / 12, n.abs() % 12);
        let sign = n.signum();
        Transpose {
            diatonic: (sign * DIATONIC[rest as usize]) as i8,
            chromatic: (sign * rest) as i8,
            octave_change: (sign * octaves).clamp(-127, 127) as i8,
        }
    }

    /// The pitch a written c' sounds at (LilyPond's `\transposition`).
    pub fn sounding_c(&self) -> Pitch {
        let o = i32::from(self.octave_change);
        Pitch::new(PitchStep::C, 4).transpose_diatonic(
            i32::from(self.diatonic) + 7 * o,
            i32::from(self.chromatic) + 12 * o,
        )
    }

    /// The transposition of an instrument whose written c' sounds at `p`,
    /// whole octaves apart as MusicXML writes them.
    pub fn from_sounding_c(p: &Pitch) -> Self {
        let diatonic = p.step.index() + 7 * (p.octave - 4);
        let chromatic = p.midi_number() - 60;
        let octave = diatonic / 7;
        Transpose {
            diatonic: (diatonic - 7 * octave) as i8,
            chromatic: (chromatic - 12 * octave) as i8,
            octave_change: octave as i8,
        }
    }
}

/// Attributes that may change at the start of a measure.
///
/// From lytk-py's `MeasureAttributes(IRNode)`. In the Rust IR this is a plain
/// struct stored as an `Option<MeasureAttributes>` on [`Measure`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasureAttributes {
    /// Divisions per quarter note (for MusicXML duration math).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub divisions: u16,
    /// Key signature, if changed at this measure.
    #[serde(default, skip_serializing_if = "is_default")]
    pub key: Option<KeySignature>,
    /// Time signature, if changed at this measure.
    #[serde(default, skip_serializing_if = "is_default")]
    pub time: Option<TimeSignature>,
    /// Clefs per staff number, if changed.
    #[serde(default, skip_serializing_if = "is_default")]
    pub clefs: HashMap<u8, Clef>,
    /// Transposition info, if present.
    #[serde(default, skip_serializing_if = "is_default")]
    pub transpose: Option<Transpose>,
    /// Number of staves, if changed.
    #[serde(default, skip_serializing_if = "is_default")]
    pub staves: Option<u8>,
    /// Number of staff lines (default 5). E.g. 1-line percussion, 6-line TAB.
    #[serde(default, skip_serializing_if = "is_default")]
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub number_label: Option<String>,
    /// Whether this is an implicit measure (e.g. anacrusis / pickup).
    #[serde(default, skip_serializing_if = "is_default")]
    pub implicit: bool,
    /// Senza misura (free time, e.g. inside `\cadenzaOn … \cadenzaOff`). Such a
    /// measure has no fixed length and must not be re-barred to a time signature.
    #[serde(default, skip_serializing_if = "is_default")]
    pub senza_misura: bool,
    /// Optional width hint.
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Option<f32>,
    /// Attributes that take effect at this measure.
    #[serde(default, skip_serializing_if = "is_default")]
    pub attributes: Option<MeasureAttributes>,
    /// Left barline.
    #[serde(default, skip_serializing_if = "is_default")]
    pub left_barline: Option<Barline>,
    /// Right barline.
    #[serde(default, skip_serializing_if = "is_default")]
    pub right_barline: Option<Barline>,
    /// Directions attached to this measure.
    #[serde(default, skip_serializing_if = "is_default")]
    pub directions: Vec<Direction>,
    /// Chord symbols (harmony) in this measure.
    #[serde(default, skip_serializing_if = "is_default")]
    pub harmonies: Vec<Harmony>,
    /// Figured bass indications in this measure.
    #[serde(default, skip_serializing_if = "is_default")]
    pub figured_bass: Vec<FiguredBass>,
    /// Whether this measure should be printed (MusicXML `print-object`).
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub print_object: bool,
    /// Multi-measure rest count (e.g. 4 = rest spanning 4 measures).
    #[serde(default, skip_serializing_if = "is_default")]
    pub multi_measure_rest: Option<u16>,
    /// Measure-repeat: this measure repeats the previous N measures (the "%"
    /// sign; MusicXML `<measure-style><measure-repeat>`). `None` for a normal
    /// measure. Omitted from serialization when absent for JSON back-compat.
    #[serde(default, skip_serializing_if = "is_default")]
    pub measure_repeat: Option<u8>,
    /// Voices within this measure.
    #[serde(default, skip_serializing_if = "is_default")]
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
            measure_repeat: None,
            voices: Vec::new(),
        }
    }

    /// How long the bar's music is: its longest voice (zero when empty).
    pub fn content_length(&self) -> Ratio<i64> {
        self.voices
            .iter()
            .map(|v| {
                v.elements
                    .iter()
                    .map(super::note::VoiceElement::metric_duration)
                    .sum()
            })
            .max()
            .unwrap_or_else(|| Ratio::from_integer(0))
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
    fn a_key_knows_its_tonic_in_any_mode() {
        let tonic = |fifths, mode| KeySignature { fifths, mode }.tonic();
        assert_eq!(tonic(2, KeyMode::Major), (PitchStep::D, 0));
        assert_eq!(tonic(2, KeyMode::Minor), (PitchStep::B, 0));
        assert_eq!(tonic(0, KeyMode::Dorian), (PitchStep::D, 0));
        assert_eq!(tonic(5, KeyMode::Minor), (PitchStep::G, 1));
        assert_eq!(tonic(-7, KeyMode::Locrian), (PitchStep::B, -1));
    }

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
    fn a_meter_reads_its_numerator_one_way() {
        let ts = |beats: &str, beat_type| TimeSignature {
            beats: beats.to_string(),
            beat_type,
            symbol: None,
        };
        assert_eq!(ts("3+2", 8).terms(), vec![3, 2]);
        assert_eq!(ts("3 + x + 0 + 2", 8).numerator(), 5);
        assert!(ts("6", 8).is_compound() && ts("3+3", 8).is_compound());
        assert!(!ts("3", 8).is_compound() && !ts("4", 4).is_compound());
        assert_eq!(ts("4", 0).beats_fraction(), Ratio::new(4, 1));
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
