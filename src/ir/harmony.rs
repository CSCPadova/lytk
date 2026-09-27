//! Harmony (chord symbol) and figured bass IR types.
//!
//! These represent chord symbols (`Cmaj7`, `Dm/F`) and figured bass notation
//! at the measure level, parallel to notes.

use serde::{Deserialize, Serialize};

use super::duration::Duration;

/// The MusicXML kind of a chord symbol's quality suffix: LilyPond's
/// (`m`, `maj7`, `m7.5-`) and the usual lead-sheet spellings (`M7`, `-7`,
/// `°`, `ø`, `+`, `m7b5`). An unknown suffix reads as major.
pub fn kind_from_suffix(suffix: &str) -> &'static str {
    match suffix {
        "" | "M" | "maj" | "Maj" | "major" => "major",
        "m" | "min" | "mi" | "-" => "minor",
        "7" | "dom" | "dom7" => "dominant",
        "maj7" | "major7" | "M7" | "Maj7" | "ma7" | "Δ" | "Δ7" => "major-seventh",
        "m7" | "min7" | "mi7" | "-7" => "minor-seventh",
        "dim" | "°" | "o" => "diminished",
        "dim7" | "°7" | "o7" => "diminished-seventh",
        "aug" | "+" | "+5" => "augmented",
        "m7.5-" | "m7-5" | "dim5m7" | "m7b5" | "mi7b5" | "-7b5" | "ø" | "ø7" => "half-diminished",
        "6" | "maj6" => "major-sixth",
        "m6" | "min6" | "-6" => "minor-sixth",
        "9" => "dominant-ninth",
        "maj9" | "M9" => "major-ninth",
        "m9" | "min9" | "-9" => "minor-ninth",
        "11" => "dominant-11th",
        "13" => "dominant-13th",
        "sus2" => "suspended-second",
        "sus4" | "sus" => "suspended-fourth",
        "5" => "power",
        _ => "major",
    }
}

/// The lead-sheet suffix of a MusicXML chord kind (`minor-seventh` → `m7`),
/// the inverse of [`kind_from_suffix`] for the kinds it reads.
pub fn suffix_of_kind(kind: &str) -> &'static str {
    match kind {
        "minor" => "m",
        "dominant" => "7",
        "major-seventh" => "maj7",
        "minor-seventh" => "m7",
        "diminished" => "dim",
        "diminished-seventh" => "dim7",
        "augmented" => "aug",
        "half-diminished" => "m7b5",
        "major-sixth" => "6",
        "minor-sixth" => "m6",
        "dominant-ninth" => "9",
        "major-ninth" => "maj9",
        "minor-ninth" => "m9",
        "dominant-11th" => "11",
        "dominant-13th" => "13",
        "suspended-second" => "sus2",
        "suspended-fourth" => "sus4",
        "power" => "5",
        _ => "",
    }
}

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

#[cfg(test)]
mod suffix_tests {
    use super::*;

    #[test]
    fn suffixes_round_trip() {
        for kind in [
            "major",
            "minor",
            "dominant",
            "major-seventh",
            "minor-seventh",
            "diminished",
            "diminished-seventh",
            "augmented",
            "half-diminished",
            "major-sixth",
            "minor-sixth",
            "dominant-ninth",
            "major-ninth",
            "minor-ninth",
            "dominant-11th",
            "dominant-13th",
            "suspended-second",
            "suspended-fourth",
            "power",
        ] {
            assert_eq!(kind_from_suffix(suffix_of_kind(kind)), kind);
        }
        assert_eq!(kind_from_suffix("-7"), "minor-seventh");
        assert_eq!(kind_from_suffix("ø"), "half-diminished");
    }
}
