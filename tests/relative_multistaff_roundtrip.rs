//! Regression: the Score-path LilyPond emitter (what Python `to_lilypond(score)`
//! uses) must not octave-shift notes on a relative multi-staff / multi-voice
//! score. Relative octave threading only round-trips for simple single-staff
//! single-voice parts; multi-staff (piano) and multi-voice `<< \\ >>` parts are
//! emitted with absolute octaves instead. Either way the pitch multiset must be
//! preserved across a parse → emit → parse round-trip.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::note::VoiceElement;
use _core::ir::Score;

fn pitch_multiset(score: &Score) -> Vec<i32> {
    let mut all = Vec::new();
    for part in score.parts() {
        for m in &part.measures {
            for v in &m.voices {
                for e in &v.elements {
                    match e {
                        VoiceElement::Note(n) => all.push(n.pitch.midi_number()),
                        VoiceElement::Chord(c) => {
                            all.extend(c.notes.iter().map(|n| n.pitch.midi_number()))
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    all.sort();
    all
}

/// parse → Score-path emit → parse, asserting the pitch multiset is preserved.
fn assert_score_path_roundtrip(fixture: &str) {
    let src = std::fs::read_to_string(fixture).unwrap_or_else(|_| panic!("read {fixture}"));
    let parser = LyToIrAdapter::new();
    let score1 = parser.convert_str(&src).expect("parse 1");
    let before = pitch_multiset(&score1);

    // Mirror Python `to_lilypond(score)`: the Score-path emitter.
    let ly = IrToLyAdapter::new().convert(&score1).expect("emit");
    let score2 = parser.convert_str(&ly).expect("parse 2");
    let after = pitch_multiset(&score2);

    assert_eq!(
        before, after,
        "{fixture}: Score-path round-trip changed the pitch multiset (octave shift)"
    );
}

#[test]
fn pedal_relative_multistaff_roundtrips() {
    assert_score_path_roundtrip("tests/fixtures/ly/pedal.ly");
}

#[test]
fn chopin_relative_multistaff_roundtrips() {
    assert_score_path_roundtrip("tests/fixtures/ly/chopin_n.ly");
}

#[test]
fn simple_single_staff_stays_relative() {
    // A simple single-staff single-voice part keeps the tidy `\relative` form
    // (and still round-trips its pitches).
    let parser = LyToIrAdapter::new();
    let score = parser
        .convert_str(r#"\relative c' { c4 d e f g a b c }"#)
        .expect("parse");
    let ly = IrToLyAdapter::new().convert(&score).expect("emit");
    assert!(
        ly.contains("\\relative"),
        "simple single-staff input should still emit \\relative; got:\n{ly}"
    );
    let score2 = parser.convert_str(&ly).expect("reparse");
    assert_eq!(pitch_multiset(&score), pitch_multiset(&score2));
}
