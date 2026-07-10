//! Property-based tests for core IR types and transforms.
//!
//! Uses `proptest` to verify algebraic properties:
//! - Pitch: transpose round-trip, midi_number range, identity transpose
//! - Duration: actual_duration positivity, dot monotonicity
//! - Transforms: transpose inverse, invert self-inverse, retrograde self-inverse

use _core::ir::duration::{Duration, Frac};
use _core::ir::measure::{Measure, MeasureAttributes, TimeSignature};
use _core::ir::note::{Note, VoiceElement};
use _core::ir::pitch::{Alter, Pitch, PitchStep};
use _core::ir::score::{Score, ScoreChild};
use _core::ir::voice::Voice;
use _core::ir::Part;
use _core::transforms::invert::Invert;
use _core::transforms::retrograde::Retrograde;
use _core::transforms::transpose::Transpose;
use _core::transforms::Transform;

use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn arb_pitch_step() -> impl Strategy<Value = PitchStep> {
    (0u8..7).prop_map(|i| PitchStep::from_index(i as i32))
}

/// Integer alters only — suitable for MIDI roundtrip tests where
/// `midi_number()` must be exact (integer division truncates microtones).
fn arb_integer_alter() -> impl Strategy<Value = Alter> {
    prop_oneof![
        Just(Alter::from_integer(-2)),
        Just(Alter::from_integer(-1)),
        Just(Alter::from_integer(0)),
        Just(Alter::from_integer(1)),
        Just(Alter::from_integer(2)),
    ]
}

/// Full alter range including microtones — for non-MIDI tests.
fn arb_alter() -> impl Strategy<Value = Alter> {
    prop_oneof![
        Just(Alter::from_integer(-2)),
        Just(Alter::from_integer(-1)),
        Just(Alter::new(-1, 2)),
        Just(Alter::from_integer(0)),
        Just(Alter::new(1, 2)),
        Just(Alter::from_integer(1)),
        Just(Alter::from_integer(2)),
    ]
}

/// Pitch with integer alter — safe for MIDI roundtrip assertions.
fn arb_pitch() -> impl Strategy<Value = Pitch> {
    (arb_pitch_step(), arb_integer_alter(), 1i32..8)
        .prop_map(|(step, alter, octave)| Pitch::with_alter(step, alter, octave))
}

/// Pitch with any alter including microtones.
fn arb_pitch_microtonal() -> impl Strategy<Value = Pitch> {
    (arb_pitch_step(), arb_alter(), 0i32..9)
        .prop_map(|(step, alter, octave)| Pitch::with_alter(step, alter, octave))
}

/// Standard note base durations as fractions of a whole note.
fn arb_base_duration() -> impl Strategy<Value = Frac> {
    prop_oneof![
        Just(Frac::from_integer(2)), // breve
        Just(Frac::from_integer(1)), // whole
        Just(Frac::new(1, 2)),       // half
        Just(Frac::new(1, 4)),       // quarter
        Just(Frac::new(1, 8)),       // eighth
        Just(Frac::new(1, 16)),      // 16th
        Just(Frac::new(1, 32)),      // 32nd
    ]
}

fn arb_duration() -> impl Strategy<Value = Duration> {
    (arb_base_duration(), 0u8..4).prop_map(|(base, dots)| Duration::dotted(base, dots))
}

/// Build a single-part score with one measure containing the given notes.
fn make_score(notes: Vec<Note>) -> Score {
    let elements = notes
        .into_iter()
        .map(|n| VoiceElement::Note(Box::new(n)))
        .collect();
    let voice = Voice {
        number: 1,
        elements,
    };
    let measure = Measure {
        number: 1,
        voices: vec![voice],
        attributes: Some(MeasureAttributes {
            time: Some(TimeSignature {
                beats: "4".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            ..Default::default()
        }),
        ..Measure::new(1)
    };
    let mut part = Part::new("P1");
    part.name = "Part".to_string();
    part.measures = vec![measure];
    Score {
        children: vec![ScoreChild::Part(part)],
        ..Default::default()
    }
}

/// Collect all note MIDI numbers from a score.
fn collect_midi(score: &Score) -> Vec<i32> {
    let mut out = Vec::new();
    for part in score.parts() {
        for m in &part.measures {
            for v in &m.voices {
                for e in &v.elements {
                    if let VoiceElement::Note(n) = e {
                        out.push(n.pitch.midi_number());
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Pitch properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn pitch_transpose_roundtrip(
        pitch in arb_pitch(),
        semitones in -24i32..25,
    ) {
        let up = pitch.transposed(semitones);
        let back = up.transposed(-semitones);
        // MIDI number must round-trip exactly.
        prop_assert_eq!(back.midi_number(), pitch.midi_number());
    }

    #[test]
    fn pitch_transpose_zero_is_identity(pitch in arb_pitch()) {
        let result = pitch.transposed(0);
        prop_assert_eq!(result.midi_number(), pitch.midi_number());
    }

    #[test]
    fn pitch_transpose_adds_semitones(
        pitch in arb_pitch(),
        semitones in -24i32..25,
    ) {
        let result = pitch.transposed(semitones);
        prop_assert_eq!(
            result.midi_number(),
            pitch.midi_number() + semitones,
        );
    }

    #[test]
    fn pitch_transpose_associative(
        pitch in arb_pitch(),
        a in -12i32..13,
        b in -12i32..13,
    ) {
        // T(a)(T(b)(p)).midi == T(a+b)(p).midi
        let step_by_step = pitch.transposed(b).transposed(a);
        let combined = pitch.transposed(a + b);
        prop_assert_eq!(step_by_step.midi_number(), combined.midi_number());
    }

    #[test]
    fn pitch_midi_number_reasonable(pitch in arb_pitch_microtonal()) {
        // Octaves 0..8, alters -2..2 → MIDI should be in a plausible range.
        let midi = pitch.midi_number();
        prop_assert!((-15..140).contains(&midi),
            "MIDI {} out of expected range for {:?}", midi, pitch);
    }

    #[test]
    fn pitch_step_from_index_roundtrip(index in 0i32..7) {
        let step = PitchStep::from_index(index);
        prop_assert_eq!(step.index(), index);
    }

    #[test]
    fn pitch_step_from_index_wraps(index in 0i32..700) {
        let step = PitchStep::from_index(index);
        prop_assert_eq!(step.index(), index % 7);
    }
}

// ---------------------------------------------------------------------------
// Duration properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn duration_actual_positive(dur in arb_duration()) {
        let actual = dur.actual_duration();
        prop_assert!(actual > Frac::from_integer(0),
            "Duration {:?} has non-positive actual_duration: {}", dur, actual);
    }

    #[test]
    fn duration_dots_increase_length(base in arb_base_duration()) {
        let d0 = Duration::new(base);
        let d1 = Duration::dotted(base, 1);
        let d2 = Duration::dotted(base, 2);
        prop_assert!(d1.actual_duration() > d0.actual_duration(),
            "single dot should increase duration");
        prop_assert!(d2.actual_duration() > d1.actual_duration(),
            "double dot should increase more than single");
    }

    #[test]
    fn duration_single_dot_is_one_and_half(base in arb_base_duration()) {
        let dotted = Duration::dotted(base, 1);
        let expected = base * Frac::new(3, 2);
        prop_assert_eq!(dotted.actual_duration(), expected);
    }

    #[test]
    fn duration_no_dots_no_tuplet_equals_base(base in arb_base_duration()) {
        let d = Duration::new(base);
        prop_assert_eq!(d.actual_duration(), base);
    }

    #[test]
    fn duration_tuplet_scaling(
        base in arb_base_duration(),
        normal in 2u8..5,
        actual in 2u8..5,
    ) {
        let d = Duration {
            base,
            dots: 0,
            tuplet_normal: normal,
            tuplet_actual: actual,
        };
        let expected = base * Frac::new(normal as i64, actual as i64);
        prop_assert_eq!(d.actual_duration(), expected);
    }
}

// ---------------------------------------------------------------------------
// Transform properties (Score-level)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn transpose_inverse(
        semitones in -12i32..13,
        steps in prop::collection::vec(arb_pitch_step(), 1..8),
        octave in 3i32..6,
    ) {
        let notes: Vec<Note> = steps
            .iter()
            .map(|&s| Note::new(Pitch::new(s, octave), Duration::quarter()))
            .collect();
        let score = make_score(notes);

        let original_midis = collect_midi(&score);

        let up = Transpose::new(semitones).apply(&score);
        let back = Transpose::new(-semitones).apply(&up);

        let roundtrip_midis = collect_midi(&back);
        prop_assert_eq!(roundtrip_midis, original_midis,
            "Transpose({}) then Transpose({}) should be identity on MIDI numbers",
            semitones, -semitones);
    }

    #[test]
    fn invert_self_inverse(
        steps in prop::collection::vec(arb_pitch_step(), 1..8),
        octave in 3i32..6,
        axis_step in arb_pitch_step(),
        axis_octave in 3i32..6,
    ) {
        let notes: Vec<Note> = steps
            .iter()
            .map(|&s| Note::new(Pitch::new(s, octave), Duration::quarter()))
            .collect();
        let score = make_score(notes);

        let original_midis = collect_midi(&score);

        let axis = Pitch::new(axis_step, axis_octave);
        let inv = Invert::new(axis);
        let once = inv.apply(&score);
        let twice = inv.apply(&once);

        let roundtrip_midis = collect_midi(&twice);
        prop_assert_eq!(roundtrip_midis, original_midis,
            "Invert should be self-inverse");
    }

    #[test]
    fn retrograde_self_inverse(
        steps in prop::collection::vec(arb_pitch_step(), 1..8),
        octave in 3i32..6,
    ) {
        let notes: Vec<Note> = steps
            .iter()
            .map(|&s| Note::new(Pitch::new(s, octave), Duration::quarter()))
            .collect();
        let score = make_score(notes);

        let original_midis = collect_midi(&score);

        let ret = Retrograde::new();
        let once = ret.apply(&score);
        let twice = ret.apply(&once);

        let roundtrip_midis = collect_midi(&twice);
        prop_assert_eq!(roundtrip_midis, original_midis,
            "Retrograde should be self-inverse");
    }

    #[test]
    fn transpose_preserves_note_count(
        semitones in -12i32..13,
        n in 1usize..10,
    ) {
        let notes: Vec<Note> = (0..n)
            .map(|i| {
                Note::new(
                    Pitch::new(PitchStep::from_index(i as i32 % 7), 4),
                    Duration::quarter(),
                )
            })
            .collect();
        let score = make_score(notes);

        let transposed = Transpose::new(semitones).apply(&score);
        prop_assert_eq!(collect_midi(&transposed).len(), n);
    }

    #[test]
    fn invert_preserves_note_count(
        n in 1usize..10,
    ) {
        let notes: Vec<Note> = (0..n)
            .map(|i| {
                Note::new(
                    Pitch::new(PitchStep::from_index(i as i32 % 7), 4),
                    Duration::quarter(),
                )
            })
            .collect();
        let score = make_score(notes);

        let inverted = Invert::default().apply(&score);
        prop_assert_eq!(collect_midi(&inverted).len(), n);
    }

    #[test]
    fn retrograde_preserves_note_count(
        n in 1usize..10,
    ) {
        let notes: Vec<Note> = (0..n)
            .map(|i| {
                Note::new(
                    Pitch::new(PitchStep::from_index(i as i32 % 7), 4),
                    Duration::quarter(),
                )
            })
            .collect();
        let score = make_score(notes);

        let reversed = Retrograde::new().apply(&score);
        prop_assert_eq!(collect_midi(&reversed).len(), n);
    }

    #[test]
    fn retrograde_reverses_order(
        steps in prop::collection::vec(arb_pitch_step(), 2..8),
        octave in 3i32..6,
    ) {
        let notes: Vec<Note> = steps
            .iter()
            .map(|&s| Note::new(Pitch::new(s, octave), Duration::quarter()))
            .collect();
        let score = make_score(notes);

        let original_midis = collect_midi(&score);

        let reversed_score = Retrograde::new().apply(&score);
        let reversed_midis = collect_midi(&reversed_score);

        // With a single measure, retrograde reverses element order.
        let mut expected = original_midis.clone();
        expected.reverse();
        prop_assert_eq!(reversed_midis, expected,
            "Single-measure retrograde should reverse note order");
    }

    #[test]
    fn transpose_then_invert_then_transpose(
        steps in prop::collection::vec(arb_pitch_step(), 1..6),
        octave in 3i32..6,
        semitones in -6i32..7,
    ) {
        // T(n) . I(axis) . T(-n) should equal I(axis_transposed)
        // but more simply: the composition should preserve note count.
        let notes: Vec<Note> = steps
            .iter()
            .map(|&s| Note::new(Pitch::new(s, octave), Duration::quarter()))
            .collect();
        let score = make_score(notes);
        let n = collect_midi(&score).len();

        let t = Transpose::new(semitones);
        let inv = Invert::default();
        let result = t.apply(&inv.apply(&t.apply(&score)));
        prop_assert_eq!(collect_midi(&result).len(), n);
    }
}
