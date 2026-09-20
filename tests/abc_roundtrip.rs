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

/// Number of top-level voices (Simultaneous branches), or 1 for a single staff.
fn voice_count(doc: &MusicDocument) -> usize {
    match &doc.music {
        Music::Simultaneous(b) => b.len(),
        _ => 1,
    }
}

#[test]
fn abc_multivoice_roundtrips() {
    let src = read_abc("multivoice.abc");
    let before = parse(&src);
    assert_eq!(voice_count(&before), 2, "fixture should parse to 2 voices");

    let abc = emit(&before);
    // Multi-voice ABC must emit `V:` blocks for both voices.
    assert!(abc.contains("V:1"), "no V:1 in emitted ABC:\n{abc}");
    assert!(abc.contains("V:2"), "no V:2 in emitted ABC:\n{abc}");

    let after = parse(&abc);
    assert_eq!(voice_count(&after), 2, "voices lost on round-trip:\n{abc}");
    assert_eq!(
        pitch_durations(&before),
        pitch_durations(&after),
        "pitch/duration changed on multi-voice round-trip:\n{abc}"
    );
    // Voice names survive (emitted on the V: line, re-read on parse).
    assert!(
        abc.contains("name=\"Right\""),
        "right voice name lost:\n{abc}"
    );
    assert!(
        abc.contains("name=\"Left\""),
        "left voice name lost:\n{abc}"
    );
}

#[test]
fn abc_multivoice_lowers_to_two_parts() {
    // A multi-voice ABC tune lowers to a Score with two parts.
    let score = AbcToIrAdapter::new()
        .convert_str(&read_abc("multivoice.abc"))
        .expect("ABC → Score");
    assert_eq!(score.parts().len(), 2, "expected 2 parts");
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

// ---------------------------------------------------------------------------
// Tuplets (regression: `(p:q:r` was dropped on both read and write, so triplet
// durations silently came back as plain ones)
// ---------------------------------------------------------------------------

#[test]
fn abc_tuplet_is_parsed_with_its_ratio() {
    let doc = parse("X:1\nM:4/4\nL:1/4\nK:C\n(3cde (3fga |\n");
    let pd = pitch_durations(&doc);
    assert_eq!(pd.len(), 6, "six triplet notes");
    for (_, n, d) in &pd {
        assert_eq!(
            (*n, *d),
            (1, 6),
            "triplet quarter sounds 1/6 whole, got {n}/{d}"
        );
    }
}

#[test]
fn abc_tuplet_shorthand_defaults() {
    // `(3` alone means 3-in-the-time-of-2 over 3 notes; `(2` is 2-in-3.
    let trip = pitch_durations(&parse("X:1\nM:4/4\nL:1/8\nK:C\n(3cde\n"));
    assert!(trip.iter().all(|(_, n, d)| (*n, *d) == (1, 12)));
    let duple = pitch_durations(&parse("X:1\nM:6/8\nL:1/8\nK:C\n(2cd\n"));
    assert!(
        duple.iter().all(|(_, n, d)| (*n, *d) == (3, 16)),
        "duplet eighth = 3/2 × 1/8 = 3/16, got {duple:?}"
    );
}

#[test]
fn abc_tuplet_survives_roundtrip() {
    let src = "X:1\nM:4/4\nL:1/4\nK:C\n(3cde (3fga |\n";
    let before = parse(src);
    let after = parse(&emit(&before));
    assert_eq!(
        pitch_durations(&before),
        pitch_durations(&after),
        "tuplet round-trip changed pitches/durations\nemitted:\n{}",
        emit(&before)
    );
    assert!(emit(&before).contains("(3:2:3"), "no tuplet marker emitted");
}

// ---------------------------------------------------------------------------
// Bar lines (regression: only *explicit* IR barlines were emitted, so scores
// coming from MusicXML/MIDI came out as one unbarred ABC measure)
// ---------------------------------------------------------------------------

#[test]
fn abc_emits_regular_barlines_from_the_meter() {
    // Eight quarter notes in 4/4 = two bars, even though the IR carries no
    // explicit Barline events for them.
    let doc = parse("X:1\nM:4/4\nL:1/4\nK:C\ncdef gabc\n");
    let out = emit(&doc);
    let body: String = out
        .lines()
        .filter(|l| !l.contains(':') || l.starts_with('|'))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        body.matches('|').count(),
        2,
        "expected two bar lines, got:\n{out}"
    );
}

#[test]
fn abc_wraps_long_bodies_into_lines() {
    let mut src = String::from("X:1\nM:4/4\nL:1/4\nK:C\n");
    for _ in 0..12 {
        src.push_str("cdef ");
    }
    let out = emit(&parse(&src));
    let body_lines = out.lines().filter(|l| l.starts_with('c')).count();
    assert!(body_lines >= 3, "12 bars should wrap over lines:\n{out}");
}

// ---------------------------------------------------------------------------
// Grace notes (regression: `Music::Grace` was written as nothing and `{…}` was
// skipped on read, so every grace note vanished on conversion)
// ---------------------------------------------------------------------------

#[test]
fn abc_grace_group_is_parsed() {
    let doc = parse("X:1\nM:4/4\nL:1/4\nK:C\n{d}c {/e}d |\n");
    fn graces(m: &Music, n: &mut usize) {
        if let Music::Grace { .. } = m {
            *n += 1;
        }
        match m {
            Music::Sequential(v) | Music::Simultaneous(v) => v.iter().for_each(|x| graces(x, n)),
            Music::Context { content, .. }
            | Music::Variable { content, .. }
            | Music::Grace { content, .. }
            | Music::Tuplet { content, .. } => graces(content, n),
            _ => {}
        }
    }
    let mut n = 0;
    graces(&doc.music, &mut n);
    assert_eq!(n, 2, "expected two grace groups");
}

#[test]
fn abc_grace_notes_survive_roundtrip() {
    let src = "X:1\nM:4/4\nL:1/4\nK:C\n{d}c {/e}d c c |\n";
    let before = parse(src);
    let out = emit(&before);
    assert!(out.contains('{'), "no grace group emitted:\n{out}");
    assert_eq!(
        pitch_durations(&before),
        pitch_durations(&parse(&out)),
        "grace notes lost on round-trip:\n{out}"
    );
}

#[test]
fn abc_grace_notes_do_not_move_the_bar_clock() {
    // Four quarters plus graces is still a single 4/4 bar.
    let out = emit(&parse("X:1\nM:4/4\nL:1/4\nK:C\n{d}c {e}d e f\n"));
    let body = out.lines().last().unwrap_or_default();
    assert_eq!(
        body.matches('|').count(),
        1,
        "expected one bar line:\n{out}"
    );
}
