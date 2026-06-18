//! Musical intervals for spelling-correct (diatonic) transposition.
//!
//! An [`Interval`] is stored as a signed `(diatonic, chromatic)` pair — how many
//! staff positions and how many semitones to move. This is the representation
//! transposition needs to keep enharmonic spelling correct: a major third up is
//! `(diatonic 2, chromatic 4)` while a diminished fourth up is
//! `(diatonic 3, chromatic 4)` — the same sounding pitch, spelled differently.
//!
//! # Influences
//! Generic-interval arithmetic mirrors music21's `interval` module and quickly's
//! transposer (diatonic step + chromatic remainder), kept minimal here.

use crate::ir::pitch::Pitch;

/// A directed musical interval as a `(diatonic, chromatic)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    /// Signed diatonic step count: `0` = unison, `+2` = a third up, `−4` = a fifth down.
    pub diatonic: i32,
    /// Signed chromatic semitones.
    pub chromatic: i32,
}

impl Interval {
    pub const fn new(diatonic: i32, chromatic: i32) -> Self {
        Self {
            diatonic,
            chromatic,
        }
    }

    /// The directed interval from `from` up to `to`.
    pub fn between(from: &Pitch, to: &Pitch) -> Self {
        let diatonic = (to.step.index() + 7 * to.octave) - (from.step.index() + 7 * from.octave);
        let chromatic = to.midi_number() - from.midi_number();
        Self {
            diatonic,
            chromatic,
        }
    }

    /// Reverse the interval's direction.
    pub fn inverted(self) -> Self {
        Self {
            diatonic: -self.diatonic,
            chromatic: -self.chromatic,
        }
    }

    /// Parse an interval name such as `M3`, `m3`, `P5`, `A4`, `d5`, `P8`, `M10`.
    ///
    /// A leading `-` makes it descending (`-m3`). Qualities: `P` perfect,
    /// `M` major, `m` minor, `A` augmented, `d` diminished — repeat `A`/`d` for
    /// doubly augmented/diminished (`AA4`, `dd5`).
    pub fn from_name(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (down, rest) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s),
        };
        let split = rest
            .find(|c: char| c.is_ascii_digit())
            .ok_or_else(|| format!("interval `{s}` has no number"))?;
        let (qual, num_str) = rest.split_at(split);
        let number: i32 = num_str
            .parse()
            .map_err(|_| format!("interval `{s}` has an invalid number"))?;
        if number < 1 {
            return Err(format!("interval number must be ≥ 1 in `{s}`"));
        }
        let (base, perfect) = base_semitones(number);
        let adjust = quality_adjust(qual, perfect)
            .ok_or_else(|| format!("invalid quality `{qual}` for interval `{s}`"))?;
        let chromatic = base + adjust;
        let diatonic = number - 1;
        let sign = if down { -1 } else { 1 };
        Ok(Self {
            diatonic: sign * diatonic,
            chromatic: sign * chromatic,
        })
    }
}

/// Base (perfect/major) semitone size of a generic interval number, plus whether
/// the number is in the perfect class (unison / 4th / 5th / octave and compounds).
fn base_semitones(number: i32) -> (i32, bool) {
    let n0 = (number - 1).rem_euclid(7);
    let octaves = (number - 1) / 7;
    let (base, perfect) = match n0 {
        0 => (0, true),
        1 => (2, false),
        2 => (4, false),
        3 => (5, true),
        4 => (7, true),
        5 => (9, false),
        6 => (11, false),
        _ => unreachable!(),
    };
    (base + 12 * octaves, perfect)
}

/// Semitone adjustment for an interval-quality string given its class. Returns
/// `None` if the quality is invalid for the class (e.g. `M5`, `P3`).
fn quality_adjust(qual: &str, perfect: bool) -> Option<i32> {
    match qual {
        "P" => perfect.then_some(0),
        "M" => (!perfect).then_some(0),
        "m" => (!perfect).then_some(-1),
        q if !q.is_empty() && q.bytes().all(|b| b == b'A') => Some(q.len() as i32),
        q if !q.is_empty() && q.bytes().all(|b| b == b'd') => {
            let k = q.len() as i32;
            Some(if perfect { -k } else { -k - 1 })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::pitch::{Pitch, PitchStep};

    #[test]
    fn parse_common_intervals() {
        assert_eq!(Interval::from_name("P1").unwrap(), Interval::new(0, 0));
        assert_eq!(Interval::from_name("m2").unwrap(), Interval::new(1, 1));
        assert_eq!(Interval::from_name("M2").unwrap(), Interval::new(1, 2));
        assert_eq!(Interval::from_name("m3").unwrap(), Interval::new(2, 3));
        assert_eq!(Interval::from_name("M3").unwrap(), Interval::new(2, 4));
        assert_eq!(Interval::from_name("P4").unwrap(), Interval::new(3, 5));
        assert_eq!(Interval::from_name("A4").unwrap(), Interval::new(3, 6));
        assert_eq!(Interval::from_name("d5").unwrap(), Interval::new(4, 6));
        assert_eq!(Interval::from_name("P5").unwrap(), Interval::new(4, 7));
        assert_eq!(Interval::from_name("M7").unwrap(), Interval::new(6, 11));
        assert_eq!(Interval::from_name("P8").unwrap(), Interval::new(7, 12));
        assert_eq!(Interval::from_name("M10").unwrap(), Interval::new(9, 16));
    }

    #[test]
    fn parse_augmented_diminished_and_descending() {
        assert_eq!(Interval::from_name("A1").unwrap(), Interval::new(0, 1));
        assert_eq!(Interval::from_name("d1").unwrap(), Interval::new(0, -1));
        assert_eq!(Interval::from_name("AA4").unwrap(), Interval::new(3, 7));
        assert_eq!(Interval::from_name("dd5").unwrap(), Interval::new(4, 5));
        assert_eq!(Interval::from_name("-m3").unwrap(), Interval::new(-2, -3));
        assert_eq!(Interval::from_name("-P5").unwrap(), Interval::new(-4, -7));
    }

    #[test]
    fn invalid_qualities_rejected() {
        assert!(Interval::from_name("P3").is_err()); // 3rd is not perfect-class
        assert!(Interval::from_name("M5").is_err()); // 5th is not major-class
        assert!(Interval::from_name("X3").is_err());
        assert!(Interval::from_name("M").is_err()); // no number
    }

    #[test]
    fn between_matches_names() {
        let c4 = Pitch::new(PitchStep::C, 4);
        let e4 = Pitch::new(PitchStep::E, 4);
        let g4 = Pitch::new(PitchStep::G, 4);
        assert_eq!(Interval::between(&c4, &e4), Interval::new(2, 4)); // M3
        assert_eq!(Interval::between(&c4, &g4), Interval::new(4, 7)); // P5
        assert_eq!(Interval::between(&e4, &c4), Interval::new(-2, -4)); // down M3
    }
}
