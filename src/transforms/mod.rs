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

use crate::ir::music::MusicDocument;
use crate::ir::score::Score;

pub mod invert;
pub mod language;
pub mod retrograde;
pub mod transpose;

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
// MusicTransform trait (Layer 1)
// ---------------------------------------------------------------------------

/// A composable, idempotent transformation on the Music tree (Layer 1).
///
/// Like [`Transform`] but operates on `MusicDocument` instead of `Score`.
pub trait MusicTransform {
    /// Apply this transform to a music document, returning a new document.
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument;
}

/// Apply a sequence of music transforms in order.
pub fn apply_all_music(doc: &MusicDocument, transforms: &[&dyn MusicTransform]) -> MusicDocument {
    let mut result = doc.clone();
    for t in transforms {
        result = t.apply_music(&result);
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::music::{Music, MusicDocument};
    use crate::ir::score::Score;

    /// Identity transform — useful as a baseline for testing the trait.
    struct Identity;
    impl Transform for Identity {
        fn apply(&self, score: &Score) -> Score {
            score.clone()
        }
    }
    impl MusicTransform for Identity {
        fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
            doc.clone()
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

    #[test]
    fn music_identity_is_idempotent() {
        let doc = MusicDocument::new(Music::empty());
        let once = Identity.apply_music(&doc);
        let twice = Identity.apply_music(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn apply_all_music_composes() {
        let doc = MusicDocument::new(Music::empty());
        let result = apply_all_music(&doc, &[&Identity, &Identity]);
        assert_eq!(result, doc);
    }
}
