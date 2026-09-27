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

// ---- Bars through the Score path (the lowering) ----

/// A pickup is a short first bar, and no note is copied into two bars (the
/// old lowering re-barred from 0 and duplicated notes crossing its grid).
#[test]
fn pickup_bar_is_short_and_no_note_is_duplicated() {
    use _core::ir::note::VoiceElement;
    let score = AbcToIrAdapter::new()
        .convert_str("X:1\nM:4/4\nL:1/8\nK:G\nD|G2A2 B4|c8|]\n")
        .unwrap();
    let part = &score.parts()[0];
    let notes = |m: &_core::ir::measure::Measure| {
        m.voices
            .iter()
            .flat_map(|v| &v.elements)
            .filter(|e| matches!(e, VoiceElement::Note(_)))
            .count()
    };
    assert_eq!(part.measures.len(), 3);
    assert!(part.measures[0].implicit, "the D is a pickup");
    assert_eq!(
        part.measures.iter().map(notes).collect::<Vec<_>>(),
        vec![1, 3, 1]
    );
}

/// Repeat bar lines land on the right side of their bars, so MusicXML and
/// LilyPond output keep the repeat.
#[test]
fn repeat_bar_lines_open_and_close_their_bars() {
    use _core::ir::direction::RepeatDirection;
    let score = AbcToIrAdapter::new()
        .convert_str(&read_abc("repeats.abc"))
        .unwrap();
    let m = &score.parts()[0].measures;
    assert_eq!(
        m[0].left_barline.as_ref().and_then(|b| b.repeat_direction),
        Some(RepeatDirection::Forward)
    );
    assert_eq!(
        m[m.len() - 1]
            .right_barline
            .as_ref()
            .and_then(|b| b.repeat_direction),
        Some(RepeatDirection::Backward)
    );
    let xml = IrToMxmlAdapter::new().convert(&score).unwrap();
    assert!(xml.contains(r#"<repeat direction="forward"/>"#), "{xml}");
    assert!(xml.contains(r#"<repeat direction="backward"/>"#), "{xml}");
}

/// Endings `[1` / `[2` are endings, not chords: read, barred and lifted back
/// into a repeat, the tune plays C D E F, C D G A.
#[test]
fn endings_are_read_as_endings() {
    let score = AbcToIrAdapter::new()
        .convert_str("X:1\nM:2/4\nL:1/4\nK:C\n|:C D|[1 E F:|[2 G A|]\n")
        .unwrap();
    let lifted = _core::ir::lift::lift_to_music(&score);
    let played: Vec<i32> = _core::representations::to_note_array(&lifted, 480)
        .notes
        .iter()
        .map(|n| n.pitch as i32)
        .collect();
    assert_eq!(played, vec![60, 62, 64, 65, 60, 62, 67, 69]);
}

/// ABC bar lines are real bar lines: a bar longer or shorter than its meter
/// keeps its length (a bar check in LilyPond only checks; here the bar line
/// is where the bar ends).
#[test]
fn irregular_bars_keep_their_length() {
    use _core::ir::duration::Frac;
    let lengths = |abc: &str| -> Vec<Frac> {
        let score = AbcToIrAdapter::new().convert_str(abc).unwrap();
        score.parts()[0]
            .measures
            .iter()
            .map(|m| {
                m.voices
                    .iter()
                    .filter(|v| v.number == 1)
                    .flat_map(|v| &v.elements)
                    .map(|e| e.metric_duration())
                    .sum()
            })
            .collect()
    };
    let q = |n| Frac::new(n, 4);
    // An overfull first bar, then a short bar mid-piece.
    assert_eq!(
        lengths("X:1\nM:4/4\nL:1/4\nK:C\nC D E F G|A B c d|e f|g a b c'|]\n"),
        vec![q(5), q(4), q(2), q(4)]
    );
    // Without a meter each bar is as long as its music, and nothing is tied.
    assert_eq!(
        lengths("X:1\nM:none\nL:1/4\nK:C\nC3 D3 E2|F2|\n"),
        vec![q(8), q(2)]
    );
    // A meter change at a bar line is not an irregular bar.
    assert_eq!(
        lengths("X:1\nM:4/4\nL:1/4\nK:C\nC D E F|[M:3/4] G A B|c d e|]\n"),
        vec![q(4), q(3), q(3)]
    );
}

/// Every tune of a file, each read on its own; the file header (before the
/// first `X:`) applies to all of them.
#[test]
fn every_tune_of_a_file() {
    use _core::ir::note::VoiceElement;
    let text = "%%propagate-accidentals pitch\n\nX:1\nK:C\n^C c|\n\nFree text.\n\nX:2\nT:Two\nK:G\nF G A|\n";
    let tunes = AbcToIrAdapter::new().convert_str_tunes(text).unwrap();
    assert_eq!(tunes.len(), 2);
    let pitches = |s: &_core::ir::score::Score| -> Vec<i32> {
        s.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.midi_number()),
                _ => None,
            })
            .collect()
    };
    // `pitch` propagation from the file header: the c is sharp too.
    assert_eq!(pitches(&tunes[0]), vec![61, 73]);
    assert_eq!(pitches(&tunes[1]), vec![66, 67, 69]);
    assert_eq!(tunes[1].metadata.title.as_deref(), Some("Two"));
    // Without X: the text is one tune.
    assert_eq!(
        AbcToIrAdapter::new()
            .convert_str_tunes("K:C\nC|\n")
            .unwrap()
            .len(),
        1
    );
}

/// What a score plays, repeats unfolded.
fn played(score: &_core::ir::score::Score) -> Vec<i32> {
    _core::representations::to_note_array(&_core::ir::lift::lift_to_music(score), 480)
        .notes
        .iter()
        .map(|n| n.pitch as i32)
        .collect()
}

fn via_abc(score: &_core::ir::score::Score) -> _core::ir::score::Score {
    let abc = IrToAbcAdapter::new()
        .convert_music(&_core::ir::lift::lift_to_music(score))
        .unwrap();
    AbcToIrAdapter::new()
        .convert_str(&abc)
        .unwrap_or_else(|e| panic!("{e}\n{abc}"))
}

/// Repeats play in the right order, read and after a trip through the writer:
/// a `|:` right after an ending, `::`, an ending the tune ends in, a last
/// ending closed by a plain bar line.
#[test]
fn repeat_structures_play_in_order() {
    let (c, d, e, f, g, a, b, c2) = (60, 62, 64, 65, 67, 69, 71, 72);
    for (abc, want) in [
        (
            "X:1\nM:2/4\nL:1/4\nK:C\n|:C D|1 E F:|2 G A|\n|:B c:|\n",
            vec![c, d, e, f, c, d, g, a, b, c2, b, c2],
        ),
        (
            "X:1\nM:2/4\nL:1/4\nK:C\n|:C D::E F:|\n",
            vec![c, d, c, d, e, f, e, f],
        ),
        (
            "X:1\nM:2/4\nL:1/4\nK:C\n|:C D|1 E F:|2 G A|\n",
            vec![c, d, e, f, c, d, g, a],
        ),
        (
            "X:1\nM:2/4\nL:1/4\nK:C\n|:C D|1 E F:|2 G A||B c|]\n",
            vec![c, d, e, f, c, d, g, a, b, c2],
        ),
        // No `|:`: the repeat goes back to the start.
        (
            "X:1\nM:2/4\nL:1/4\nK:C\nC D|1 E F:|2 G A|]\n",
            vec![c, d, e, f, c, d, g, a],
        ),
    ] {
        let score = AbcToIrAdapter::new().convert_str(abc).unwrap();
        assert_eq!(played(&score), want, "read: {abc}");
        assert_eq!(played(&via_abc(&score)), want, "written: {abc}");
    }
    // An ending the tune ends in is stopped (MusicXML needs the stop).
    let score = AbcToIrAdapter::new()
        .convert_str("X:1\nM:2/4\nL:1/4\nK:C\n|:C D|1 E F:|2 G A\n")
        .unwrap();
    let last = score.parts()[0].measures.last().unwrap();
    let stop = last
        .right_barline
        .as_ref()
        .and_then(|b| b.ending_type.clone());
    assert_eq!(stop.as_deref(), Some("stop"));
}

/// A repeat read from MusicXML — a backward repeat on a final-style bar line,
/// five times through — plays the same after a trip through ABC.
#[test]
fn musicxml_repeats_play_the_same_in_abc() {
    use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
    let src = std::fs::read_to_string("tests/fixtures/xml/45a-SimpleRepeat.xml").unwrap();
    let score = MxmlToIrAdapter::new().convert_str(&src).unwrap();
    assert_eq!(played(&via_abc(&score)), played(&score));
}

/// A bar longer than the meter stays one bar through the Music-tree writer.
#[test]
fn irregular_bars_are_written_whole() {
    let abc = "X:1\nM:4/4\nL:1/4\nK:C\ncdef|gabc'd'|c'4|]\n";
    let doc = AbcToIrAdapter::new().convert_str_to_music(abc).unwrap();
    let out = IrToAbcAdapter::new().convert_music(&doc).unwrap();
    let bars = |s: &str| {
        let score = AbcToIrAdapter::new().convert_str(s).unwrap();
        score.parts()[0].measures.len()
    };
    assert_eq!(bars(&out), bars(abc), "{out}");
}

/// A bar's inner voices are written as `&` layers, spacers as `x`, and a note
/// across a bar line is tied over it.
#[test]
fn writer_keeps_inner_voices_spacers_and_overflow() {
    use _core::ir::duration::{Duration, Frac};
    use _core::ir::measure::TimeSignature;
    use _core::ir::pitch::{Pitch, PitchStep};
    // Two voices in a bar, and a skip.
    let abc = "X:1\nM:2/4\nL:1/4\nK:C\nc d & E F|x G|\n";
    let score = AbcToIrAdapter::new().convert_str(abc).unwrap();
    let out = IrToAbcAdapter::new()
        .convert_music(&_core::ir::lift::lift_to_music(&score))
        .unwrap();
    assert!(out.contains('&') && out.contains('x'), "{out}");
    let back = AbcToIrAdapter::new().convert_str(&out).unwrap();
    assert_eq!(played(&back), played(&score), "{out}");
    // D is a half note starting on beat 2 of a 2/4 bar.
    let note = |step, q| Music::Note {
        pitch: Pitch::new(step, 4),
        duration: Duration::new(Frac::new(q, 4)),
        annotations: Vec::new(),
    };
    let doc = MusicDocument {
        metadata: Default::default(),
        music: Music::Sequential(vec![
            Music::TimeSignature(TimeSignature {
                beats: "2".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            note(PitchStep::C, 1),
            note(PitchStep::D, 2),
            note(PitchStep::E, 1),
        ]),
    };
    let out = IrToAbcAdapter::new().convert_music(&doc).unwrap();
    assert!(out.contains("D2- | D2"), "{out}");
}

/// Review findings on the reader: `Z2` is two bars; a byte-order mark, a
/// draft tune without `K:`, an overflowing meter and `L:0` don't break it.
#[test]
fn reader_edge_cases() {
    let score = AbcToIrAdapter::new()
        .convert_str("X:1\nM:4/4\nL:1/4\nK:C\nZ2|G4|\n")
        .unwrap();
    assert_eq!(score.parts()[0].measures.len(), 3);
    let tunes = AbcToIrAdapter::new()
        .convert_str_tunes("\u{FEFF}X:1\nT:A\nK:C\nABC|\n\nX:2\nT:B\nK:G\nGAB|\n\nX:3\nT:draft\n")
        .unwrap();
    let titles: Vec<_> = tunes.iter().map(|t| t.metadata.title.clone()).collect();
    assert_eq!(titles, [Some("A".to_string()), Some("B".to_string())]);
    for abc in [
        "X:1\nM:4294967295+1/4\nK:C\nC|\n",
        "X:1\nL:0\nK:C\n[CE]|\n",
        "X:1\nK:C\n[L:0/4][CE]|\n",
    ] {
        assert!(AbcToIrAdapter::new().convert_str(abc).is_ok(), "{abc}");
    }
}

/// Review findings on the fixes: a lone ending still repeats; a pickup is
/// not applied twice; a bar line at the very start doesn't close bar 1.
#[test]
fn lone_endings_pickups_and_leading_bar_lines() {
    let (a, b, c) = (69, 71, 72);
    let score = AbcToIrAdapter::new()
        .convert_str("X:1\nM:1/4\nL:1/4\nK:C\n|: A |1 B :| c |]\n")
        .unwrap();
    assert_eq!(played(&score), vec![a, b, a, c]);
    assert_eq!(played(&via_abc(&score)), vec![a, b, a, c]);

    use _core::transforms::retrograde::Retrograde;
    use _core::transforms::MusicTransform;
    let doc = AbcToIrAdapter::new()
        .convert_str_to_music("X:1\nM:4/4\nL:1/4\nK:C\nC | D E F G | A B |]\n")
        .unwrap();
    let out = IrToAbcAdapter::new()
        .convert_music(&Retrograde.apply_music(&doc))
        .unwrap();
    let body = out.lines().last().unwrap();
    let notes = |s: &str| {
        s.split_whitespace()
            .filter(|t| t.starts_with(|c: char| c.is_ascii_alphabetic()))
            .count()
    };
    let first_bar = body.split('|').find(|s| notes(s) > 0).unwrap();
    assert_eq!(notes(first_bar), 2, "{out}");

    let score = AbcToIrAdapter::new()
        .convert_str("X:1\nM:4/4\nL:1/4\nK:C\n[| C D E F | G A B c |]\n")
        .unwrap();
    let first = &score.parts()[0].measures[0];
    assert!(
        first
            .right_barline
            .as_ref()
            .is_none_or(|b| b.style == _core::ir::direction::BarlineType::Regular),
        "{:?}",
        first.right_barline
    );
}
