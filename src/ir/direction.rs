//! Direction and barline types.
//!
//! Directions are score-level instructions (tempo, text, rehearsal marks, pedal,
//! octave shifts) that are attached to measures but are not notes.
//!
//! # Influences
//! - Typed direction payload model from lytk-py (`lytk-py/ir/direction.py`).
//! - Spanner-with-duration model from PDMX (`PDMX/reading/classes.py`).

use super::articulation::Placement;
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
    pub style: BarlineType,
    pub location: String,
    pub repeat_direction: Option<RepeatDirection>,
    pub ending_number: Option<u8>,
    pub ending_type: Option<String>,
}

impl Default for Barline {
    fn default() -> Self {
        Self {
            style: BarlineType::Regular,
            location: "right".to_string(),
            repeat_direction: None,
            ending_number: None,
            ending_type: None,
        }
    }
}

/// A tempo marking.
///
/// From lytk-py's `TempoDirection`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoDirection {
    pub text: Option<String>,
    pub beat_unit: Option<String>,
    pub per_minute: Option<f64>,
    pub dots: u8,
    pub placement: Placement,
}

/// A text direction (e.g. "dolce", "pizz.").
///
/// From lytk-py's `TextDirection`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextDirection {
    pub text: String,
    pub placement: Placement,
    pub font_style: Option<String>,
    pub font_weight: Option<String>,
}

/// A rehearsal mark.
///
/// From lytk-py's `RehearsalMark`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RehearsalMark {
    pub text: String,
}

/// An ottava (octave shift) indication.
///
/// From lytk-py's `OctaveShift`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OctaveShift {
    pub shift_type: String,
    pub size: i8,
}

/// A pedal marking.
///
/// From lytk-py's `PedalEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PedalEvent {
    pub pedal_type: String,
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
    pub offset: i32,
    pub placement: Placement,
    pub tempo: Option<TempoDirection>,
    pub text: Option<TextDirection>,
    pub rehearsal: Option<RehearsalMark>,
    pub octave_shift: Option<OctaveShift>,
    pub pedal: Option<PedalEvent>,
    pub dynamic: Option<super::articulation::DynamicMark>,
    pub wedge: Option<super::articulation::Wedge>,
    /// Coda sign.
    pub coda: bool,
    /// Segno sign.
    pub segno: bool,
    /// Da Capo text (e.g. "D.C.", "D.C. al Fine").
    pub da_capo: Option<String>,
    /// Dal Segno text (e.g. "D.S.", "D.S. al Coda").
    pub dal_segno: Option<String>,
    /// Layout break at this position.
    pub layout_break: Option<LayoutBreakType>,
    /// Mid-part instrument change.
    pub instrument_change: Option<InstrumentRef>,
}

impl Default for Direction {
    fn default() -> Self {
        Self {
            offset: 0,
            placement: Placement::Unspecified,
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
        }
    }
}
