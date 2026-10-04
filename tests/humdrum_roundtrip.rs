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

#[test]
fn kern_writes_repeat_barlines() {
    // The lowering keeps volta repeats as bar lines (it used to write them
    // out), so the kern writer must write them or a repeat plays once.
    let before = from_xml("45a-SimpleRepeat.xml");
    let krn = IrToHumdrumAdapter::new()
        .convert(&before)
        .expect("IR → kern");
    assert!(krn.contains(":|!"), "backward repeat lost:\n{krn}");
    let repeats = |s: &Score| {
        s.parts()[0]
            .measures
            .iter()
            .filter(|m| {
                [&m.left_barline, &m.right_barline]
                    .into_iter()
                    .flatten()
                    .any(|b| b.repeat_direction.is_some())
            })
            .count()
    };
    assert_eq!(repeats(&before), repeats(&roundtrip(&before)));
}

#[test]
fn kern_reads_back_to_back_repeats() {
    use _core::ir::direction::{Barline, RepeatDirection::*};
    let krn = "**kern\n*M2/4\n4c\n4d\n=2:|!|:\n4e\n4f\n==:|!\n*-\n";
    let score = HumdrumToIrAdapter::new().convert_str(krn).expect("kern");
    let m = &score.parts()[0].measures;
    let dir = |b: &Option<Barline>| b.as_ref().and_then(|b| b.repeat_direction);
    assert_eq!(dir(&m[0].right_barline), Some(Backward));
    assert_eq!(dir(&m[1].left_barline), Some(Forward));
    assert_eq!(dir(&m[1].right_barline), Some(Backward));
}

/// Sounding (onset, pitch) pairs of a score: what a transposing
/// instrument plays.
fn sounding(score: &Score) -> Vec<(u32, u8)> {
    let doc = _core::ir::lift::lift_to_music(score);
    let mut v: Vec<(u32, u8)> = _core::representations::to_note_array(&doc, 480)
        .notes
        .iter()
        .map(|n| (n.onset, n.pitch))
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn transposing_instruments_sound_the_same_through_abc_and_kern() {
    use _core::adapters::abc_to_ir::AbcToIrAdapter;
    use _core::adapters::ir_to_abc::IrToAbcAdapter;
    use _core::adapters::FromMusicAdapter;
    for name in [
        "72a-TransposingInstruments",
        "72c-TransposingInstruments-Change",
    ] {
        let path = format!("tests/fixtures/xml/{name}.xml");
        let score = MxmlToIrAdapter::new()
            .convert_file(std::path::Path::new(&path))
            .unwrap();
        let kern = IrToHumdrumAdapter::new().convert(&score).unwrap();
        assert!(kern.contains("*ITrd"), "{name}: {kern}");
        let back = HumdrumToIrAdapter::new().convert_str(&kern).unwrap();
        assert_eq!(sounding(&back), sounding(&score), "{name} through kern");
        let abc = IrToAbcAdapter::new()
            .convert_music(&_core::ir::lift::lift_to_music(&score))
            .unwrap();
        assert!(abc.contains("transpose="), "{name}: {abc}");
        let back = AbcToIrAdapter::new().convert_str(&abc).unwrap();
        assert_eq!(sounding(&back), sounding(&score), "{name} through ABC");
    }
}

#[test]
fn transposition_comes_from_semitones_as_the_usual_interval() {
    use _core::ir::measure::Transpose;
    let t = |d, c, o| Transpose {
        diatonic: d,
        chromatic: c,
        octave_change: o,
    };
    assert_eq!(Transpose::from_semitones(-2), t(-1, -2, 0)); // B♭ clarinet
    assert_eq!(Transpose::from_semitones(-9), t(-5, -9, 0)); // E♭ alto sax
    assert_eq!(Transpose::from_semitones(-14), t(-1, -2, -1)); // tenor sax
    assert_eq!(Transpose::from_semitones(3), t(2, 3, 0)); // E♭ clarinet
    assert_eq!(Transpose::from_semitones(-14).semitones(), -14);
}
