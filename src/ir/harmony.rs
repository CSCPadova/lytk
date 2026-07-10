//! Harmony (chord symbol) and figured bass IR types.
//!
//! These represent chord symbols (`Cmaj7`, `Dm/F`) and figured bass notation
//! at the measure level, parallel to notes.

use serde::{Deserialize, Serialize};

use super::duration::Duration;

/// A pitch used in chord symbol descriptions (root or bass).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordPitch {
    /// Note step: C, D, E, F, G, A, B.
    pub step: String,
    /// Chromatic alteration in semitones (-2.0 to 2.0).
    pub alter: f64,
}

/// A chord degree modification (add, subtract, or alter a scale degree).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordDegree {
    /// Scale degree number (1–13).
    pub value: u8,
    /// Alteration in semitones (-2.0 to 2.0).
    pub alter: f64,
    /// Type: "add", "subtract", or "alter".
    pub degree_type: String,
}

/// A harmony / chord symbol.
///
/// Corresponds to MusicXML `<harmony>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Harmony {
    /// Root pitch of the chord.
    pub root: ChordPitch,
    /// Chord quality: "major", "minor", "dominant", "diminished",
    /// "augmented", "half-diminished", "major-seventh", etc.
    pub kind: String,
    /// Optional bass note for inversions (e.g. C/E).
    pub bass: Option<ChordPitch>,
    /// Degree modifications.
    pub degrees: Vec<ChordDegree>,
    /// Position in the measure (offset from measure start in divisions).
    pub offset: i32,
    /// Optional functional-harmony Roman numeral (MusicXML `<function>`, e.g.
    /// `"V"`, `"ii"`). Supplements the chord symbol; `None` for a plain chord
    /// symbol. Omitted from serialization when absent for JSON back-compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

/// A single figure in a figured bass indication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Figure {
    /// The interval number (e.g. 6, 4, 3). None for an empty figure slot.
    pub number: Option<u8>,
    /// Prefix accidental: "sharp", "flat", "natural", "double-sharp", etc.
    pub prefix: Option<String>,
    /// Suffix accidental.
    pub suffix: Option<String>,
}

/// A figured bass indication.
///
/// Corresponds to MusicXML `<figured-bass>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FiguredBass {
    /// Individual figures (e.g. [6, 4] for a 6/4 chord).
    pub figures: Vec<Figure>,
    /// Duration of the figured bass indication.
    pub duration: Duration,
    /// Whether figures are enclosed in parentheses.
    pub parentheses: bool,
    /// Position in the measure (offset from measure start in divisions).
    pub offset: i32,
}
