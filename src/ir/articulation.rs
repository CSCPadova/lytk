//! Articulation, ornament, and notation marking value objects.
//!
//! All types here are small value objects (not tree nodes). They are stored
//! as `Vec` fields on [`Note`](super::note::Note).
//!
//! # Influences
//! - Frozen dataclasses pattern from lytk-py (`lytk-py/ir/articulation.py`).
//! - Annotation typed-payload model from PDMX (`PDMX/reading/classes.py`).

use serde::{Deserialize, Serialize};

/// Placement hint for a notation marking.
///
/// From lytk-py's `Placement` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Placement {
    Above,
    Below,
    #[default]
    Unspecified,
}

/// Start/stop/continue state for spanning notations.
///
/// From lytk-py's `StartStop` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StartStop {
    Start,
    Stop,
    Continue,
}

/// An articulation marking (e.g. staccato, accent, tenuto).
///
/// From lytk-py's `Articulation` frozen dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Articulation {
    pub name: String,
    pub placement: Placement,
}

/// An ornament (e.g. trill, mordent, turn).
///
/// From lytk-py's `Ornament`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ornament {
    pub name: String,
    pub placement: Placement,
}

/// A technical indication (e.g. fingering, string number).
///
/// From lytk-py's `Technical`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Technical {
    pub name: String,
    pub value: String,
}

/// A slur event (start/stop of a slur).
///
/// From lytk-py's `SlurEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlurEvent {
    pub slur_type: StartStop,
    pub number: u8,
    pub placement: Placement,
}

/// A tie event (start/stop of a tie).
///
/// From lytk-py's `TieEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TieEvent {
    pub tie_type: StartStop,
}

/// A dynamic marking (e.g. pp, mf, ff).
///
/// From lytk-py's `DynamicMark`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DynamicMark {
    pub sign: String,
    pub placement: Placement,
}

/// A crescendo/decrescendo hairpin.
///
/// From lytk-py's `Wedge`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wedge {
    pub wedge_type: String,
    pub placement: Placement,
}

/// A beam event.
///
/// From lytk-py's `BeamEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BeamEvent {
    pub beam_type: String,
    pub number: u8,
}

/// Tuplet display hint.
///
/// From lytk-py's `TupletDisplay`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TupletDisplay {
    pub tuplet_type: StartStop,
    pub bracket: bool,
    pub show_number: String,
}

/// A fermata.
///
/// From lytk-py's `Fermata`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fermata {
    pub shape: String,
    pub inverted: bool,
}

/// A lyric syllable attached to a note.
///
/// From lytk-py's `LyricSyllable`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LyricSyllable {
    pub text: String,
    pub syllabic: SyllabicType,
    pub number: u8,
    pub extend: bool,
    pub elision: bool,
}

/// Syllable position within a word.
///
/// From lytk-py's `SyllabicType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SyllabicType {
    #[default]
    Single,
    Begin,
    End,
    Middle,
}
