//! Humdrum (`**kern`) round-trip tests.
//!
//! The `.krn` format had no round-trip coverage of its own — only CLI smoke
//! tests — so this file pins what a kern round-trip preserves.

use _core::adapters::humdrum_to_ir::HumdrumToIrAdapter;
use _core::adapters::ir_to_humdrum::IrToHumdrumAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::score::Score;

fn notes(score: &Score) -> Vec<(i32, i64, i64)> {
    let mut out = Vec::new();
    for part in score.parts() {
        for measure in &part.measures {
            for voice in &measure.voices {
                for elem in &voice.elements {
                    use _core::ir::note::VoiceElement;
                    match elem {
                        VoiceElement::Note(n) => {
                            let d = n.duration.actual_duration();
                            out.push((n.pitch.midi_number(), *d.numer(), *d.denom()));
                        }
                        VoiceElement::Chord(c) => {
                            let d = c.duration.actual_duration();
                            for n in &c.notes {
                                out.push((n.pitch.midi_number(), *d.numer(), *d.denom()));
                            }
                        }
                        VoiceElement::Rest(_) => {}
                    }
                }
            }
        }
    }
    out
}

fn from_xml(fixture: &str) -> Score {
    let src = std::fs::read_to_string(format!("tests/fixtures/xml/{fixture}")).expect("fixture");
    MxmlToIrAdapter::new().convert_str(&src).expect("XML → IR")
}

fn roundtrip(score: &Score) -> Score {
    let krn = IrToHumdrumAdapter::new().convert(score).expect("IR → kern");
    HumdrumToIrAdapter::new()
        .convert_str(&krn)
        .expect("kern → IR")
}

#[test]
fn kern_roundtrip_preserves_pitches_and_durations() {
    for fixture in [
        "01a-Pitches-Pitches.xml",
        "02a-Rests-Durations.xml",
        "13a-KeySignatures.xml",
        "21a-Chord-Basic.xml",
        "32a-Notations.xml",
    ] {
        let before = from_xml(fixture);
        let after = roundtrip(&before);
        assert_eq!(
            notes(&before),
            notes(&after),
            "{fixture} changed through **kern"
        );
    }
}

#[test]
fn kern_grace_notes_get_their_own_record() {
    // Regression: events were keyed by onset alone, so a zero-duration grace
    // note overwrote the note it decorates and vanished from the output.
    let before = from_xml("24a-GraceNotes.xml");
    let krn = IrToHumdrumAdapter::new()
        .convert(&before)
        .expect("IR → kern");
    let graces = krn.matches('q').count() + krn.matches('Q').count();
    assert!(graces >= 10, "grace notes dropped from kern output:\n{krn}");
    assert_eq!(
        notes(&before).len(),
        notes(&roundtrip(&before)).len(),
        "grace notes lost on kern round-trip"
    );
}

#[test]
fn kern_tuplets_keep_their_ratio() {
    let before = from_xml("23a-Tuplets.xml");
    assert_eq!(
        notes(&before),
        notes(&roundtrip(&before)),
        "tuplet durations changed through **kern"
    );
}
