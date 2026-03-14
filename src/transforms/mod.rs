//! Transform passes on the IR.
//!
//! Each transform is **idempotent** and **composable**. Transforms borrow
//! the input [`Score`] and return a new owned [`Score`] — the original is
//! never mutated.
//!
//! Both OOP-style (`Transform::apply(&self, &Score) -> Score`) and functional
//! helpers (`transpose(&Score, semitones) -> Score`) are provided.
//!
//! # Conventions
//! See `.github/instructions/transform-conventions.instructions.md`.

use crate::ir::score::Score;

// Sub-modules for individual transforms will go here:
// pub mod transpose;
// pub mod language;

// ---------------------------------------------------------------------------
// Transform trait
// ---------------------------------------------------------------------------

/// A composable, idempotent transformation on the IR.
///
/// Implementations must:
/// - Not mutate the input `Score`.
/// - Return a new `Score` (clone/copy-on-write).
/// - Be idempotent: `T(T(x)) == T(x)` (or `T(T(x)) == x` for self-inverse
///   transforms like retrograde — document which).
pub trait Transform {
    /// Apply this transform to a score, returning a new score.
    fn apply(&self, score: &Score) -> Score;
}

/// Apply a sequence of transforms in order.
pub fn apply_all(score: &Score, transforms: &[&dyn Transform]) -> Score {
    let mut result = score.clone();
    for t in transforms {
        result = t.apply(&result);
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::score::Score;

    /// Identity transform — useful as a baseline for testing the trait.
    struct Identity;
    impl Transform for Identity {
        fn apply(&self, score: &Score) -> Score {
            score.clone()
        }
    }

    #[test]
    fn identity_is_idempotent() {
        let score = Score::new();
        let once = Identity.apply(&score);
        let twice = Identity.apply(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn apply_all_composes() {
        let score = Score::new();
        let result = apply_all(&score, &[&Identity, &Identity]);
        assert_eq!(result, score);
    }
}
