//! Per-fixture MIDI round-trip coverage (`MIDI → IR → MIDI → IR`).
//!
//! The aggregate [`fidelity`](../fidelity.rs) scoreboard gates the whole corpus
//! on a non-decreasing baseline; this file pins the *per-fixture* guarantee so a
//! regression names the exact fixture that broke.
//!
//! After the export-side carried-meter fix (`ir_to_midi::build_part_track` sizes
//! every bar by the running time signature), three fixtures round-trip with a
//! stable note-count AND pitch multiset; the remaining two are documented,
//! gated limitations (see `fidelity.rs` and `docs/devlog.md`).

mod common;

use common::{note_signature, pitch_multiset};

use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::ir::score::Score;

fn import(name: &str) -> Score {
    let bytes = std::fs::read(format!("tests/fixtures/midi/{name}")).expect("read fixture");
    MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI → IR")
}

/// Round-trip a score: IR → MIDI → IR.
fn roundtrip(score: &Score) -> Score {
    let bytes = IrToMidiAdapter::new()
        .convert_bytes(score)
        .expect("IR → MIDI");
    MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI → IR (re-import)")
}

/// A fixture whose note-count, pitch multiset AND (onset, duration) signature
/// are all preserved through a MIDI round-trip.
fn assert_fully_stable(name: &str) {
    let before = import(name);
    let after = roundtrip(&before);
    assert_eq!(
        note_signature(&before),
        note_signature(&after),
        "{name}: (onset, duration, pitch) signature drifted on MIDI round-trip"
    );
}

/// A fixture whose note-count and pitch multiset survive (the sound is
/// preserved) even if exact onsets/durations re-quantize.
fn assert_pitch_stable(name: &str) {
    let before = import(name);
    let after = roundtrip(&before);
    assert!(
        before
            .parts()
            .iter()
            .map(|p| p.measures.len())
            .sum::<usize>()
            > 0,
        "{name}: imported empty"
    );
    assert_eq!(
        pitch_multiset(&before),
        pitch_multiset(&after),
        "{name}: pitch multiset changed on MIDI round-trip"
    );
}

#[test]
fn midi_example_fully_stable() {
    assert_fully_stable("example.midi");
}

#[test]
fn midi_example2_0_fully_stable() {
    assert_fully_stable("example2_0.midi");
}

#[test]
fn midi_example2_1_pitch_stable() {
    // After the carried-meter fix this matches on note-count + pitch multiset;
    // one note's duration still drifts across a meter boundary (see fidelity.rs).
    assert_pitch_stable("example2_1.midi");
}

#[test]
fn midi_drifters_do_not_panic_and_keep_notes() {
    // pedal + chopin_n are documented round-trip limitations (a quantized-budget
    // cascade and a non-bar-aligned source meter, respectively). They must still
    // import, export and re-import without panicking, and keep (most of) their
    // notes — the sound survives even though the notation isn't note-for-note
    // stable. This guards against a *total* loss regression.
    for name in ["pedal.midi", "chopin_n.midi"] {
        let before = import(name);
        let after = roundtrip(&before);
        let nb = before.parts().iter().flat_map(|p| &p.measures).count();
        let na = after.parts().iter().flat_map(|p| &p.measures).count();
        assert!(nb > 0 && na > 0, "{name}: lost all measures on round-trip");
    }
}

/// Regression (review R5): the reader pairs overlapping same-pitch notes
/// first-in first-out, so a file holding `<< { c1 } \\ { c2 c2 } >>` as three
/// overlapping notes keeps all three (it used to keep one pending note-on per
/// key, losing the whole note and truncating the halves).
#[test]
fn overlapping_unison_in_a_file_keeps_every_note() {
    use common::smf::{build, Ev, Track};
    let bytes = build(
        480,
        &[(0, Ev::Time(4, 4))],
        &[Track::new(
            "p",
            0,
            &[(0, 1920, 60, 90), (0, 960, 60, 90), (960, 1920, 60, 90)],
        )],
    );
    let score = MidiToIrAdapter::new().convert_bytes(&bytes).unwrap();
    assert_eq!(
        pitch_multiset(&score).len(),
        3,
        "a unison overlap lost notes"
    );
}

/// A channel can't sound one key twice, so the writer plays a unison as
/// LilyPond's MIDI walker does (MuseScore's export agrees): `c'1` and `c'2`
/// starting together are one note, and the second `c'2` re-strikes it.
#[test]
fn unison_across_voices_plays_as_lilypond_does() {
    use _core::adapters::ly_to_ir::LyToIrAdapter;
    use _core::adapters::ToIrAdapter;

    let score = LyToIrAdapter::new()
        .convert_str(r#"\score { \new Staff << { c'1 } \\ { c'2 c'2 } >> }"#)
        .unwrap();
    let bytes = IrToMidiAdapter::new().convert_bytes(&score).unwrap();
    let notes: Vec<(u32, u32, u8)> = common::smf::notes(&bytes)
        .iter()
        .map(|n| (n.on, n.off, n.pitch))
        .collect();
    assert_eq!(notes, vec![(0, 768, 60), (768, 1536, 60)]);
}
