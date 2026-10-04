//! Articulation, ornament, and notation marking value objects.
//!
//! All types here are small value objects (not tree nodes). They are stored
//! as `Vec` fields on [`Note`](super::note::Note).
//!
//! # Influences
//! - Frozen dataclasses pattern from lytk-py (`lytk-py/ir/articulation.py`).
//! - Annotation typed-payload model from PDMX (`PDMX/reading/classes.py`).

use super::serde_defaults::{is_default, is_one, one};
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

open_named_enum! {
    /// What an articulation is, by its MusicXML name (`<articulations>`).
    pub enum ArticulationType {
        Accent => "accent",
        StrongAccent => "strong-accent",
        Staccato => "staccato",
        Tenuto => "tenuto",
        DetachedLegato => "detached-legato",
        Staccatissimo => "staccatissimo",
        Spiccato => "spiccato",
        Scoop => "scoop",
        Plop => "plop",
        Doit => "doit",
        Falloff => "falloff",
        BreathMark => "breath-mark",
        Caesura => "caesura",
        Stress => "stress",
        Unstress => "unstress",
        SoftAccent => "soft-accent",
    }
}

open_named_enum! {
    /// What an ornament is, by its MusicXML name (`<ornaments>`; a wavy
    /// line with its start, stop or continuation).
    pub enum OrnamentType {
        TrillMark => "trill-mark",
        Mordent => "mordent",
        InvertedMordent => "inverted-mordent",
        Turn => "turn",
        InvertedTurn => "inverted-turn",
        DelayedTurn => "delayed-turn",
        DelayedInvertedTurn => "delayed-inverted-turn",
        Shake => "shake",
        Schleifer => "schleifer",
        Haydn => "haydn",
        Tremolo => "tremolo",
        WavyLineStart => "wavy-line-start",
        WavyLineStop => "wavy-line-stop",
        WavyLineContinue => "wavy-line-continue",
    }
}

open_named_enum! {
    /// What a technical indication is, by its MusicXML name (`<technical>`).
    pub enum TechnicalType {
        UpBow => "up-bow",
        DownBow => "down-bow",
        Harmonic => "harmonic",
        OpenString => "open-string",
        Stopped => "stopped",
        SnapPizzicato => "snap-pizzicato",
        Fingering => "fingering",
        Fret => "fret",
        String => "string",
    }
}

open_named_enum! {
    /// A dynamic, by its text: MusicXML's `<dynamics>` names and LilyPond's
    /// other predefined ones; any other text (LilyPond's
    /// `make-dynamic-script`, MusicXML's `<other-dynamics>`) as written.
    pub enum DynamicType {
        P => "p",
        Pp => "pp",
        Ppp => "ppp",
        Pppp => "pppp",
        Ppppp => "ppppp",
        Pppppp => "pppppp",
        F => "f",
        Ff => "ff",
        Fff => "fff",
        Ffff => "ffff",
        Fffff => "fffff",
        Ffffff => "ffffff",
        Mp => "mp",
        Mf => "mf",
        Sf => "sf",
        Sfp => "sfp",
        Sfpp => "sfpp",
        Fp => "fp",
        Rf => "rf",
        Rfz => "rfz",
        Sfz => "sfz",
        Sffz => "sffz",
        Sff => "sff",
        Fz => "fz",
        N => "n",
        Pf => "pf",
        Sfzp => "sfzp",
        Sp => "sp",
        Spp => "spp",
    }
}

/// An articulation marking (e.g. staccato, accent, tenuto).
///
/// From lytk-py's `Articulation` frozen dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Articulation {
    pub name: ArticulationType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
}

/// An ornament (e.g. trill, mordent, turn).
///
/// From lytk-py's `Ornament`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ornament {
    pub name: OrnamentType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
}

/// A technical indication (e.g. fingering, string number).
///
/// From lytk-py's `Technical`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Technical {
    pub name: TechnicalType,
    /// The text of a fingering, the number of a fret or string; else empty.
    #[serde(default, skip_serializing_if = "is_default")]
    pub value: String,
}

named_enum! {
    /// A beam's state at a note (MusicXML's `<beam>`).
    pub enum BeamValue {
        Begin => "begin",
        Continue => "continue",
        End => "end",
        ForwardHook => "forward hook",
        BackwardHook => "backward hook",
    }
}

named_enum! {
    /// A hairpin's start, which way, or its end (MusicXML's `<wedge>`).
    pub enum WedgeType {
        Crescendo => "crescendo",
        Diminuendo => "diminuendo",
        Stop => "stop",
        Continue => "continue",
    }
}

named_enum! {
    /// Which number a tuplet shows (MusicXML's `show-number`).
    pub enum ShowNumber {
        Actual => "actual",
        Both => "both",
        NoNumber => "none",
    }
}

named_enum! {
    /// A fermata's shape (MusicXML's `<fermata>`).
    pub enum FermataShape {
        Normal => "normal",
        Angled => "angled",
        Square => "square",
        DoubleAngled => "double-angled",
        DoubleSquare => "double-square",
        DoubleDot => "double-dot",
        HalfCurve => "half-curve",
        Curlew => "curlew",
    }
}

/// A slur event (start/stop of a slur).
///
/// From lytk-py's `SlurEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlurEvent {
    pub slur_type: StartStop,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub number: u8,
    #[serde(default, skip_serializing_if = "is_default")]
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
    pub sign: DynamicType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
}

/// A crescendo/decrescendo hairpin.
///
/// From lytk-py's `Wedge`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wedge {
    pub wedge_type: WedgeType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub placement: Placement,
}

/// A beam event.
///
/// From lytk-py's `BeamEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BeamEvent {
    pub beam_type: BeamValue,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub number: u8,
}

/// Tuplet display hint.
///
/// From lytk-py's `TupletDisplay`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TupletDisplay {
    pub tuplet_type: StartStop,
    #[serde(default, skip_serializing_if = "is_default")]
    pub bracket: bool,
    /// The number shown, when the source says.
    #[serde(default, skip_serializing_if = "is_default")]
    pub show_number: Option<ShowNumber>,
}

/// A fermata.
///
/// From lytk-py's `Fermata`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fermata {
    pub shape: FermataShape,
    #[serde(default, skip_serializing_if = "is_default")]
    pub inverted: bool,
}

/// A lyric syllable attached to a note.
///
/// `number` is the verse (1, 2, …), `name` what the source calls it (a
/// LilyPond stanza, a MusicXML `name` or non-numeric `number`). Words sung
/// on one note (`my‿a`) are one syllable, joined with U+203F and `elision`
/// set.
///
/// From lytk-py's `LyricSyllable`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct LyricSyllable {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub syllabic: SyllabicType,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub number: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub extend: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub elision: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub name: Option<String>,
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
