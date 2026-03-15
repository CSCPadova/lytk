//! Retrograde transform — reverse the order of voice elements within each voice.
//!
//! This reverses the temporal ordering of notes, rests, and chords in every
//! voice of every measure, producing a time-reversed version of the music.
//!
//! # Idempotency
//! Retrograde is self-inverse: `R(R(x)) == x`.

use crate::ir::score::Score;

use super::Transform;

/// Reverse the order of voice elements within every voice.
///
/// Forward/Backup elements are also reversed, which preserves the overall
/// time-shift structure when the voice is played back in reverse.
pub struct Retrograde;

impl Retrograde {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Retrograde {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for Retrograde {
    fn apply(&self, score: &Score) -> Score {
        let mut result = score.clone();

        for part in result.parts_mut() {
            // Reverse the order of measures
            part.measures.reverse();

            // Re-number measures sequentially
            for (i, measure) in part.measures.iter_mut().enumerate() {
                measure.number = (i + 1) as u32;

                // Reverse elements within each voice
                for voice in &mut measure.voices {
                    voice.elements.reverse();
                }
            }
        }

        result
    }
}

/// Functional API: reverse all voice elements and measure order.
pub fn retrograde(score: &Score) -> Score {
    Retrograde::new().apply(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::measure::Measure;
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;

    fn make_three_note_score() -> Score {
        let n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let n2 = Note::new(Pitch::new(PitchStep::D, 4), Duration::quarter());
        let n3 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let voice = Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(n1)),
                VoiceElement::Note(Box::new(n2)),
                VoiceElement::Note(Box::new(n3)),
            ],
        };
        let measure = Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    fn extract_pitches(score: &Score) -> Vec<PitchStep> {
        let parts = score.parts();
        parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.step),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn retrograde_basic() {
        let score = make_three_note_score();
        let result = retrograde(&score);
        let pitches = extract_pitches(&result);
        assert_eq!(pitches, vec![PitchStep::E, PitchStep::D, PitchStep::C]);
    }

    #[test]
    fn retrograde_self_inverse() {
        let score = make_three_note_score();
        let doubled = retrograde(&retrograde(&score));
        let orig_pitches = extract_pitches(&score);
        let round_pitches = extract_pitches(&doubled);
        assert_eq!(orig_pitches, round_pitches, "retrograde is self-inverse");
    }

    #[test]
    fn retrograde_no_mutation() {
        let score = make_three_note_score();
        let original = score.clone();
        let _ = retrograde(&score);
        assert_eq!(score, original, "input must not be mutated");
    }

    #[test]
    fn retrograde_empty_score() {
        let score = Score::new();
        let result = retrograde(&score);
        assert_eq!(result, score);
    }
}
