//! Retrograde transform — reverse the order of voice elements within each voice.
//!
//! This reverses the temporal ordering of notes, rests, and chords in every
//! voice of every measure, producing a time-reversed version of the music.
//!
//! # Idempotency
//! Retrograde is self-inverse: `R(R(x)) == x`.

use crate::ir::music::{Music, MusicDocument};
use crate::ir::score::Score;

use super::{MusicTransform, Transform};

/// Reverse the order of voice elements within every voice.
///
/// Voice elements are reversed, producing a time-reversed version.
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

impl MusicTransform for Retrograde {
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
        let mut result = doc.clone();
        retrograde_music_node(&mut result.music);
        result
    }
}

/// Recursively reverse Sequential children in a Music tree.
fn retrograde_music_node(music: &mut Music) {
    match music {
        Music::Sequential(children) => {
            children.reverse();
            for child in children {
                retrograde_music_node(child);
            }
        }
        Music::Simultaneous(children) => {
            // Don't reverse simultaneous — each voice gets retrograded independently
            for child in children {
                retrograde_music_node(child);
            }
        }
        Music::Context { content, .. }
        | Music::Grace { content, .. }
        | Music::Tuplet { content, .. }
        | Music::Variable { content, .. } => {
            retrograde_music_node(content);
        }
        Music::Repeat {
            body, alternatives, ..
        } => {
            retrograde_music_node(body);
            for alt in alternatives {
                retrograde_music_node(alt);
            }
        }
        _ => {}
    }
}

/// Functional API: reverse all voice elements and measure order.
pub fn retrograde(score: &Score) -> Score {
    Retrograde::new().apply(score)
}

/// Functional API: reverse Sequential children in a Music tree.
pub fn retrograde_music(doc: &MusicDocument) -> MusicDocument {
    Retrograde::new().apply_music(doc)
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
            print_object: true,
            multi_measure_rest: None,
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

    #[test]
    fn retrograde_music_basic() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let result = super::retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                let steps: Vec<_> = children
                    .iter()
                    .filter_map(|c| match c {
                        Music::Note { pitch, .. } => Some(pitch.step),
                        _ => None,
                    })
                    .collect();
                assert_eq!(steps, vec![PitchStep::E, PitchStep::D, PitchStep::C]);
            }
            _ => panic!("expected Sequential"),
        }
    }

    #[test]
    fn retrograde_music_self_inverse() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let doubled = super::retrograde_music(&super::retrograde_music(&doc));
        assert_eq!(doc, doubled);
    }
}
