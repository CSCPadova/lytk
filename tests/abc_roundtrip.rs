//! ABC adapter round-trip tests (Epic E, EET3).
//!
//! Verifies that parsing an ABC tune, emitting it back, and re-parsing
//! preserves the musical content (pitch multiset + duration sequence), and
//! that ABC cross-converts to the other formats.

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::ir_to_abc::IrToAbcAdapter;
use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
use _core::ir::music::{Music, MusicDocument};

fn read_abc(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/abc/{name}")).expect("read abc fixture")
}

fn parse(src: &str) -> MusicDocument {
    AbcToIrAdapter::new()
        .convert_str_to_music(src)
        .expect("ABC → Music")
}

fn emit(doc: &MusicDocument) -> String {
    IrToAbcAdapter::new()
        .convert_music(doc)
        .expect("Music → ABC")
}

/// Flatten the tree and collect (midi, numer, denom) for every note/chord-note.
fn pitch_durations(doc: &MusicDocument) -> Vec<(i32, i64, i64)> {
    fn walk(m: &Music, out: &mut Vec<(i32, i64, i64)>) {
        match m {
            Music::Sequential(v) | Music::Simultaneous(v) => {
                for x in v {
                    walk(x, out);
                }
            }
            Music::Context { content, .. }
            | Music::Variable { content, .. }
            | Music::Tuplet { content, .. } => walk(content, out),
            Music::Note {
                pitch, duration, ..
            } => {
                let d = duration.actual_duration();
                out.push((pitch.midi_number(), *d.numer(), *d.denom()));
            }
            Music::Chord {
                pitches, duration, ..
            } => {
                let d = duration.actual_duration();
                for (p, _) in pitches {
                    out.push((p.midi_number(), *d.numer(), *d.denom()));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&doc.music, &mut out);
    out
}

fn pitch_multiset(doc: &MusicDocument) -> Vec<i32> {
    let mut v: Vec<i32> = pitch_durations(doc)
        .into_iter()
        .map(|(m, _, _)| m)
        .collect();
    v.sort_unstable();
    v
}

fn assert_roundtrips(name: &str) {
    let src = read_abc(name);
    let before = parse(&src);
    let abc = emit(&before);
    let after = parse(&abc);
    assert_eq!(
        pitch_durations(&before),
        pitch_durations(&after),
        "{name}: pitch/duration changed on ABC round-trip.\nemitted:\n{abc}"
    );
}

#[test]
fn abc_roundtrip_simple() {
    assert_roundtrips("simple.abc");
}

#[test]
fn abc_roundtrip_repeats() {
    assert_roundtrips("repeats.abc");
    // The repeat barlines must survive.
    let doc = parse(&read_abc("repeats.abc"));
    let abc = emit(&doc);
    assert!(
        abc.contains("|:") && abc.contains(":|"),
        "repeats lost:\n{abc}"
    );
}

#[test]
fn abc_roundtrip_chords() {
    assert_roundtrips("chords.abc");
}

#[test]
fn abc_preserves_pitch_multiset() {
    for f in ["simple.abc", "repeats.abc", "chords.abc"] {
        let before = parse(&read_abc(f));
        let after = parse(&emit(&before));
        assert_eq!(pitch_multiset(&before), pitch_multiset(&after), "{f}");
    }
}

#[test]
fn abc_converts_to_lilypond() {
    // ABC → Score → LilyPond produces non-empty, note-bearing output.
    let score = AbcToIrAdapter::new()
        .convert_str(&read_abc("simple.abc"))
        .expect("ABC → Score");
    let ly = IrToLyAdapter::new().convert(&score).expect("Score → LY");
    assert!(ly.contains("\\new Staff"), "no staff in LY:\n{ly}");
    // C major scale starts on c.
    assert!(ly.contains('c'), "no notes in LY:\n{ly}");
}

#[test]
fn abc_converts_to_musicxml() {
    let score = AbcToIrAdapter::new()
        .convert_str(&read_abc("simple.abc"))
        .expect("ABC → Score");
    let xml = IrToMxmlAdapter::new().convert(&score).expect("Score → XML");
    assert!(
        xml.contains("<score-partwise"),
        "not MusicXML:\n{}",
        &xml[..xml.len().min(200)]
    );
    assert!(xml.contains("<pitch>"), "no pitches in MusicXML");
}
