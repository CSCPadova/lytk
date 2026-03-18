//! ChangeLanguage transform — switch the pitch language used for LilyPond output.
//!
//! Since pitches are stored abstractly as (`PitchStep`, `Alter`, `octave`),
//! changing the language only requires updating `ScoreMetadata.pitch_language`.
//! The language is applied at emission time by the IR→LilyPond adapter.
//!
//! # Idempotency
//! `ChangeLanguage(L)(ChangeLanguage(L)(x)) == ChangeLanguage(L)(x)` — idempotent.

use crate::ir::language::PitchLanguage;
use crate::ir::music::MusicDocument;
use crate::ir::score::Score;

use super::{MusicTransform, Transform};

/// Change the pitch language stored in score metadata.
///
/// This affects how note names are rendered when emitting LilyPond. The
/// abstract pitch representation (step + alter) remains unchanged.
pub struct ChangeLanguage {
    pub target: PitchLanguage,
}

impl ChangeLanguage {
    pub fn new(target: PitchLanguage) -> Self {
        Self { target }
    }
}

impl Transform for ChangeLanguage {
    fn apply(&self, score: &Score) -> Score {
        let mut result = score.clone();
        result.metadata.pitch_language = Some(self.target);
        result
    }
}

impl MusicTransform for ChangeLanguage {
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
        let mut result = doc.clone();
        result.metadata.pitch_language = Some(self.target);
        result
    }
}

/// Functional API: change the pitch language to `target`.
pub fn change_language(score: &Score, target: PitchLanguage) -> Score {
    ChangeLanguage::new(target).apply(score)
}

/// Functional API: change the pitch language in a Music tree to `target`.
pub fn change_language_music(doc: &MusicDocument, target: PitchLanguage) -> MusicDocument {
    ChangeLanguage::new(target).apply_music(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::language::PitchLanguage;
    use crate::ir::score::Score;

    #[test]
    fn change_language_basic() {
        let score = Score::new();
        let result = change_language(&score, PitchLanguage::Italiano);
        assert_eq!(result.metadata.pitch_language, Some(PitchLanguage::Italiano));
    }

    #[test]
    fn change_language_idempotent() {
        let score = Score::new();
        let once = change_language(&score, PitchLanguage::Deutsch);
        let twice = change_language(&once, PitchLanguage::Deutsch);
        assert_eq!(once, twice);
    }

    #[test]
    fn change_language_no_mutation() {
        let score = Score::new();
        let original = score.clone();
        let _ = change_language(&score, PitchLanguage::Espanol);
        assert_eq!(score, original, "input must not be mutated");
    }

    #[test]
    fn change_language_overwrite() {
        let mut score = Score::new();
        score.metadata.pitch_language = Some(PitchLanguage::English);
        let result = change_language(&score, PitchLanguage::Nederlands);
        assert_eq!(result.metadata.pitch_language, Some(PitchLanguage::Nederlands));
    }

    #[test]
    fn change_language_music_basic() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::empty());
        let result = super::change_language_music(&doc, PitchLanguage::Italiano);
        assert_eq!(result.metadata.pitch_language, Some(PitchLanguage::Italiano));
    }
}
