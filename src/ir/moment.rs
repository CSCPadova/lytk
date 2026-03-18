//! Moment — a position in time with separate main and grace components.
//!
//! Inspired by LilyPond's `Moment` class which uses a dual-part time system:
//! the main timeline for regular notes and a separate grace timeline for
//! grace notes that exist "between" main beats.

use super::duration::Frac;
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Sub};

/// A position in musical time with main and grace components.
///
/// Grace notes have a negative `grace` offset, placing them just before
/// the next main beat. Regular notes have `grace == 0`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Moment {
    /// Position on the main timeline (in whole-note fractions).
    pub main: Frac,
    /// Grace note offset (normally 0; negative for grace notes).
    pub grace: Frac,
}

impl Moment {
    /// A moment at time zero.
    pub const ZERO: Moment = Moment {
        main: Frac::new_raw(0, 1),
        grace: Frac::new_raw(0, 1),
    };

    /// Create a new moment with only a main component.
    pub fn new(main: Frac) -> Self {
        Self {
            main,
            grace: Frac::from_integer(0),
        }
    }

    /// Create a moment with both main and grace components.
    pub fn with_grace(main: Frac, grace: Frac) -> Self {
        Self { main, grace }
    }

    /// True if this moment is at time zero (no main or grace offset).
    pub fn is_zero(&self) -> bool {
        self.main == Frac::from_integer(0) && self.grace == Frac::from_integer(0)
    }

    /// The main-part duration only (ignoring grace).
    pub fn main_part(&self) -> Frac {
        self.main
    }
}

impl Default for Moment {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Debug for Moment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.grace == Frac::from_integer(0) {
            write!(f, "Moment({})", self.main)
        } else {
            write!(f, "Moment({}, grace={})", self.main, self.grace)
        }
    }
}

impl fmt::Display for Moment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.grace == Frac::from_integer(0) {
            write!(f, "{}", self.main)
        } else {
            write!(f, "{}g{}", self.main, self.grace)
        }
    }
}

impl Ord for Moment {
    fn cmp(&self, other: &Self) -> Ordering {
        self.main
            .cmp(&other.main)
            .then(self.grace.cmp(&other.grace))
    }
}

impl PartialOrd for Moment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Moment {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            main: self.main + rhs.main,
            grace: self.grace + rhs.grace,
        }
    }
}

impl AddAssign for Moment {
    fn add_assign(&mut self, rhs: Self) {
        self.main = self.main + rhs.main;
        self.grace = self.grace + rhs.grace;
    }
}

impl Sub for Moment {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            main: self.main - rhs.main,
            grace: self.grace - rhs.grace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moment_zero() {
        let m = Moment::ZERO;
        assert!(m.is_zero());
        assert_eq!(m.main, Frac::from_integer(0));
        assert_eq!(m.grace, Frac::from_integer(0));
    }

    #[test]
    fn test_moment_new() {
        let m = Moment::new(Frac::new(1, 4));
        assert_eq!(m.main, Frac::new(1, 4));
        assert_eq!(m.grace, Frac::from_integer(0));
        assert!(!m.is_zero());
    }

    #[test]
    fn test_moment_add() {
        let a = Moment::new(Frac::new(1, 4));
        let b = Moment::new(Frac::new(1, 4));
        let c = a + b;
        assert_eq!(c.main, Frac::new(1, 2));
    }

    #[test]
    fn test_moment_ordering() {
        let a = Moment::new(Frac::new(1, 4));
        let b = Moment::new(Frac::new(1, 2));
        assert!(a < b);

        // Same main, grace determines order
        let c = Moment::with_grace(Frac::new(1, 4), Frac::new(-1, 8));
        let d = Moment::new(Frac::new(1, 4));
        assert!(c < d); // grace notes come before main beats
    }

    #[test]
    fn test_moment_sub() {
        let a = Moment::new(Frac::new(3, 4));
        let b = Moment::new(Frac::new(1, 4));
        let c = a - b;
        assert_eq!(c.main, Frac::new(1, 2));
    }
}
