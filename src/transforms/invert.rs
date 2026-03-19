//! Invert transform — mirror pitches around an axis pitch.
//!
//! For each pitch, the interval from the axis is negated, producing a
//! melodic inversion. For example, if the axis is C4 and a note is E4
//! (4 semitones above), the inversion is Ab3 (4 semitones below).
//!
//! # Idempotency
//! Invert is self-inverse: `I(I(x)) == x` (reflecting twice around the same
//! axis restores the original).

use crate::ir::music::{Music, MusicDocument};
use crate::ir::note::VoiceElement;
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::Score;

use super::{MusicTransform, Transform};

/// Invert all pitches around an axis pitch.
///
/// The default axis is middle C (C4). Each pitch is reflected across the
/// axis by negating the interval in semitones.
pub struct Invert {
    pub axis: Pitch,
}

impl Invert {
    pub fn new(axis: Pitch) -> Self {
        Self { axis }
    }
}

impl Default for Invert {
    fn default() -> Self {
        Self::new(Pitch::new(PitchStep::C, 4))
    }
}

impl Transform for Invert {
    fn apply(&self, score: &Score) -> Score {
        let mut result = score.clone();
        let axis_midi = self.axis.midi_number();

        for part in result.parts_mut() {
            for measure in &mut part.measures {
                for voice in &mut measure.voices {
                    for elem in &mut voice.elements {
                        match elem {
                            VoiceElement::Note(n) => {
                                n.pitch = invert_pitch(n.pitch, axis_midi);
                            }
                            VoiceElement::Chord(c) => {
                                for n in &mut c.notes {
                                    n.pitch = invert_pitch(n.pitch, axis_midi);
                                }
                            }
                            VoiceElement::Rest(_) => {}
                        }
                    }
                }
            }
        }

        result
    }
}

impl MusicTransform for Invert {
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
        let mut result = doc.clone();
        let axis_midi = self.axis.midi_number();
        invert_music_node(&mut result.music, axis_midi);
        result
    }
}

/// Recursively invert all pitches in a Music tree.
fn invert_music_node(music: &mut Music, axis_midi: i32) {
    match music {
        Music::Note { pitch, .. } => {
            *pitch = invert_pitch(*pitch, axis_midi);
        }
        Music::Chord { pitches, .. } => {
            for (pitch, _) in pitches.iter_mut() {
                *pitch = invert_pitch(*pitch, axis_midi);
            }
        }
        Music::Sequential(children) | Music::Simultaneous(children) => {
            for child in children {
                invert_music_node(child, axis_midi);
            }
        }
        Music::Context { content, .. }
        | Music::Grace { content, .. }
        | Music::Tuplet { content, .. }
        | Music::Variable { content, .. } => {
            invert_music_node(content, axis_midi);
        }
        Music::Repeat {
            body, alternatives, ..
        } => {
            invert_music_node(body, axis_midi);
            for alt in alternatives {
                invert_music_node(alt, axis_midi);
            }
        }
        _ => {}
    }
}

/// Functional API: invert all pitches around `axis`.
pub fn invert(score: &Score, axis: Pitch) -> Score {
    Invert::new(axis).apply(score)
}

/// Functional API: invert all pitches in a Music tree around `axis`.
pub fn invert_music(doc: &MusicDocument, axis: Pitch) -> MusicDocument {
    Invert::new(axis).apply_music(doc)
}

/// Reflect a single pitch across the axis MIDI number.
fn invert_pitch(pitch: Pitch, axis_midi: i32) -> Pitch {
    let pitch_midi = pitch.midi_number();
    let interval = pitch_midi - axis_midi;
    // Reflect: new_midi = axis - interval = axis - (pitch - axis) = 2*axis - pitch
    let target_midi = axis_midi - interval;
    let semitone_diff = target_midi - pitch_midi;
    pitch.transposed(semitone_diff)
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

    fn make_score_with_pitches(pitches: &[Pitch]) -> Score {
        let voice = Voice {
            number: 1,
            elements: pitches
                .iter()
                .map(|p| VoiceElement::Note(Box::new(Note::new(*p, Duration::quarter()))))
                .collect(),
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

    fn extract_midi(score: &Score) -> Vec<i32> {
        let parts = score.parts();
        parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.midi_number()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn invert_basic() {
        // Axis = C4 (MIDI 60). E4 (64) → Ab3 (56): interval 4 up becomes 4 down
        let score = make_score_with_pitches(&[
            Pitch::new(PitchStep::C, 4),
            Pitch::new(PitchStep::E, 4),
            Pitch::new(PitchStep::G, 4),
        ]);
        let axis = Pitch::new(PitchStep::C, 4);
        let result = invert(&score, axis);
        let midi = extract_midi(&result);

        // C4=60 → 60, E4=64 → 56 (Ab3), G4=67 → 53 (F3)
        assert_eq!(midi, vec![60, 56, 53]);
    }

    #[test]
    fn invert_self_inverse() {
        let score = make_score_with_pitches(&[
            Pitch::new(PitchStep::C, 4),
            Pitch::new(PitchStep::E, 4),
            Pitch::new(PitchStep::G, 4),
        ]);
        let axis = Pitch::new(PitchStep::C, 4);
        let doubled = invert(&invert(&score, axis), axis);

        let orig_midi = extract_midi(&score);
        let round_midi = extract_midi(&doubled);
        assert_eq!(orig_midi, round_midi, "invert is self-inverse");
    }

    #[test]
    fn invert_no_mutation() {
        let score = make_score_with_pitches(&[Pitch::new(PitchStep::D, 5)]);
        let original = score.clone();
        let _ = invert(&score, Pitch::new(PitchStep::C, 4));
        assert_eq!(score, original, "input must not be mutated");
    }

    #[test]
    fn invert_axis_note_unchanged() {
        // A note on the axis should map to itself.
        let axis = Pitch::new(PitchStep::E, 4);
        let score = make_score_with_pitches(&[axis]);
        let result = invert(&score, axis);
        let midi = extract_midi(&result);
        assert_eq!(midi, vec![axis.midi_number()]);
    }

    #[test]
    fn invert_music_self_inverse() {
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
        let axis = Pitch::new(PitchStep::C, 4);
        let doubled = super::invert_music(&super::invert_music(&doc, axis), axis);

        // Extract MIDI numbers from both
        fn midi_from(m: &Music) -> Vec<i32> {
            match m {
                Music::Sequential(children) => children
                    .iter()
                    .filter_map(|c| match c {
                        Music::Note { pitch, .. } => Some(pitch.midi_number()),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            }
        }
        assert_eq!(midi_from(&doc.music), midi_from(&doubled.music));
    }
}
