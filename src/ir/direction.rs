//! Direction and barline types.
//!
//! Directions are score-level instructions (tempo, text, rehearsal marks, pedal,
//! octave shifts) that are attached to measures but are not notes.
//!
//! # Influences
//! - Typed direction payload model from lytk-py (`lytk-py/ir/direction.py`).
//! - Spanner-with-duration model from PDMX (`PDMX/reading/classes.py`).

use super::articulation::Placement;
use super::duration::Frac;
use super::serde_defaults::{is_default, reduced};
use serde::{Deserialize, Serialize};

/// Layout break type (page / system / section).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayoutBreakType {
    Page,
    System,
    Section,
}

/// Reference to an instrument (for mid-part instrument changes).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentRef {
    pub instrument_id: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub instrument_name: Option<String>,
}

/// Barline style.
///
/// From lytk-py's `BarlineType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BarlineType {
    #[default]
    Regular,
    Double,
    Final,
    RepeatForward,
    RepeatBackward,
    RepeatBoth,
    Dashed,
    Dotted,
    Tick,
    Short,
    None,
}

named_enum! {
    /// Which side of the bar a barline stands on (MusicXML's `location`).
    #[derive(Default)]
    pub enum BarlineLocation {
        #[default]
        Right => "right",
        Left => "left",
        Middle => "middle",
    }
}

named_enum! {
    /// A volta bracket's start, its end, or an end without a hook
    /// (MusicXML's `<ending type>`).
    pub enum EndingType {
        Start => "start",
        Stop => "stop",
        Discontinue => "discontinue",
    }
}

named_enum! {
    /// A text's style (MusicXML's `font-style`).
    pub enum FontStyle {
        Normal => "normal",
        Italic => "italic",
    }
}

named_enum! {
    /// A text's weight (MusicXML's `font-weight`).
    pub enum FontWeight {
        Normal => "normal",
        Bold => "bold",
    }
}

named_enum! {
    /// An octave sign's start, by where the music sounds from where it is
    /// written (`Up` for an 8va, LilyPond's `\ottava #1`; MusicXML calls
    /// it "down": the notes are printed down), its end or continuation.
    pub enum OctaveShiftType {
        Up => "up",
        Down => "down",
        Stop => "stop",
        Continue => "continue",
    }
}

named_enum! {
    /// A pedal mark (MusicXML's `<pedal type>`).
    pub enum PedalType {
        Start => "start",
        Stop => "stop",
        Change => "change",
        Sostenuto => "sostenuto",
        Continue => "continue",
        Discontinue => "discontinue",
        Resume => "resume",
    }
}

/// Repeat direction (forward/backward).
///
/// From lytk-py's `RepeatDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RepeatDirection {
    Forward,
    Backward,
}

/// A barline.
///
/// From lytk-py's `Barline` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Barline {
    #[serde(default, skip_serializing_if = "is_default")]
    pub style: BarlineType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub location: BarlineLocation,
    #[serde(default, skip_serializing_if = "is_default")]
    pub repeat_direction: Option<RepeatDirection>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ending_number: Option<u8>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ending_type: Option<EndingType>,
    /// Number of times a repeat plays (MusicXML `<repeat times="N">`,
    /// LilyPond `\repeat volta N`). Carried on the forward barline; `None`
    /// defaults to 2 at emit time.
    #[serde(default, skip_serializing_if = "is_default")]
    pub repeat_times: Option<u8>,
}

impl Default for Barline {
    fn default() -> Self {
        Self {
            style: BarlineType::Regular,
            location: BarlineLocation::Right,
            repeat_direction: None,
            ending_number: None,
            ending_type: None,
            repeat_times: None,
        }
    }
}

/// A tempo marking.
///
/// From lytk-py's `TempoDirection`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoDirection {
    #[serde(default, skip_serializing_if = "is_default")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub beat_unit: Option<super::duration::NoteType>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub per_minute: Option<f64>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub dots: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
}

/// A text direction (e.g. "dolce", "pizz.").
///
/// From lytk-py's `TextDirection`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextDirection {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
    #[serde(default, skip_serializing_if = "is_default")]
    pub font_style: Option<FontStyle>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub font_weight: Option<FontWeight>,
}

/// A rehearsal mark.
///
/// From lytk-py's `RehearsalMark`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RehearsalMark {
    pub text: String,
}

impl RehearsalMark {
    /// LilyPond's `n`th default mark (from 1): A, B, … H, J (no I) … Z,
    /// AA, AB, …
    pub fn lilypond_default(n: u32) -> String {
        const LETTERS: &[u8] = b"ABCDEFGHJKLMNOPQRSTUVWXYZ";
        let mut n = n.max(1) as usize;
        let mut out = Vec::new();
        while n > 0 {
            n -= 1;
            out.push(LETTERS[n % LETTERS.len()]);
            n /= LETTERS.len();
        }
        out.reverse();
        String::from_utf8(out).unwrap_or_default()
    }
}

/// An ottava (octave shift) indication.
///
/// From lytk-py's `OctaveShift`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OctaveShift {
    pub shift_type: OctaveShiftType,
    /// MusicXML's size: the interval the shift spans, 8, 15 or 22 (0 for a
    /// stop).
    pub size: i8,
}

impl OctaveShift {
    /// LilyPond's `\ottava #n`: `n` octaves up (positive) or down, at most
    /// three either way (22ma); 0 stops the shift.
    pub fn from_octaves(n: i64) -> Self {
        let octaves = n.clamp(-3, 3) as i8;
        let shift_type = match octaves.signum() {
            1 => OctaveShiftType::Up,
            -1 => OctaveShiftType::Down,
            _ => OctaveShiftType::Stop,
        };
        Self {
            shift_type,
            size: if octaves == 0 {
                0
            } else {
                7 * octaves.abs() + 1
            },
        }
    }

    /// The octaves of the shift, as `\ottava` takes them: positive up,
    /// negative down, 0 for a stop.
    pub fn octaves(&self) -> i8 {
        let n = (self.size.clamp(8, 22) - 1) / 7;
        match self.shift_type {
            OctaveShiftType::Up => n,
            OctaveShiftType::Down => -n,
            OctaveShiftType::Stop | OctaveShiftType::Continue => 0,
        }
    }
}

/// A pedal marking.
///
/// From lytk-py's `PedalEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PedalEvent {
    pub pedal_type: PedalType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub line: bool,
}

/// A direction element holding typed payload(s).
///
/// Corresponds to MusicXML `<direction>` or LilyPond top-level commands like
/// `\tempo`, `\mark`, etc.
///
/// From lytk-py's `Direction(IRNode)`. In the Rust IR, directions are stored
/// as a `Vec<Direction>` on [`Measure`](super::measure::Measure) rather than
/// as tree children, following lytk-py's field-based approach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Direction {
    /// Where in the bar, in whole notes from its start (1/2: two quarter
    /// beats in).
    #[serde(
        default,
        skip_serializing_if = "is_default",
        deserialize_with = "reduced"
    )]
    pub offset: Frac,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
    /// MusicXML `<staff>` this direction attaches to in a multi-staff part.
    /// `0` = unset (no `<staff>` emitted); `1..=N` = a specific staff.
    #[serde(default, skip_serializing_if = "is_default")]
    pub staff: u8,
    /// The voice a note's dynamic, hairpin or text belongs to (a LilyPond
    /// `c4\\f`), at the note's onset; `None` for the staff's.
    #[serde(default, skip_serializing_if = "is_default")]
    pub voice: Option<u8>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tempo: Option<TempoDirection>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub text: Option<TextDirection>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub rehearsal: Option<RehearsalMark>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub octave_shift: Option<OctaveShift>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub pedal: Option<PedalEvent>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub dynamic: Option<super::articulation::DynamicMark>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub wedge: Option<super::articulation::Wedge>,
    /// Coda sign.
    #[serde(default, skip_serializing_if = "is_default")]
    pub coda: bool,
    /// Segno sign.
    #[serde(default, skip_serializing_if = "is_default")]
    pub segno: bool,
    /// Da Capo text (e.g. "D.C.", "D.C. al Fine").
    #[serde(default, skip_serializing_if = "is_default")]
    pub da_capo: Option<String>,
    /// Dal Segno text (e.g. "D.S.", "D.S. al Coda").
    #[serde(default, skip_serializing_if = "is_default")]
    pub dal_segno: Option<String>,
    /// Layout break at this position.
    #[serde(default, skip_serializing_if = "is_default")]
    pub layout_break: Option<LayoutBreakType>,
    /// Mid-part instrument change.
    #[serde(default, skip_serializing_if = "is_default")]
    pub instrument_change: Option<InstrumentRef>,
    /// A clef change inside the bar, on `staff` (a bar's opening clef is in
    /// its attributes).
    #[serde(default, skip_serializing_if = "is_default")]
    pub clef: Option<super::measure::Clef>,
}

impl Default for Direction {
    fn default() -> Self {
        Self {
            offset: Frac::from_integer(0),
            placement: Placement::Unspecified,
            staff: 0,
            voice: None,
            tempo: None,
            text: None,
            rehearsal: None,
            octave_shift: None,
            pedal: None,
            dynamic: None,
            wedge: None,
            coda: false,
            segno: false,
            da_capo: None,
            dal_segno: None,
            layout_break: None,
            instrument_change: None,
            clef: None,
        }
    }
}
