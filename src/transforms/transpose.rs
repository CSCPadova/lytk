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
use crate::ir::pitch::{major_tonic, respell, Alter, Pitch};
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
    fn is_identity(self) -> bool {
        match self {
            TransposeMode::Chromatic(n) => n == 0,
            TransposeMode::Diatonic(iv) => iv.diatonic == 0 && iv.chromatic == 0,
        }
    }

    /// Transpose a pitch. `fifths` is the *target* key's circle-of-fifths
    /// position, used to respell chromatic results (C major +3 must yield
    /// e-flat under the E-flat signature, not d-sharp). Microtonal pitches
    /// are exempt from respelling (it would round them to semitones).
    fn apply_pitch(self, p: Pitch, fifths: i32) -> Pitch {
        match self {
            TransposeMode::Chromatic(n) => {
                let t = p.transposed(n);
                if p.alter.is_integer() {
                    respell(t, fifths)
                } else {
                    t
                }
            }
            TransposeMode::Diatonic(iv) => p.transpose_diatonic(iv.diatonic, iv.chromatic),
        }
    }

    /// Transpose a key signature. Diatonic mode moves it by the interval's
    /// line-of-fifths delta (so d5 lands on G-flat, A4 on F-sharp); chromatic
    /// mode uses the fixed nearest-key table.
    fn apply_key(self, key: KeySignature) -> KeySignature {
        match self {
            TransposeMode::Chromatic(n) => transpose_key(key, n),
            TransposeMode::Diatonic(iv) => {
                // An interval of d diatonic steps and s semitones sits at
                // 7s − 12d on the line of fifths (M2 → +2, m3 → −3, P5 → +1).
                let delta = 7 * iv.chromatic - 12 * iv.diatonic;
                let mut fifths = key.fifths as i32 + delta;
                // Normalize into a real signature; ±12 fifths is an
                // enharmonic respelling (C-sharp major ↔ D-flat major).
                while fifths > 7 {
                    fifths -= 12;
                }
                while fifths < -7 {
                    fifths += 12;
                }
                KeySignature {
                    fifths: fifths as i8,
                    mode: key.mode,
                }
            }
        }
    }

    /// Target-key fifths when the source has no (or an implicit C major) key
    /// signature — the reference for chromatic respelling.
    fn default_target_fifths(self) -> i32 {
        self.apply_key(KeySignature {
            fifths: 0,
            mode: crate::ir::measure::KeyMode::Major,
        })
        .fifths as i32
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
            // Track the *transposed* key so chromatic results respell to it.
            let mut cur_fifths = self.mode.default_target_fifths();
            for measure in &mut part.measures {
                // Transpose key signature
                if let Some(attrs) = &mut measure.attributes {
                    if let Some(key) = &mut attrs.key {
                        *key = self.mode.apply_key(*key);
                        cur_fifths = key.fifths as i32;
                    }
                }

                // Transpose chord symbols alongside the notes they harmonize.
                for harmony in &mut measure.harmonies {
                    transpose_harmony(harmony, self.mode, cur_fifths);
                }

                // Transpose notes in all voices
                for voice in &mut measure.voices {
                    for elem in &mut voice.elements {
                        match elem {
                            VoiceElement::Note(n) => {
                                n.pitch = self.mode.apply_pitch(n.pitch, cur_fifths);
                            }
                            VoiceElement::Chord(c) => {
                                for n in &mut c.notes {
                                    n.pitch = self.mode.apply_pitch(n.pitch, cur_fifths);
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
        let mut fifths = self.mode.default_target_fifths();
        transpose_music_node(&mut result.music, self.mode, &mut fifths);
        result
    }
}

/// Recursively transpose all pitches, key signatures, and chord symbols in a
/// Music tree. `fifths` tracks the current *transposed* key for respelling;
/// a key change inside one branch of a Simultaneous stays local to it.
fn transpose_music_node(music: &mut Music, mode: TransposeMode, fifths: &mut i32) {
    match music {
        Music::Note { pitch, .. } => {
            *pitch = mode.apply_pitch(*pitch, *fifths);
        }
        Music::Chord { pitches, .. } => {
            for (pitch, _) in pitches.iter_mut() {
                *pitch = mode.apply_pitch(*pitch, *fifths);
            }
        }
        Music::KeySignature(key) => {
            *key = mode.apply_key(*key);
            *fifths = key.fifths as i32;
        }
        Music::Harmony(h) => {
            transpose_harmony(h, mode, *fifths);
        }
        Music::Sequential(children) => {
            for child in children {
                transpose_music_node(child, mode, fifths);
            }
        }
        Music::Simultaneous(children) => {
            for child in children {
                let mut branch_fifths = *fifths;
                transpose_music_node(child, mode, &mut branch_fifths);
            }
        }
        Music::Context { content, .. }
        | Music::Grace { content, .. }
        | Music::Tuplet { content, .. }
        | Music::Variable { content, .. } => {
            transpose_music_node(content, mode, fifths);
        }
        Music::Repeat {
            body, alternatives, ..
        } => {
            transpose_music_node(body, mode, fifths);
            for alt in alternatives {
                let mut alt_fifths = *fifths;
                transpose_music_node(alt, mode, &mut alt_fifths);
            }
        }
        _ => {}
    }
}

/// Transpose a chord symbol's root and bass by the same rule as the notes.
fn transpose_harmony(h: &mut crate::ir::harmony::Harmony, mode: TransposeMode, fifths: i32) {
    transpose_chord_pitch(&mut h.root, mode, fifths);
    if let Some(bass) = &mut h.bass {
        transpose_chord_pitch(bass, mode, fifths);
    }
}

fn transpose_chord_pitch(
    cp: &mut crate::ir::harmony::ChordPitch,
    mode: TransposeMode,
    fifths: i32,
) {
    let Some(step) = crate::ir::pitch::PitchStep::from_name(&cp.step) else {
        return;
    };
    if cp.alter.fract() != 0.0 {
        return; // microtonal chord root — leave untouched
    }
    let p = Pitch::with_alter(step, Alter::from_integer(cp.alter as i32), 4);
    let t = mode.apply_pitch(p, fifths);
    cp.step = format!("{:?}", t.step);
    cp.alter = *t.alter.numer() as f64 / *t.alter.denom() as f64;
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
            measure_repeat: None,
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

/// Regression tests (review R3): key-aware spelling, interval-derived key
/// signatures, and chord-symbol transposition.
#[cfg(test)]
mod spelling_tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::harmony::{ChordPitch, Harmony};
    use crate::ir::interval::Interval;
    use crate::ir::measure::{KeyMode, KeySignature, Measure, MeasureAttributes};
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Alter, Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;

    fn score_in_c(notes: &[(PitchStep, i32)]) -> Score {
        let mut measure = Measure::new(1);
        measure.attributes = Some(MeasureAttributes {
            key: Some(KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            }),
            ..MeasureAttributes::default()
        });
        measure.voices.push(Voice {
            number: 1,
            elements: notes
                .iter()
                .map(|&(s, a)| {
                    VoiceElement::Note(Box::new(Note::new(
                        Pitch::with_alter(s, Alter::from_integer(a), 4),
                        Duration::quarter(),
                    )))
                })
                .collect(),
        });
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    fn pitches(score: &Score) -> Vec<(PitchStep, i32)> {
        score.parts()[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some((n.pitch.step, n.pitch.alter.to_integer())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn chromatic_transpose_respells_for_flat_target_key() {
        // C major + 3 semitones = E-flat major: c e g → ees g bes,
        // NOT dis g ais (the canonical wrong-enharmonic case).
        let score = score_in_c(&[(PitchStep::C, 0), (PitchStep::E, 0), (PitchStep::G, 0)]);
        let result = transpose(&score, 3);
        let key = result.parts()[0].measures[0]
            .attributes
            .as_ref()
            .unwrap()
            .key
            .unwrap();
        assert_eq!(key.fifths, -3, "E-flat major");
        assert_eq!(
            pitches(&result),
            vec![(PitchStep::E, -1), (PitchStep::G, 0), (PitchStep::B, -1)],
            "black keys must follow the target key's flats"
        );
    }

    #[test]
    fn diatonic_key_signature_follows_the_interval() {
        // C major up a diminished fifth → G-flat major (6 flats), not F# (6 sharps).
        let score = score_in_c(&[(PitchStep::C, 0)]);
        let dim5 = Interval::from_name("d5").unwrap();
        let result = transpose_interval(&score, dim5);
        let key = result.parts()[0].measures[0]
            .attributes
            .as_ref()
            .unwrap()
            .key
            .unwrap();
        assert_eq!(key.fifths, -6, "d5 above C is G-flat, a flat key");
        // And the note spelling agrees with the signature.
        assert_eq!(pitches(&result), vec![(PitchStep::G, -1)]);

        // Augmented fourth → F-sharp major (6 sharps).
        let aug4 = Interval::from_name("A4").unwrap();
        let result = transpose_interval(&score, aug4);
        let key = result.parts()[0].measures[0]
            .attributes
            .as_ref()
            .unwrap()
            .key
            .unwrap();
        assert_eq!(key.fifths, 6, "A4 above C is F-sharp, a sharp key");
        assert_eq!(pitches(&result), vec![(PitchStep::F, 1)]);
    }

    #[test]
    fn transpose_moves_chord_symbols() {
        let mut score = score_in_c(&[(PitchStep::C, 0)]);
        if let ScoreChild::Part(part) = &mut score.children[0] {
            part.measures[0].harmonies.push(Harmony {
                root: ChordPitch {
                    step: "C".to_string(),
                    alter: 0.0,
                },
                kind: "major".to_string(),
                bass: Some(ChordPitch {
                    step: "E".to_string(),
                    alter: 0.0,
                }),
                degrees: vec![],
                offset: 0,
                function: None,
            });
        }
        // Chromatic +3 into E-flat major: C/E → Eb/G.
        let result = transpose(&score, 3);
        let h = &result.parts()[0].measures[0].harmonies[0];
        assert_eq!(
            (h.root.step.as_str(), h.root.alter),
            ("E", -1.0),
            "root C → Eb"
        );
        let bass = h.bass.as_ref().unwrap();
        assert_eq!((bass.step.as_str(), bass.alter), ("G", 0.0), "bass E → G");

        // Diatonic M2: C/E → D/F#.
        let result = transpose_interval(&score, Interval::from_name("M2").unwrap());
        let h = &result.parts()[0].measures[0].harmonies[0];
        assert_eq!((h.root.step.as_str(), h.root.alter), ("D", 0.0));
        let bass = h.bass.as_ref().unwrap();
        assert_eq!((bass.step.as_str(), bass.alter), ("F", 1.0));
    }

    #[test]
    fn music_layer_agrees_with_score_layer() {
        use crate::ir::music::{Music, MusicDocument};
        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::KeySignature(KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            }),
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let result = transpose_music(&doc, 3);
        match &result.music {
            Music::Sequential(children) => {
                match &children[0] {
                    Music::KeySignature(k) => assert_eq!(k.fifths, -3),
                    other => panic!("expected key signature, got {other:?}"),
                }
                match &children[1] {
                    Music::Note { pitch, .. } => {
                        assert_eq!(
                            (pitch.step, pitch.alter.to_integer()),
                            (PitchStep::G, 0),
                            "E + 3 in E-flat major is G"
                        );
                    }
                    other => panic!("expected note, got {other:?}"),
                }
            }
            other => panic!("expected Sequential, got {other:?}"),
        }
    }
}
