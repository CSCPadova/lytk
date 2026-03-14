//! Transpose transform — shift all pitches by N semitones.
//!
//! Also updates key signatures on the circle of fifths.
//!
//! # Idempotency
//! `Transpose(n)` applied twice equals one application of `Transpose(n)` only
//! when `n == 0`. For non-zero values, Transpose is *invertible* instead:
//! `Transpose(-n)(Transpose(n)(x)) == x`.

use crate::ir::measure::KeySignature;
use crate::ir::note::VoiceElement;
use crate::ir::score::Score;

use super::Transform;

/// Transpose all pitches by a fixed number of semitones.
///
/// Key signatures are adjusted on the circle of fifths. Mode is preserved.
pub struct Transpose {
    pub semitones: i32,
}

impl Transpose {
    pub fn new(semitones: i32) -> Self {
        Self { semitones }
    }
}

impl Transform for Transpose {
    fn apply(&self, score: &Score) -> Score {
        if self.semitones == 0 {
            return score.clone();
        }

        let mut result = score.clone();

        for part in result.parts_mut() {
            for measure in &mut part.measures {
                // Transpose key signature
                if let Some(attrs) = &mut measure.attributes {
                    if let Some(key) = &mut attrs.key {
                        *key = transpose_key(*key, self.semitones);
                    }
                }

                // Transpose notes in all voices
                for voice in &mut measure.voices {
                    for elem in &mut voice.elements {
                        match elem {
                            VoiceElement::Note(n) => {
                                n.pitch = n.pitch.transposed(self.semitones);
                            }
                            VoiceElement::Chord(c) => {
                                for n in &mut c.notes {
                                    n.pitch = n.pitch.transposed(self.semitones);
                                }
                            }
                            VoiceElement::Rest(_)
                            | VoiceElement::Forward(_)
                            | VoiceElement::Backup(_) => {}
                        }
                    }
                }
            }
        }

        result
    }
}

/// Functional API: transpose all pitches by `semitones`.
pub fn transpose(score: &Score, semitones: i32) -> Score {
    Transpose::new(semitones).apply(score)
}

/// Transpose a key signature on the circle of fifths.
///
/// Each semitone maps to a number of fifths steps. The result is clamped
/// to the valid range -7..=7 (wrapping enharmonically when needed).
fn transpose_key(key: KeySignature, semitones: i32) -> KeySignature {
    // Semitone-to-fifths mapping: C→0, C#→7, D→2, Eb→-3, E→4, F→-1,
    // F#→6, G→1, Ab→-4, A→3, Bb→-2, B→5
    const SEMITONE_TO_FIFTHS: [i32; 12] = [0, 7, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5];

    // Current root pitch in semitones from C (based on fifths position)
    let current_semitones = fifths_to_semitones(key.fifths as i32);
    let target_semitones = (current_semitones + semitones).rem_euclid(12) as usize;
    let new_fifths = SEMITONE_TO_FIFTHS[target_semitones];

    // Clamp to -7..=7 (enharmonic wrap)
    let clamped = if new_fifths > 7 {
        new_fifths - 12
    } else if new_fifths < -7 {
        new_fifths + 12
    } else {
        new_fifths
    };

    KeySignature {
        fifths: clamped as i8,
        mode: key.mode,
    }
}

/// Convert fifths position to semitones from C.
fn fifths_to_semitones(fifths: i32) -> i32 {
    // Each fifth = 7 semitones (mod 12)
    (fifths * 7).rem_euclid(12)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::measure::{KeyMode, KeySignature, Measure, MeasureAttributes};
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;
    use std::collections::HashMap;

    fn make_test_score() -> Score {
        let attrs = MeasureAttributes {
            divisions: 4,
            key: Some(KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            }),
            time: None,
            clefs: HashMap::new(),
            transpose: None,
            staves: None,
        };
        let n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let n3 = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
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
            attributes: Some(attrs),
            left_barline: None,
            right_barline: None,
            directions: vec![],
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    #[test]
    fn transpose_basic() {
        let score = make_test_score();
        let result = transpose(&score, 2); // up major 2nd

        let parts = result.parts();
        let notes: Vec<_> = parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(&n.pitch),
                _ => None,
            })
            .collect();

        assert_eq!(notes[0].step, PitchStep::D);
        assert_eq!(notes[1].step, PitchStep::F); // E + 2 semitones -> F# actually
        assert_eq!(notes[2].step, PitchStep::A);
    }

    #[test]
    fn transpose_no_mutation() {
        let score = make_test_score();
        let original = score.clone();
        let _ = transpose(&score, 5);
        assert_eq!(score, original, "input must not be mutated");
    }

    #[test]
    fn transpose_zero_is_identity() {
        let score = make_test_score();
        let result = transpose(&score, 0);
        assert_eq!(result, score);
    }

    #[test]
    fn transpose_roundtrip() {
        let score = make_test_score();
        let up = transpose(&score, 5);
        let restored = transpose(&up, -5);

        let orig_parts = score.parts();
        let rest_parts = restored.parts();
        let orig_notes: Vec<_> = orig_parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.midi_number()),
                _ => None,
            })
            .collect();
        let rest_notes: Vec<_> = rest_parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.midi_number()),
                _ => None,
            })
            .collect();

        assert_eq!(orig_notes, rest_notes, "transpose roundtrip must preserve MIDI numbers");
    }

    #[test]
    fn transpose_key_c_to_d() {
        let key = KeySignature {
            fifths: 0,
            mode: KeyMode::Major,
        };
        let result = transpose_key(key, 2); // C → D = 2 sharps
        assert_eq!(result.fifths, 2);
        assert_eq!(result.mode, KeyMode::Major);
    }

    #[test]
    fn transpose_key_wraps() {
        let key = KeySignature {
            fifths: 6,
            mode: KeyMode::Major,
        };
        // F# major (6 sharps) + 1 semitone = G major (1 sharp)
        let result = transpose_key(key, 1);
        assert_eq!(result.fifths, 1);
    }
}
