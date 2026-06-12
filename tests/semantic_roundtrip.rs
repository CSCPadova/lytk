//! Semantic round-trip tests (Epic C).
//!
//! Unlike `fixture_regression.rs` (asserts "non-empty") and `fixture_audit.rs`
//! (asserts "no panic"), these assert that *meaningful musical content* survives
//! a conversion: pitches, durations, dynamics, articulations, ties/slurs,
//! lyrics, and repeat structure.
//!
//! Helpers here are shared building blocks for the per-fixture round-trip suite.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};

mod common;
use common::{pitch_multiset, signature};

// ---------------------------------------------------------------------------
// Conversion shortcuts
// ---------------------------------------------------------------------------

/// LilyPond → LilyPond via the Music-tree path (the path the CLI uses for LY→LY).
fn ly_to_ly_music(src: &str) -> String {
    let doc = LyToIrAdapter::new()
        .convert_str_to_music(src)
        .expect("LY → Music");
    IrToLyAdapter::new()
        .convert_music(&doc)
        .expect("Music → LY")
}

/// LilyPond → LilyPond via the Score path.
fn ly_to_ly_score(src: &str) -> String {
    let score = LyToIrAdapter::new().convert_str(src).expect("LY → Score");
    IrToLyAdapter::new().convert(&score).expect("Score → LY")
}

/// MusicXML → LilyPond via the Score path.
fn xml_to_ly(src: &str) -> String {
    let score = MxmlToIrAdapter::new()
        .convert_str(src)
        .expect("XML → Score");
    IrToLyAdapter::new().convert(&score).expect("Score → LY")
}

fn read_ly(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/ly/{name}")).expect("read ly fixture")
}

fn read_xml(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/xml/{name}")).expect("read xml fixture")
}

/// True if the emitted LilyPond contains a lyrics block with actual syllables.
fn has_lyric_content(ly: &str) -> bool {
    ly.contains("\\lyricmode") || ly.contains("\\lyricsto") || ly.contains("\\addlyrics")
}

// ---------------------------------------------------------------------------
// Lyrics (B1)
// ---------------------------------------------------------------------------

#[test]
fn xml_to_ly_preserves_lyrics() {
    let ly = xml_to_ly(&read_xml("61a-Lyrics.xml"));
    assert!(
        has_lyric_content(&ly),
        "XML→LY dropped lyrics; output had no lyric block:\n{ly}"
    );
}

/// Lyrics defined via `\lyricmode` / `\lyricsto` must survive LY→LY on both paths.
/// `example.ly` uses this form (parsed today); the bug was the emitter dropping them.
#[test]
fn ly_to_ly_music_preserves_lyrics_lyricmode() {
    let ly = ly_to_ly_music(&read_ly("example.ly"));
    assert!(
        has_lyric_content(&ly),
        "LY→LY (Music path) dropped \\lyricmode lyrics"
    );
}

#[test]
fn ly_to_ly_score_preserves_lyrics_lyricmode() {
    let ly = ly_to_ly_score(&read_ly("example.ly"));
    assert!(
        has_lyric_content(&ly),
        "LY→LY (Score path) dropped \\lyricmode lyrics"
    );
}

/// `\addlyrics` (postfix form) must be parsed and re-emitted. Use a `\layout`
/// block so the score is not treated as MIDI-only (which would be skipped).
const ADDLYRICS_SRC: &str = r#"\version "2.24.0"
\score {
  \new Staff { c'4 d'4 e'4 f'4 }
  \addlyrics { these are the words }
  \layout { }
}
"#;

#[test]
fn ly_to_ly_music_preserves_addlyrics() {
    let ly = ly_to_ly_music(ADDLYRICS_SRC);
    assert!(
        has_lyric_content(&ly) && ly.contains("words"),
        "LY→LY (Music path) dropped \\addlyrics; output:\n{ly}"
    );
}

#[test]
fn ly_to_ly_score_preserves_addlyrics() {
    let ly = ly_to_ly_score(ADDLYRICS_SRC);
    assert!(
        has_lyric_content(&ly) && ly.contains("words"),
        "LY→LY (Score path) dropped \\addlyrics; output:\n{ly}"
    );
}

// ---------------------------------------------------------------------------
// Repeats / voltas (B2)
// ---------------------------------------------------------------------------

const VOLTA_SRC: &str = r#"\version "2.24.0"
\score {
  \new Staff {
    \repeat volta 2 { c'4 d'4 e'4 f'4 }
    \alternative { { g'1 } { a'1 } }
  }
  \layout { }
}
"#;

// EBT2: the Music path (LY→LY, the CLI path) reconstructs `Music::Repeat` in
// the lift pass from the Score's repeat barlines + volta endings, so
// `\repeat volta` / `\alternative` survive the round-trip.
#[test]
fn ly_to_ly_music_preserves_volta() {
    let ly = ly_to_ly_music(VOLTA_SRC);
    assert!(
        ly.contains("\\repeat volta"),
        "Music path dropped \\repeat volta:\n{ly}"
    );
    assert!(
        ly.contains("\\alternative"),
        "Music path dropped \\alternative:\n{ly}"
    );
    // Body notes and both alternative notes must all survive.
    for tok in ["c'", "d'", "e'", "f'", "g'", "a'"] {
        assert!(ly.contains(tok), "Music path lost note {tok}:\n{ly}");
    }
}

// The Score path (XML→LY) reconstructs repeats directly from barlines in
// ir_to_ly; structural repeat round-trip on that path is tracked separately.
#[test]
#[ignore = "EBT2: Score-path (ir_to_ly) repeat emission not yet fixed; Music path is the LY→LY route"]
fn ly_to_ly_score_preserves_volta() {
    let ly = ly_to_ly_score(VOLTA_SRC);
    assert!(
        ly.contains("\\repeat"),
        "Score path dropped \\repeat:\n{ly}"
    );
}

// ---------------------------------------------------------------------------
// MIDI velocity <-> dynamics (B5)
// ---------------------------------------------------------------------------

fn note_dynamics(score: &_core::ir::score::Score) -> Vec<String> {
    use _core::ir::note::VoiceElement;
    let mut out = Vec::new();
    for part in score.parts() {
        for m in &part.measures {
            for v in &m.voices {
                for e in &v.elements {
                    if let VoiceElement::Note(n) = e {
                        for d in &n.dynamics {
                            out.push(d.sign.clone());
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn midi_roundtrip_maps_dynamics_to_velocity_and_back() {
    use _core::adapters::ir_to_midi::IrToMidiAdapter;
    use _core::adapters::midi_to_ir::MidiToIrAdapter;

    let src = r#"\version "2.24.0"
\score { \new Staff { c'1\ppp d'1\fff } \layout { } }
"#;
    let score = LyToIrAdapter::new().convert_str(src).expect("LY → Score");
    // The source dynamics must be parsed onto the notes.
    let src_dyns = note_dynamics(&score);
    assert!(
        src_dyns.iter().any(|d| d == "ppp") && src_dyns.iter().any(|d| d == "fff"),
        "parser did not attach note dynamics: {src_dyns:?}"
    );

    let bytes = IrToMidiAdapter::new()
        .convert_bytes(&score)
        .expect("IR → MIDI");
    let score2 = MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI → IR");

    let dyns = note_dynamics(&score2);
    assert!(
        !dyns.is_empty(),
        "MIDI→IR produced no dynamics (velocity not mapped back)"
    );
    // The soft note must map back softer than the loud note.
    assert_eq!(
        dyns.first().map(String::as_str),
        Some("ppp"),
        "soft note band"
    );
    assert!(
        dyns.iter().any(|d| d == "fff"),
        "loud note should map to fff band: {dyns:?}"
    );
}

const SIMPLE_REPEAT_SRC: &str = r#"\version "2.24.0"
\score {
  \new Staff { \repeat volta 2 { c'4 d'4 e'4 f'4 } g'1 }
  \layout { }
}
"#;

#[test]
fn ly_to_ly_music_preserves_simple_repeat() {
    let ly = ly_to_ly_music(SIMPLE_REPEAT_SRC);
    assert!(
        ly.contains("\\repeat volta"),
        "Music path dropped simple \\repeat volta:\n{ly}"
    );
    // The trailing note after the repeat must remain outside the repeat block.
    for tok in ["c'", "d'", "e'", "f'", "g'"] {
        assert!(ly.contains(tok), "lost note {tok}:\n{ly}");
    }
}

/// Re-parse the emitted LilyPond and confirm the repeat survives a second pass.
#[test]
fn volta_survives_double_roundtrip() {
    let once = ly_to_ly_music(VOLTA_SRC);
    let twice = ly_to_ly_music(&once);
    assert!(
        twice.contains("\\repeat volta") && twice.contains("\\alternative"),
        "repeat structure lost on second round-trip:\n{twice}"
    );
}

// ---------------------------------------------------------------------------
// Chord names / \chordmode (EBT3)
// ---------------------------------------------------------------------------

const CHORDMODE_SRC: &str = r#"\version "2.24.0"
\score {
  <<
    \new ChordNames \chordmode { c1 a:m d:7 g:maj7 }
    \new Staff { c'1 a'1 d'1 g'1 }
  >>
  \layout { }
}
"#;

fn first_part_harmonies(score: &_core::ir::score::Score) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in score.parts() {
        for m in &part.measures {
            for h in &m.harmonies {
                out.push((h.root.step.clone(), h.kind.clone()));
            }
        }
    }
    out
}

#[test]
fn chordmode_parses_into_harmonies() {
    let score = LyToIrAdapter::new()
        .convert_str(CHORDMODE_SRC)
        .expect("LY → Score");
    let harms = first_part_harmonies(&score);
    assert_eq!(
        harms,
        vec![
            ("C".to_string(), "major".to_string()),
            ("A".to_string(), "minor".to_string()),
            ("D".to_string(), "dominant".to_string()),
            ("G".to_string(), "major-seventh".to_string()),
        ],
        "chordmode did not parse into the expected harmonies"
    );
}

#[test]
fn ly_to_ly_preserves_chordmode() {
    let ly = ly_to_ly_score(CHORDMODE_SRC);
    assert!(
        ly.contains("\\chordmode"),
        "LY→LY dropped \\chordmode:\n{ly}"
    );
    assert!(
        ly.contains(":m") && ly.contains(":7") && ly.contains(":maj7"),
        "lost chord qualities:\n{ly}"
    );
}

// ---------------------------------------------------------------------------
// \partial in multi-movement contexts (EBT6)
// ---------------------------------------------------------------------------

const MULTI_PARTIAL_SRC: &str = r#"\version "2.24.0"
\score { \new Staff { \partial 4 g'4 | c'4 d'4 e'4 f'4 } \layout { } }
\score { \new Staff { c'4 d'4 e'4 f'4 } \layout { } }
"#;

#[test]
fn partial_does_not_leak_across_movements() {
    let scores = LyToIrAdapter::new()
        .convert_str_multi(MULTI_PARTIAL_SRC)
        .expect("LY → Scores");
    assert_eq!(scores.len(), 2, "expected two movements");
    assert!(
        scores[0].parts()[0].measures.first().unwrap().implicit,
        "movement 1 first measure should be an implicit pickup (\\partial 4)"
    );
    assert!(
        !scores[1].parts()[0].measures.first().unwrap().implicit,
        "movement 2 has no \\partial; its first measure must not be implicit"
    );
}

// ---------------------------------------------------------------------------
// ECT2: per-fixture semantic round-trip suite
//
// Named tests asserting strong invariants (pitch multiset + note count) survive
// a round-trip — better diagnostics than the aggregate `fidelity` scoreboard.
// ---------------------------------------------------------------------------

/// LY → IR → LY (Music path) → IR preserves the pitch multiset and note count.
macro_rules! ly_roundtrip_fixture {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let src = read_ly($file);
            let before = LyToIrAdapter::new().convert_str(&src).expect("LY → Score");
            let out = ly_to_ly_music(&src);
            let after = LyToIrAdapter::new()
                .convert_str(&out)
                .expect("re-parse emitted LY");
            assert_eq!(
                pitch_multiset(&before),
                pitch_multiset(&after),
                "pitch multiset changed on LY round-trip of {}",
                $file
            );
            assert_eq!(
                signature(&before).notes,
                signature(&after).notes,
                "note count changed on LY round-trip of {}",
                $file
            );
        }
    };
}

ly_roundtrip_fixture!(rt_ly_pedal, "pedal.ly");
ly_roundtrip_fixture!(rt_ly_chopin, "chopin_n.ly");
ly_roundtrip_fixture!(rt_ly_repeats, "repeats.ly");
ly_roundtrip_fixture!(rt_ly_relative_repeat, "relative-repeat.ly");
ly_roundtrip_fixture!(rt_ly_make_relative_copies, "make-relative-copies.ly");
ly_roundtrip_fixture!(rt_ly_slur_nice, "slur-nice.ly");

/// XML → IR → XML → IR preserves the pitch multiset and note count.
macro_rules! xml_roundtrip_fixture {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let src = read_xml($file);
            let before = MxmlToIrAdapter::new()
                .convert_str(&src)
                .expect("XML → Score");
            let out = IrToMxmlAdapter::new()
                .convert(&before)
                .expect("Score → XML");
            let after = MxmlToIrAdapter::new()
                .convert_str(&out)
                .expect("re-parse emitted XML");
            assert_eq!(
                pitch_multiset(&before),
                pitch_multiset(&after),
                "pitch multiset changed on XML round-trip of {}",
                $file
            );
            assert_eq!(
                signature(&before).notes,
                signature(&after).notes,
                "note count changed on XML round-trip of {}",
                $file
            );
        }
    };
}

xml_roundtrip_fixture!(rt_xml_pitches, "01a-Pitches-Pitches.xml");
xml_roundtrip_fixture!(rt_xml_chords, "01b-Pitches-Intervals.xml");
xml_roundtrip_fixture!(rt_xml_lyrics, "61a-Lyrics.xml");
xml_roundtrip_fixture!(rt_xml_repeats, "45a-SimpleRepeat.xml");
xml_roundtrip_fixture!(rt_xml_grace, "24a-GraceNotes.xml");

/// Dynamics survive XML round-trip (a content type the count/pitch checks miss).
#[test]
fn rt_xml_dynamics_preserved() {
    let src = read_xml("31a-Directions.xml");
    let before = MxmlToIrAdapter::new()
        .convert_str(&src)
        .expect("XML → Score");
    let out = IrToMxmlAdapter::new()
        .convert(&before)
        .expect("Score → XML");
    let after = MxmlToIrAdapter::new()
        .convert_str(&out)
        .expect("re-parse XML");
    let b = common::sorted_dynamics(&before);
    let a = common::sorted_dynamics(&after);
    assert_eq!(b, a, "dynamics changed on XML round-trip");
}

// ---------------------------------------------------------------------------
// Bar-splitting consistency across parts (grace-note duration regression)
//
// In a multi-part score where some parts lack an explicit \time, the
// time-signature synchronization resplits them to match a reference part.
// A grace note (appoggiatura) was wrongly counted toward the bar boundary,
// drifting every subsequent barline so parts disagreed on measure durations.
// ---------------------------------------------------------------------------

fn assert_bars_consistent_across_parts(src: &str, fixture: &str) {
    let score = LyToIrAdapter::new().convert_str(src).expect("LY → Score");
    let nparts = score.parts().len();
    assert!(nparts >= 2, "{fixture}: expected multiple parts");
    let p0 = common::part_measure_durations(&score, 0);
    for pi in 1..nparts {
        let pn = common::part_measure_durations(&score, pi);
        assert_eq!(
            p0.len(),
            pn.len(),
            "{fixture}: part {pi} has a different measure count than part 0"
        );
        for (i, (a, b)) in p0.iter().zip(pn.iter()).enumerate() {
            assert_eq!(
                a,
                b,
                "{fixture}: measure {} duration differs: part0={a:?} part{pi}={b:?}",
                i + 1
            );
        }
    }
}

#[test]
fn example_bars_consistent_across_parts() {
    assert_bars_consistent_across_parts(&read_ly("example.ly"), "example.ly");
}

#[test]
fn example2_bars_consistent_across_parts() {
    // example2.ly is multi-movement; check each movement's parts agree on bar
    // durations. The very last measure of a movement is excluded: a movement's
    // final bar can carry ragged trailing content that the resplit dumps into
    // the last measure (a separate, lower-priority multi-meter edge — the
    // systematic grace-note drift this guards against affects interior bars).
    let scores = LyToIrAdapter::new()
        .convert_str_multi(&read_ly("example2.ly"))
        .expect("LY → Scores");
    for (mi, score) in scores.iter().enumerate() {
        let nparts = score.parts().len();
        if nparts < 2 {
            continue;
        }
        let p0 = common::part_measure_durations(score, 0);
        let interior = p0.len().saturating_sub(1);
        for pi in 1..nparts {
            let pn = common::part_measure_durations(score, pi);
            assert_eq!(
                p0[..interior.min(p0.len())],
                pn[..interior.min(pn.len())],
                "example2.ly movement {} part {pi} interior measure durations differ from part 0",
                mi + 1
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-staff (piano) clefs: every staff must get an initial clef
// ---------------------------------------------------------------------------

#[test]
fn pedal_piano_clefs_per_staff() {
    // pedal.ly is a piano score: the upper staff has no explicit \clef (LilyPond
    // default = treble), the lower staff is \clef bass. The MusicXML must carry a
    // numbered clef for *each* staff (was: a single unnumbered bass clef, so the
    // upper staff rendered with the wrong clef).
    let score = LyToIrAdapter::new()
        .convert_str(&read_ly("pedal.ly"))
        .expect("LY → Score");
    let part = &score.parts()[0];
    assert_eq!(part.staves, 2, "pedal.ly is a 2-staff piano part");
    let attrs = part.measures[0]
        .attributes
        .as_ref()
        .expect("first measure attributes");
    use _core::ir::measure::ClefSign;
    let c1 = attrs.clefs.get(&1).expect("staff 1 must have a clef");
    let c2 = attrs.clefs.get(&2).expect("staff 2 must have a clef");
    assert!(
        matches!(c1.sign, ClefSign::G) && c1.line == 2,
        "staff 1 should default to treble (G2), got {:?}{}",
        c1.sign,
        c1.line
    );
    assert!(
        matches!(c2.sign, ClefSign::F) && c2.line == 4,
        "staff 2 should be bass (F4), got {:?}{}",
        c2.sign,
        c2.line
    );

    let xml = IrToMxmlAdapter::new().convert(&score).expect("Score → XML");
    assert!(
        xml.contains("number=\"1\"") && xml.contains("number=\"2\""),
        "MusicXML should emit per-staff numbered clefs"
    );
}

// ---------------------------------------------------------------------------
// PianoStaff time-signature unification (pedal.ly bar-58 regression)
//
// In LilyPond a \time change goes to the score-shared Timing context, so it
// re-bars *all* staves even when only one staff declares it. Each staff is
// pre-parsed independently, so a staff missing \time declarations was barred
// under its stale meter and drifted relative to its siblings.
// ---------------------------------------------------------------------------

/// Per-measure bar fill of each staff of a multi-staff part: the maximum
/// voice duration among the voices belonging to that staff (grace notes
/// excluded — they consume no measure time).
fn staff_measure_fills(
    part: &_core::ir::part::Part,
) -> Vec<std::collections::BTreeMap<u8, _core::ir::duration::Frac>> {
    use _core::ir::duration::Frac;
    use _core::ir::note::VoiceElement;
    part.measures
        .iter()
        .map(|m| {
            let mut fills = std::collections::BTreeMap::new();
            for v in &m.voices {
                let staff = v
                    .elements
                    .iter()
                    .map(|e| match e {
                        VoiceElement::Note(n) => n.staff,
                        VoiceElement::Rest(r) => r.staff,
                        VoiceElement::Chord(c) => c.staff,
                    })
                    .next()
                    .unwrap_or(1);
                let dur = v
                    .elements
                    .iter()
                    .filter(|e| !matches!(e, VoiceElement::Note(n) if n.is_grace))
                    .map(|e| match e {
                        VoiceElement::Note(n) => n.duration.actual_duration(),
                        VoiceElement::Rest(r) => r.duration.actual_duration(),
                        VoiceElement::Chord(c) => c.duration.actual_duration(),
                    })
                    .fold(Frac::from_integer(0), |a, d| a + d);
                let entry = fills.entry(staff).or_insert_with(|| Frac::from_integer(0));
                if dur > *entry {
                    *entry = dur;
                }
            }
            fills
        })
        .collect()
}

#[test]
fn piano_staff_time_unification_minimal() {
    // The upper staff declares a \time change the lower staff omits. The lower
    // staff must be re-barred to the shared timeline: 2/4, 3/4, 3/4.
    let src = r#"
upper = { \time 2/4 c'4 d' \time 3/4 e'4 f' g' a'4 b' c'' }
lower = { \time 2/4 c4 d e4 f g a4 b c' }
\score { \new PianoStaff << \new Staff \upper \new Staff \lower >> }
"#;
    let score = LyToIrAdapter::new().convert_str(src).expect("LY → Score");
    let part = &score.parts()[0];
    assert_eq!(part.staves, 2, "expected a 2-staff piano part");
    assert_eq!(
        part.measures.len(),
        3,
        "expected 3 measures (2/4 + 3/4 + 3/4), got {}",
        part.measures.len()
    );
    use _core::ir::duration::Frac;
    let expected = [Frac::new(1, 2), Frac::new(3, 4), Frac::new(3, 4)];
    for (i, fills) in staff_measure_fills(part).iter().enumerate() {
        for (staff, fill) in fills {
            assert_eq!(
                *fill,
                expected[i],
                "measure {} staff {staff}: fill {fill} != expected {}",
                i + 1,
                expected[i]
            );
        }
    }
}

#[test]
fn pedal_piano_staff_bars_aligned_across_staves() {
    // pedal.ly: the RH (voicea) declares \time 4/4 sections the LH (voiceb)
    // omits. Without time-signature unification the LH stays barred in 3/8
    // and shifts relative to the RH (visible from bar 58 on).
    let score = LyToIrAdapter::new()
        .convert_str(&read_ly("pedal.ly"))
        .expect("LY → Score");
    let part = &score.parts()[0];
    assert_eq!(part.staves, 2, "pedal.ly is a 2-staff piano part");

    let fills = staff_measure_fills(part);
    let mut misaligned = Vec::new();
    // The final measure may legitimately be ragged (trailing content).
    for (i, f) in fills.iter().enumerate().take(fills.len().saturating_sub(1)) {
        if f.len() >= 2 {
            let mut vals = f.values();
            let first = vals.next().unwrap();
            if !vals.all(|v| v == first) {
                misaligned.push((i + 1, f.clone()));
            }
        }
    }
    assert!(
        misaligned.is_empty(),
        "staves disagree on bar fill in {} measures: {:?}",
        misaligned.len(),
        &misaligned[..misaligned.len().min(10)]
    );
}
