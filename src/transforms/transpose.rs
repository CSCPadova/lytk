//! Transpose transform — shift all pitches by N semitones.
//!
//! Also updates key signatures on the circle of fifths.
//!
//! # Idempotency
//! `Transpose(n)` applied twice equals one application of `Transpose(n)` only
//! when `n == 0`. For non-zero values, Transpose is *invertible* instead:
//! `Transpose(-n)(Transpose(n)(x)) == x`.

use crate::ir::interval::Interval;
use crate::ir::measure::KeySignature;
use crate::ir::music::{Music, MusicDocument};
use crate::ir::note::VoiceElement;
use crate::ir::pitch::{major_tonic, Pitch};
use crate::ir::score::Score;

use super::{MusicTransform, Transform};

/// How to transpose: a fixed chromatic distance, or a spelling-correct interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransposeMode {
    /// Shift every pitch by a fixed number of semitones (nearest-natural spelling).
    Chromatic(i32),
    /// Shift by a named/diatonic interval, preserving correct enharmonic spelling.
    Diatonic(Interval),
}

impl TransposeMode {
    /// Equivalent semitone distance (used for key-signature transposition).
    fn semitones(self) -> i32 {
        match self {
            TransposeMode::Chromatic(n) => n,
            TransposeMode::Diatonic(iv) => iv.chromatic,
        }
    }

    fn is_identity(self) -> bool {
        match self {
            TransposeMode::Chromatic(n) => n == 0,
            TransposeMode::Diatonic(iv) => iv.diatonic == 0 && iv.chromatic == 0,
        }
    }

    fn apply_pitch(self, p: Pitch) -> Pitch {
        match self {
            TransposeMode::Chromatic(n) => p.transposed(n),
            TransposeMode::Diatonic(iv) => p.transpose_diatonic(iv.diatonic, iv.chromatic),
        }
    }
}

/// Transpose all pitches, chromatically or by a diatonic interval.
///
/// Key signatures are adjusted on the circle of fifths. Mode is preserved.
pub struct Transpose {
    mode: TransposeMode,
}

impl Transpose {
    /// Chromatic transposition by a fixed number of semitones.
    pub fn new(semitones: i32) -> Self {
        Self {
            mode: TransposeMode::Chromatic(semitones),
        }
    }

    /// Spelling-correct transposition by a diatonic interval.
    pub fn by_interval(interval: Interval) -> Self {
        Self {
            mode: TransposeMode::Diatonic(interval),
        }
    }
}

impl Transform for Transpose {
    fn apply(&self, score: &Score) -> Score {
        if self.mode.is_identity() {
            return score.clone();
        }

        let mut result = score.clone();

        for part in result.parts_mut() {
            for measure in &mut part.measures {
                // Transpose key signature
                if let Some(attrs) = &mut measure.attributes {
                    if let Some(key) = &mut attrs.key {
                        *key = transpose_key(*key, self.mode.semitones());
                    }
                }

                // Transpose notes in all voices
                for voice in &mut measure.voices {
                    for elem in &mut voice.elements {
                        match elem {
                            VoiceElement::Note(n) => {
                                n.pitch = self.mode.apply_pitch(n.pitch);
                            }
                            VoiceElement::Chord(c) => {
                                for n in &mut c.notes {
                                    n.pitch = self.mode.apply_pitch(n.pitch);
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

impl MusicTransform for Transpose {
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
        if self.mode.is_identity() {
            return doc.clone();
        }
        let mut result = doc.clone();
        transpose_music_node(&mut result.music, self.mode);
        result
    }
}

/// Recursively transpose all pitches and key signatures in a Music tree.
fn transpose_music_node(music: &mut Music, mode: TransposeMode) {
    match music {
        Music::Note { pitch, .. } => {
            *pitch = mode.apply_pitch(*pitch);
        }
        Music::Chord { pitches, .. } => {
            for (pitch, _) in pitches.iter_mut() {
                *pitch = mode.apply_pitch(*pitch);
            }
        }
        Music::KeySignature(key) => {
            *key = transpose_key(*key, mode.semitones());
        }
        Music::Sequential(children) | Music::Simultaneous(children) => {
            for child in children {
                transpose_music_node(child, mode);
            }
        }
        Music::Context { content, .. }
        | Music::Grace { content, .. }
        | Music::Tuplet { content, .. }
        | Music::Variable { content, .. } => {
            transpose_music_node(content, mode);
        }
        Music::Repeat {
            body, alternatives, ..
        } => {
            transpose_music_node(body, mode);
            for alt in alternatives {
                transpose_music_node(alt, mode);
            }
        }
        _ => {}
    }
}

/// Functional API: chromatic transposition by `semitones`.
pub fn transpose(score: &Score, semitones: i32) -> Score {
    Transpose::new(semitones).apply(score)
}

/// Functional API: chromatic transposition of a Music tree by `semitones`.
pub fn transpose_music(doc: &MusicDocument, semitones: i32) -> MusicDocument {
    Transpose::new(semitones).apply_music(doc)
}

/// Functional API: spelling-correct transposition by a diatonic interval.
pub fn transpose_interval(score: &Score, interval: Interval) -> Score {
    Transpose::by_interval(interval).apply(score)
}

/// Functional API: spelling-correct transposition of a Music tree by an interval.
pub fn transpose_interval_music(doc: &MusicDocument, interval: Interval) -> MusicDocument {
    Transpose::by_interval(interval).apply_music(doc)
}

/// Transpose a score so its tonic becomes `target_tonic`, choosing the nearest
/// direction (≤ a tritone). The interval is computed from the score's first key
/// signature (default C major); key signatures shift with it, and any internal
/// key changes move by the same interval. Mode is preserved (a minor piece stays
/// minor at the new tonic).
pub fn transpose_to_key(score: &Score, target_tonic: Pitch) -> Score {
    let source_fifths = first_key_fifths(score).unwrap_or(0);
    let from = major_tonic(source_fifths);
    let to = Pitch::with_alter(target_tonic.step, target_tonic.alter, 4);
    let mut iv = Interval::between(&from, &to);
    // Pick the nearest octave so we never transpose more than a tritone.
    while iv.chromatic > 6 {
        iv = Interval::new(iv.diatonic - 7, iv.chromatic - 12);
    }
    while iv.chromatic < -6 {
        iv = Interval::new(iv.diatonic + 7, iv.chromatic + 12);
    }
    transpose_interval(score, iv)
}

/// The `fifths` of the first key signature found in the score, if any.
fn first_key_fifths(score: &Score) -> Option<i32> {
    score.parts().iter().find_map(|part| {
        part.measures.iter().find_map(|m| {
            m.attributes
                .as_ref()
                .and_then(|a| a.key.as_ref())
                .map(|k| k.fifths as i32)
        })
    })
}

/// Transpose a key signature on the circle of fifths.
///
/// Each semitone maps to a number of fifths steps; every entry in the table
/// below is already within the valid -7..=7 range, so no clamping is needed.
fn transpose_key(key: KeySignature, semitones: i32) -> KeySignature {
    // Semitone-to-fifths mapping: C→0, C#→7, D→2, Eb→-3, E→4, F→-1,
    // F#→6, G→1, Ab→-4, A→3, Bb→-2, B→5
    const SEMITONE_TO_FIFTHS: [i32; 12] = [0, 7, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5];

    // Current root pitch in semitones from C (based on fifths position).
    let current_semitones = fifths_to_semitones(key.fifths as i32);
    let target_semitones = (current_semitones + semitones).rem_euclid(12) as usize;

    KeySignature {
        fifths: SEMITONE_TO_FIFTHS[target_semitones] as i8,
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
    use crate::ir::interval::Interval;
    use crate::ir::measure::{KeyMode, KeySignature, Measure, MeasureAttributes};
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Alter, Pitch, PitchStep};
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
            staff_lines: None,
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
            number_label: None,
            implicit: false,
            senza_misura: false,
            width: None,
            attributes: Some(attrs),
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

        assert_eq!(
            orig_notes, rest_notes,
            "transpose roundtrip must preserve MIDI numbers"
        );
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

    #[test]
    fn transpose_music_basic() {
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
        let result = super::transpose_music(&doc, 2);
        match &result.music {
            Music::Sequential(children) => {
                match &children[0] {
                    Music::Note { pitch, .. } => assert_eq!(pitch.step, PitchStep::D),
                    _ => panic!("expected Note"),
                }
                match &children[1] {
                    Music::Note { pitch, .. } => {
                        // E + 2 semitones = F#
                        assert_eq!(pitch.step, PitchStep::F);
                    }
                    _ => panic!("expected Note"),
                }
            }
            _ => panic!("expected Sequential"),
        }
    }

    #[test]
    fn transpose_by_interval_spelling() {
        // C E G in C major, up a major third → E G# B in E major.
        let score = make_test_score();
        let result = transpose_interval(&score, Interval::from_name("M3").unwrap());
        let parts = result.parts();
        let notes: Vec<_> = parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch),
                _ => None,
            })
            .collect();
        assert_eq!(notes[0].step, PitchStep::E);
        assert_eq!(notes[0].alter, Alter::from_integer(0));
        assert_eq!(notes[1].step, PitchStep::G);
        assert_eq!(notes[1].alter, Alter::from_integer(1)); // G#, not Ab
        assert_eq!(notes[2].step, PitchStep::B);
        // Key C major → E major (4 sharps).
        let key = parts[0].measures[0]
            .attributes
            .as_ref()
            .unwrap()
            .key
            .unwrap();
        assert_eq!(key.fifths, 4);
    }

    #[test]
    fn transpose_to_key_c_to_d() {
        let score = make_test_score(); // C major
        let result = transpose_to_key(&score, crate::ir::pitch::major_tonic(2)); // → D
        let parts = result.parts();
        let key = parts[0].measures[0]
            .attributes
            .as_ref()
            .unwrap()
            .key
            .unwrap();
        assert_eq!(key.fifths, 2); // D major
        let first = match &parts[0].measures[0].voices[0].elements[0] {
            VoiceElement::Note(n) => n.pitch,
            _ => panic!("expected note"),
        };
        assert_eq!(first.step, PitchStep::D); // C → D, up a major second (nearest)
    }

    #[test]
    fn transpose_music_roundtrip() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![Music::Note {
            pitch: Pitch::new(PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        }]));
        let up = super::transpose_music(&doc, 5);
        let restored = super::transpose_music(&up, -5);
        match (&doc.music, &restored.music) {
            (Music::Sequential(orig), Music::Sequential(rest)) => match (&orig[0], &rest[0]) {
                (Music::Note { pitch: p1, .. }, Music::Note { pitch: p2, .. }) => {
                    assert_eq!(p1.midi_number(), p2.midi_number());
                }
                _ => panic!("expected Notes"),
            },
            _ => panic!("expected Sequential"),
        }
    }
}
