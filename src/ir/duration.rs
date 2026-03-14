//! Duration representation.
//!
//! A [`Duration`] encodes note length as a fraction of a whole note,
//! with optional dots and tuplet scaling.
//!
//! # Influences
//! - Fraction-based `base` (whole note = 1) from quickly (`quickly/duration.py`).
//! - `(log, dotcount)` as a display form from quickly's `log_dotcount()`.
//! - Factory methods `from_musicxml_type` / `from_lilypond_number` from lytk-py
//!   (`lytk-py/ir/duration.py`).
//! - Tuplet scaling as `normal/actual` ratio from lytk-py.

use num::rational::Ratio;
use serde::{Deserialize, Serialize};

/// Fractional duration type (fraction of a whole note).
pub type Frac = Ratio<i64>;

/// A duration value object.
///
/// `base` is measured as a fraction of a whole note (quarter = 1/4).
/// Dots and tuplet scaling modify the sounding duration via [`actual_duration`](Duration::actual_duration).
///
/// This is a pure value type — cheap to clone, compare, and serialize.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Duration {
    /// Base duration as a fraction of a whole note.
    pub base: Frac,
    /// Number of augmentation dots.
    pub dots: u8,
    /// Tuplet normal (e.g. 3 for a triplet "3 in the time of 2").
    pub tuplet_normal: u8,
    /// Tuplet actual (e.g. 2 for a triplet "3 in the time of 2").
    pub tuplet_actual: u8,
}

impl Duration {
    /// Create a simple duration with no dots or tuplet.
    pub fn new(base: Frac) -> Self {
        Self {
            base,
            dots: 0,
            tuplet_normal: 1,
            tuplet_actual: 1,
        }
    }

    /// Create a dotted duration.
    pub fn dotted(base: Frac, dots: u8) -> Self {
        Self {
            base,
            dots,
            tuplet_normal: 1,
            tuplet_actual: 1,
        }
    }

    /// The actual sounding duration accounting for dots and tuplet scaling.
    ///
    /// Algorithm from lytk-py's `Duration.actual_duration` and quickly's
    /// `duration()` function.
    pub fn actual_duration(&self) -> Frac {
        // Dot formula: base * (2 - 1/2^dots)
        let dot_multiplier = Frac::from_integer(2) - Frac::new(1, 1_i64 << self.dots as u32);
        let dotted = self.base * dot_multiplier;
        // Tuplet scaling
        dotted * Frac::new(self.tuplet_normal as i64, self.tuplet_actual as i64)
    }

    /// LilyPond duration number (1 = whole, 2 = half, 4 = quarter, …).
    ///
    /// Mapping from quickly's `log_dotcount()`: the LilyPond number is
    /// `1 / base` for standard durations. Returns `None` for non-power-of-two
    /// bases (breve, longa, maxima use 0, −1, −2 which we don't encode as ints).
    pub fn lilypond_log(&self) -> Option<i32> {
        // log2(1/base) — e.g. quarter (1/4) → log2(4) = 2
        let recip = self.base.recip();
        if *recip.denom() != 1 {
            return None; // not a power-of-two
        }
        let n = *recip.numer();
        if n > 0 && (n as u64).is_power_of_two() {
            Some((n as u64).trailing_zeros() as i32)
        } else {
            None
        }
    }

    // -- Factory methods --

    /// Create from a MusicXML duration type name.
    ///
    /// Names follow the MusicXML `note-type-value` enumeration.
    /// Mapping from lytk-py's `DURATION_TYPE_MAP`.
    pub fn from_musicxml_type(type_name: &str, dots: u8) -> Option<Self> {
        let base = match type_name {
            "maxima" => Frac::from_integer(8),
            "long" => Frac::from_integer(4),
            "breve" => Frac::from_integer(2),
            "whole" => Frac::from_integer(1),
            "half" => Frac::new(1, 2),
            "quarter" => Frac::new(1, 4),
            "eighth" => Frac::new(1, 8),
            "16th" => Frac::new(1, 16),
            "32nd" => Frac::new(1, 32),
            "64th" => Frac::new(1, 64),
            "128th" => Frac::new(1, 128),
            "256th" => Frac::new(1, 256),
            "512th" => Frac::new(1, 512),
            "1024th" => Frac::new(1, 1024),
            _ => return None,
        };
        Some(Self {
            base,
            dots,
            tuplet_normal: 1,
            tuplet_actual: 1,
        })
    }

    /// Create from a LilyPond duration number (1, 2, 4, 8, …).
    ///
    /// `0` means breve in LilyPond notation.
    /// Mapping from lytk-py's `_LY_NUM_TO_FRACTION`.
    pub fn from_lilypond_number(number: u32, dots: u8) -> Option<Self> {
        let base = if number == 0 {
            Frac::from_integer(2) // breve
        } else if number.is_power_of_two() {
            Frac::new(1, number as i64)
        } else {
            return None;
        };
        Some(Self {
            base,
            dots,
            tuplet_normal: 1,
            tuplet_actual: 1,
        })
    }

    /// Create from MusicXML `<duration>` divisions and optional type hint.
    ///
    /// `duration_value` is the raw `<duration>` element value.
    /// `divisions` is the current `<divisions>` (ticks per quarter note).
    pub fn from_divisions(duration_value: i64, divisions: i64, dots: u8) -> Self {
        let base = Frac::new(duration_value, divisions * 4);
        Self {
            base,
            dots,
            tuplet_normal: 1,
            tuplet_actual: 1,
        }
    }

    // -- Common constants --

    pub fn whole() -> Self {
        Self::new(Frac::from_integer(1))
    }
    pub fn half() -> Self {
        Self::new(Frac::new(1, 2))
    }
    pub fn quarter() -> Self {
        Self::new(Frac::new(1, 4))
    }
    pub fn eighth() -> Self {
        Self::new(Frac::new(1, 8))
    }
    pub fn sixteenth() -> Self {
        Self::new(Frac::new(1, 16))
    }
}

impl Default for Duration {
    /// Quarter note.
    fn default() -> Self {
        Self::quarter()
    }
}

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Try to display as a LilyPond-style number
        if let Some(log) = self.lilypond_log() {
            let num = 1_u32 << log as u32;
            write!(f, "{num}")?;
        } else if self.base == Frac::from_integer(2) {
            write!(f, "\\breve")?;
        } else if self.base == Frac::from_integer(4) {
            write!(f, "\\longa")?;
        } else {
            write!(f, "{}/{}", self.base.numer(), self.base.denom())?;
        }
        for _ in 0..self.dots {
            write!(f, ".")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quarter() {
        let q = Duration::quarter();
        assert_eq!(q.actual_duration(), Frac::new(1, 4));
        assert_eq!(q.lilypond_log(), Some(2));
    }

    #[test]
    fn test_dotted_half() {
        let dh = Duration::dotted(Frac::new(1, 2), 1);
        assert_eq!(dh.actual_duration(), Frac::new(3, 4));
    }

    #[test]
    fn test_triplet() {
        let mut t = Duration::quarter();
        t.tuplet_actual = 3;
        t.tuplet_normal = 2;
        // quarter note in triplet = 1/4 * 2/3 = 1/6
        assert_eq!(t.actual_duration(), Frac::new(1, 6));
    }

    #[test]
    fn test_from_musicxml_type() {
        let d = Duration::from_musicxml_type("eighth", 0).unwrap();
        assert_eq!(d.base, Frac::new(1, 8));
    }

    #[test]
    fn test_from_lilypond_number() {
        let d = Duration::from_lilypond_number(8, 1).unwrap();
        assert_eq!(d.base, Frac::new(1, 8));
        assert_eq!(d.dots, 1);
        // dotted eighth = 3/16
        assert_eq!(d.actual_duration(), Frac::new(3, 16));
    }

    #[test]
    fn test_from_divisions() {
        // quarter note with divisions=1
        let d = Duration::from_divisions(1, 1, 0);
        assert_eq!(d.base, Frac::new(1, 4));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Duration::quarter()), "4");
        assert_eq!(format!("{}", Duration::half()), "2");
        assert_eq!(format!("{}", Duration::dotted(Frac::new(1, 4), 1)), "4.");
    }
}
