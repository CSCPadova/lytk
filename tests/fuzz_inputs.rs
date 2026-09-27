//! Robustness fuzzing: every parser entry point must DEGRADE GRACEFULLY on
//! arbitrary / malformed input — return `Err` (or `Ok`), never panic, hang, or
//! OOM. lytk ingests untrusted files (`.ly`, `.xml`/`.mxl`, `.mid`, `.abc`), so
//! this is a hard requirement for a public release. proptest catches any panic
//! as a test failure; the Phase-2 robustness fixes (recursion cap, input
//! clamping, bounded unzip, panic firewalls) are what keep these green.

use proptest::prelude::*;

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::humdrum_to_ir::HumdrumToIrAdapter;
use _core::adapters::ir_to_abc::IrToAbcAdapter;
use _core::adapters::ir_to_humdrum::IrToHumdrumAdapter;
use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::{check, LyToIrAdapter};
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
use _core::ir::language::PitchLanguage;
use _core::representations;
use _core::transforms;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Arbitrary text must not crash the LilyPond parser.
    #[test]
    fn ly_parser_survives_arbitrary_text(s in "\\PC{0,2000}") {
        let _ = LyToIrAdapter::new().convert_str(&s);
    }

    /// Arbitrary text must not crash the ABC parser.
    #[test]
    fn abc_parser_survives_arbitrary_text(s in "\\PC{0,2000}") {
        let _ = AbcToIrAdapter::new().convert_str(&s);
    }

    /// Arbitrary text must not crash the Humdrum parser.
    #[test]
    fn humdrum_parser_survives_arbitrary_text(s in "\\PC{0,2000}") {
        let _ = HumdrumToIrAdapter::new().convert_str(&s);
    }

    /// kern-shaped fuzz: a **kern header with arbitrary token soup.
    #[test]
    fn humdrum_parser_survives_kern_shaped_text(s in "\\PC{0,1000}") {
        let _ = HumdrumToIrAdapter::new().convert_str(&format!("**kern\n{s}\n*-\n"));
    }

    /// Arbitrary bytes must not crash the MusicXML/MXL reader (covers the zip
    /// path too — random bytes starting with the PK magic hit the unzip code).
    #[test]
    fn xml_parser_survives_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        let s = String::from_utf8_lossy(&bytes);
        let _ = MxmlToIrAdapter::new().convert_str(&s);
    }

    /// Arbitrary bytes must not crash the MIDI reader.
    #[test]
    fn midi_parser_survives_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        let _ = MidiToIrAdapter::new().convert_bytes(&bytes);
    }

    /// Bytes carrying the ZIP magic exercise the bounded unzip path specifically.
    #[test]
    fn xml_parser_survives_ziplike_bytes(mut bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        bytes.splice(0..0, *b"PK\x03\x04");
        let s = String::from_utf8_lossy(&bytes);
        let _ = MxmlToIrAdapter::new().convert_str(&s);
    }
}

// --- LilyPond-shaped fuzz -----------------------------------------------------
//
// Arbitrary text almost never reaches the reader's number handling: `\time 0/0`
// survived 128 random-text cases per run for months. These properties build
// LilyPond out of real constructs whose numbers are edge values.

/// Numbers at the edges of every integer type the reader casts to.
fn edge_number() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => (0u64..=16).prop_map(|n| n.to_string()),
        1 => Just("0".to_string()),
        1 => Just("255".to_string()),
        1 => Just("256".to_string()),
        1 => Just("65535".to_string()),
        1 => Just("65536".to_string()),
        1 => Just("2147483648".to_string()),
        1 => Just("4294967296".to_string()),
        1 => Just("18446744073709551615".to_string()),
        1 => Just("99999999999999999999999999".to_string()),
    ]
}

/// One LilyPond construct with edge numbers in its holes.
fn construct() -> BoxedStrategy<String> {
    let n = edge_number;
    prop_oneof![
        (n(), n()).prop_map(|(a, b)| format!("\\time {a}/{b} c4 d e f")),
        (n(), n(), n()).prop_map(|(a, b, c)| format!("\\time {a}+{b}/{c} c8 d e")),
        (n(), n()).prop_map(|(a, b)| format!("\\tuplet {a}/{b} {{ c8 d e }}")),
        (n(), n()).prop_map(|(a, b)| format!("\\times {a}/{b} {{ c8 d e }}")),
        (n(), n(), n(), n()).prop_map(|(a, b, c, d)| {
            format!("\\tuplet {a}/{b} {{ c8 \\tuplet {c}/{d} {{ d16 e f }} }}")
        }),
        n().prop_map(|a| format!("\\partial {a} c4 d e f")),
        (n(), n()).prop_map(|(a, b)| format!("\\partial {a}*{b} c4")),
        n().prop_map(|a| format!("c{a} d4")),
        (n(), 0usize..300).prop_map(|(a, k)| format!("c{a}{} d4", ".".repeat(k))),
        (n(), n()).prop_map(|(a, b)| format!("c4*{a} d{b}")),
        (n(), n(), n()).prop_map(|(a, b, c)| format!("c{a}*{b}/{c} d4")),
        (n(), n()).prop_map(|(a, b)| format!("r{a}*{b} R1*{b} c4")),
        (n(), n()).prop_map(|(a, b)| format!("s{a}*{b} c4")),
        (n(), n()).prop_map(|(a, b)| format!("\\skip {a}*{b} c4")),
        (n(), n()).prop_map(|(a, b)| {
            format!("\\set Score.measureLength = #(ly:make-moment {a} {b}) c4 d e f")
        }),
        (n(), n()).prop_map(|(a, b)| {
            format!("\\set Timing.baseMoment = #(ly:make-moment {a}/{b}) c4")
        }),
        n().prop_map(|a| format!("\\repeat unfold {a} {{ c4 d }}")),
        (n(), n()).prop_map(|(a, b)| {
            format!("\\repeat unfold {a} {{ \\repeat unfold {b} {{ c16 }} }}")
        }),
        n().prop_map(|a| {
            format!("\\repeat volta {a} {{ c4 }} \\alternative {{ {{ d4 }} {{ e4 }} }}")
        }),
        (n(), n()).prop_map(|(a, b)| format!("\\scaleDurations {a}/{b} {{ c4 d }}")),
        n().prop_map(|a| format!("\\grace {{ c{a} d }} e4")),
        n().prop_map(|a| format!("\\ottava #{a} c4")),
        (n(), n()).prop_map(|(a, b)| format!("\\tempo {a} = {b} c4")),
        (n(), n()).prop_map(|(a, b)| format!("\\tempo 4 = {a}-{b} c4")),
        n().prop_map(|a| format!("\\clef \"treble_{a}\" c4")),
        n().prop_map(|a| format!("\\mark {a} c4")),
        n().prop_map(|a| format!("<c e g>{a} d4")),
        (n(), n()).prop_map(|(a, b)| format!("\\compoundMeter #'(({a} {b} 8)) c8 d e")),
        (n(), n(), 0usize..300)
            .prop_map(|(a, b, k)| { format!("\\figuremode {{ <{a}>{b}{} }}", ".".repeat(k)) }),
        (n(), n()).prop_map(|(a, b)| format!("\\figuremode {{ <6>{a}*{b} }}")),
        (n(), n()).prop_map(|(a, b)| format!("\\chordmode {{ c{a}:{b} }}")),
        (n(), n()).prop_map(|(a, b)| format!("\\drummode {{ bd{a}*{b} sn }}")),
        n().prop_map(|a| format!("c{} d4", "'".repeat(a.len().min(40) * 8))),
    ]
    .boxed()
}

/// A score built from a few constructs, in a staff or as bare music.
fn lilypond_program() -> impl Strategy<Value = String> {
    (proptest::collection::vec(construct(), 1..4), any::<bool>()).prop_map(|(parts, staff)| {
        let music = parts.join(" ");
        if staff {
            format!("\\score {{ \\new Staff {{ {music} }} \\layout {{ }} }}")
        } else {
            format!("{{ {music} }}")
        }
    })
}

/// Cases per LilyPond-shaped property: few in `cargo test` (inputs at the
/// reader's bounds take seconds in debug builds), many in CI's fuzz job
/// (`LYTK_FUZZ_CASES=1024`).
fn fuzz_cases(default: u32) -> u32 {
    std::env::var("LYTK_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(fuzz_cases(32)))]

    /// LilyPond-shaped input must not crash any reader entry point.
    #[test]
    fn ly_reader_survives_lilypond_shaped_input(src in lilypond_program()) {
        let reader = LyToIrAdapter::new();
        let _ = reader.convert_str(&src);
        let _ = reader.convert_str_multi(&src);
        let _ = reader.convert_str_to_music(&src);
        let _ = LyToIrAdapter::new()
            .with_language(PitchLanguage::English)
            .convert_str(&src);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(fuzz_cases(16)))]

    /// Whatever the reader makes of LilyPond-shaped input, every writer,
    /// transform and representation must handle it. (A piano roll is only
    /// built for short music, at a coarse resolution: it is dense by nature,
    /// and this property is about panics, not allocation.)
    #[test]
    fn downstream_survives_lilypond_shaped_input(src in lilypond_program()) {
        let Ok(score) = LyToIrAdapter::new().convert_str(&src) else { return Ok(()) };
        // Long music (up to the reader's 100,000 bars) makes big documents, not
        // new panics: keep the property's memory modest.
        if score.parts().iter().map(|p| p.measures.len()).sum::<usize>() > 5_000 {
            return Ok(());
        }
        let doc = _core::ir::lift::lift_to_music(&score);
        let _ = IrToLyAdapter::new().convert(&score);
        let _ = IrToLyAdapter::new().convert_music(&doc);
        let _ = IrToMxmlAdapter::new().convert(&score);
        let _ = IrToMidiAdapter::new().convert_bytes(&score);
        let _ = IrToAbcAdapter::new().convert_music(&doc);
        let _ = IrToHumdrumAdapter::new().convert(&score);
        let _ = transforms::transpose::transpose(&score, 7);
        let _ = transforms::retrograde::retrograde(&score);
        let _ = _core::ir::lower::lower_to_score(&doc);
        let arr = representations::to_note_array(&doc, 4);
        if arr.length() <= 100_000 {
            let _ = representations::to_piano_roll(&arr, true);
        }
    }
}

// --- Targeted regression inputs the audit reproduced as crashes/hangs --------

#[test]
fn deeply_nested_lilypond_is_rejected_not_overflowed() {
    let deep = format!("{}{}", "{ ".repeat(9000), " }".repeat(9000));
    // Must return (Err) without overflowing the stack / aborting the process.
    let _ = LyToIrAdapter::new().convert_str(&deep);
}

#[test]
fn abc_zero_denominator_meter_does_not_panic() {
    let _ = AbcToIrAdapter::new().convert_str("X:1\nM:4/0\nK:C\nCDEF\n");
}

#[test]
fn midi_zero_division_header_does_not_hang() {
    // Format-0 SMF declaring 0 ticks-per-quarter + one empty track.
    let bytes: &[u8] = &[
        b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 0, // division = 0
        b'M', b'T', b'r', b'k', 0, 0, 0, 4, 0, 0xFF, 0x2F, 0x00,
    ];
    let _ = MidiToIrAdapter::new().convert_bytes(bytes);
}

#[test]
fn empty_and_garbage_inputs_are_clean_errors() {
    let ly = LyToIrAdapter::new();
    let abc = AbcToIrAdapter::new();
    let xml = MxmlToIrAdapter::new();
    let midi = MidiToIrAdapter::new();
    for input in ["", "\0\0\0\0", "not music at all", "<<<<<<", "}}}}"] {
        let _ = ly.convert_str(input);
        let _ = abc.convert_str(input);
        let _ = xml.convert_str(input);
        let _ = midi.convert_bytes(input.as_bytes());
    }
}

/// LilyPond inputs that panicked before 0.3.0 (Epic J1), each through every
/// reader entry point. Each construct is dropped (a time signature, a tuplet
/// ratio, a measure length) and the music around it is still read.
const LY_PANICKED: &[&str] = &[
    // `\time N/0`: a zero denominator reached `TimeSignature::beats_fraction`
    r"{ \time 3/0 c4 d e f }",
    // `\time 3/256`: the denominator was truncated to a `u8` 0
    r"{ \time 3/256 c4 d e f }",
    r"{ \time 3+2/512 c8 d e f g }",
    // `\tuplet 0/N` and `\times N/0`: a zero ratio term reached `actual_duration`
    r"{ \tuplet 0/2 { c8 d e } }",
    r"{ \times 2/0 { c8 d e } }",
    r"{ \tuplet 3/2 { c8 \tuplet 0/1 { d16 e } f8 } }",
    // `ly:make-moment N 0`: a zero denominator reached `Frac::new`
    r"{ \set Score.measureLength = #(ly:make-moment 3 0) c4 d e f }",
    r"{ \set Score.measureLength = #(ly:make-moment 3/0) c4 d e f }",
];

fn read_everywhere(src: &str) -> _core::ir::score::Score {
    let reader = LyToIrAdapter::new();
    let _ = reader.convert_str_multi(src).expect("reads");
    let _ = reader.convert_str_to_music(src).expect("reads");
    reader.convert_str(src).expect("reads")
}

#[test]
fn lilypond_inputs_that_panicked_are_read() {
    for src in LY_PANICKED {
        let score = read_everywhere(src);
        let json = serde_json::to_string(&score).unwrap();
        assert!(!json.contains("\"beat_type\":0"), "{src}: {json}");
        assert!(!json.contains("\"tuplet_actual\":0"), "{src}");
        assert!(!json.contains("\"tuplet_normal\":0"), "{src}");
    }
}

#[test]
fn tuplet_with_a_zero_term_is_read_unscaled() {
    let json = |s| serde_json::to_string(&read_everywhere(s)).unwrap();
    assert_eq!(json(r"{ \tuplet 0/2 { c8 d e } }"), json(r"{ c8 d e }"));
    assert_eq!(json(r"{ \times 2/0 { c8 d e } }"), json(r"{ c8 d e }"));
}

#[test]
fn figured_bass_with_hundreds_of_dots_does_not_overflow() {
    // The dot count was a `u8` incremented without saturation.
    let src = format!(r"figs = \figuremode {{ <6>4{} }} {{ c4 }}", ".".repeat(300));
    let _ = read_everywhere(&src);
    let src = format!(r"\figures {{ <6>4{} }}", ".".repeat(300));
    let _ = read_everywhere(&src);
}

// --- Inputs the 0.3.0 panic hunt reproduced -------------------------------------
//
// Each was a panic, a hang, a stack overflow or an out-of-memory abort in lytk
// 0.2.0. Readings too large for the reader's bounds are refused (`Err`); the
// rest are read.

fn refused(src: &str) -> bool {
    LyToIrAdapter::new().convert_str(src).is_err()
}

fn note_count(score: &_core::ir::score::Score) -> usize {
    use _core::ir::note::VoiceElement;
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .map(|e| match e {
            VoiceElement::Note(_) => 1,
            VoiceElement::Chord(c) => c.notes.len(),
            VoiceElement::Rest(_) => 0,
        })
        .sum()
}

#[test]
fn oversized_music_is_refused_quickly() {
    let start = std::time::Instant::now();
    for src in [
        r"{ c'1*1000000000 }",
        r"{ c'4*9223372036854775807 c'4 }",
        r"{ s1*1000000000 c'4 }",
        r"{ R1*1000000000 }",
        r"{ \skip 1*1000000000 c'4 }",
        r"{ \set Score.measureLength = #(ly:make-moment 1 4294967295) c'1 c'1 }",
        r"{ \repeat unfold 255 { \repeat unfold 255 { \repeat unfold 255 { c'4 } } } }",
        r"{ \repeat unfold 99999999999999 { } }",
    ] {
        assert!(refused(src), "{src}");
    }
    // Twenty-two definitions, each doubling the one before, never used.
    let mut src = String::from("varA = { c'4 }\n");
    let names: Vec<String> = (0..22)
        .map(|i| format!("var{}", (b'A' + i as u8) as char))
        .collect();
    for w in names.windows(2) {
        src.push_str(&format!("{} = {{ \\{} \\{} }}\n", w[1], w[0], w[0]));
    }
    assert!(refused(&src), "doubling variables");
    assert!(start.elapsed().as_secs() < 30, "took {:?}", start.elapsed());
}

#[test]
fn coprime_durations_are_refused_not_overflowed() {
    assert!(refused(
        r"{ c'4*1/251 c'4*1/241 c'4*1/239 c'4*1/233 c'4*1/229 c'4*1/227 c'4*1/223 c'4*1/211 }"
    ));
    let tuplets: String = [3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53]
        .iter()
        .map(|p| format!(r"\tuplet {p}/1 {{ c'4 }} "))
        .collect();
    assert!(refused(&format!("{{ {tuplets} }}")));
}

#[test]
fn a_self_referencing_variable_inside_relative_is_read_once() {
    // `a = { \a \a \a }` refers to the previous `a`: three notes, not 3^16.
    let score = LyToIrAdapter::new()
        .convert_str("a = { c4 }\na = { \\a \\a \\a }\n\\relative c' { \\a }")
        .expect("reads");
    assert_eq!(note_count(&score), 3);
}

#[test]
fn deep_nesting_below_the_bound_does_not_overflow_the_stack() {
    for (open, n) in [
        (r"\tuplet 3/2 { ", 400),
        (r"\relative c' { ", 400),
        (r"\repeat volta 2 { ", 400),
        (r"\times 2/3 { ", 400),
    ] {
        let src = format!("{{ {}c'4 {} }}", open.repeat(n), "} ".repeat(n));
        let _ = LyToIrAdapter::new().convert_str(&src);
    }
    // Variables read again inside \relative multiply the walk's depth.
    let src = format!(
        "a = {{ c'4 }}\na = {{ {}\\a {}}}\n\\relative c' {{ \\a }}",
        r"\relative c' { ".repeat(200),
        "} ".repeat(200)
    );
    let _ = LyToIrAdapter::new().convert_str(&src);
}

#[test]
fn staff_groups_beyond_u8_are_refused_and_wide_scores_lower() {
    let group = |n: usize| format!(r"\new PianoStaff << {}>>", r"\new Staff { c'4 } ".repeat(n));
    assert!(refused(&group(256)));
    assert!(!refused(&group(255)));
    let wide = format!("<< {}>>", r"\new Staff { c'4 } ".repeat(256));
    let doc = LyToIrAdapter::new()
        .convert_str_to_music(&wide)
        .expect("reads");
    let _ = _core::ir::lower::lower_to_score(&doc);
}

#[test]
fn a_climbing_relative_passage_is_refused() {
    let src = format!(r"\relative c {{ c{} c c c }}", "'".repeat(10_000));
    assert!(refused(&src));
}

#[test]
fn writers_handle_zero_length_notes_and_coprime_tuplets() {
    let score = LyToIrAdapter::new()
        .convert_str(r"{ c'4*0 d'4 }")
        .expect("reads");
    assert!(IrToLyAdapter::new().convert(&score).unwrap().contains("*0"));
    let voices: Vec<String> = [251, 241, 239, 233, 229, 227, 223, 211]
        .iter()
        .map(|p| format!(r"{{ \tuplet {p}/1 {{ c'4 }} }}"))
        .collect();
    let score = LyToIrAdapter::new()
        .convert_str(&format!(r"<< {} >>", voices.join(r" \\ ")))
        .expect("reads");
    let _ = IrToMxmlAdapter::new()
        .convert(&score)
        .expect("writes MusicXML");
}

#[test]
fn oversized_tremolos_are_written_capped() {
    // `:2147483648` gave 31 tremolo marks, which the LilyPond writer shifted
    // past 32 bits (found by `downstream_survives_lilypond_shaped_input`).
    let score = read_everywhere(r"{ \chordmode { c1:2147483648 } }");
    IrToLyAdapter::new().convert(&score).expect("writes");
    // A hand-made IR may carry any count.
    let mut score = read_everywhere(r"{ c'1:32 d'1 }");
    for part in score.parts_mut() {
        for m in &mut part.measures {
            for v in &mut m.voices {
                for e in &mut v.elements {
                    if let _core::ir::note::VoiceElement::Note(n) = e {
                        n.tremolo_marks = u8::MAX;
                    }
                }
            }
        }
    }
    let doc = _core::ir::lift::lift_to_music(&score);
    IrToLyAdapter::new().convert(&score).expect("writes");
    IrToLyAdapter::new().convert_music(&doc).expect("writes");
    let _ = IrToMxmlAdapter::new().convert(&score);
    // …and as a two-note tremolo.
    for part in score.parts_mut() {
        for m in &mut part.measures {
            for v in &mut m.voices {
                for e in &mut v.elements {
                    if let _core::ir::note::VoiceElement::Note(n) = e {
                        n.two_note_tremolo = true;
                    }
                }
            }
        }
    }
    IrToLyAdapter::new().convert(&score).expect("writes");
}

#[test]
fn humdrum_dots_saturate() {
    // 300 dots overflowed the dot counter (found by J1's hunt).
    let kern = format!("**kern\n4{}c\n*-\n", ".".repeat(300));
    let score = HumdrumToIrAdapter::new().convert_str(&kern).unwrap();
    assert_eq!(note_count(&score), 1);
}

#[test]
fn music_longer_than_the_bound_is_refused_by_every_reader() {
    // A single note of millions of whole notes was tied over every bar line
    // downstream: the ABC writer aborted on a 1.6 GB allocation.
    let long = "the music is longer than 100000 whole notes";
    let abc = AbcToIrAdapter::new().convert_str("X:1\nL:1/1\nK:C\nc1000000\n");
    assert!(abc.unwrap_err().to_string().contains(long));
    let xml = "<?xml version=\"1.0\"?><score-partwise version=\"4.0\"><part-list>\
               <score-part id=\"P1\"><part-name>P</part-name></score-part></part-list>\
               <part id=\"P1\"><measure number=\"1\"><attributes><divisions>1</divisions>\
               </attributes><note><pitch><step>C</step><octave>4</octave></pitch>\
               <duration>400000000</duration></note></measure></part></score-partwise>";
    let read = MxmlToIrAdapter::new().convert_str(xml);
    assert!(read.unwrap_err().to_string().contains(long));
    let kern = format!("**kern\n{}*-\n", "0c\n".repeat(60_000));
    let read = HumdrumToIrAdapter::new().convert_str(&kern);
    assert!(read.unwrap_err().to_string().contains(long));
}

#[test]
fn the_abc_writer_refuses_hand_made_music_of_millions_of_bars() {
    use _core::ir::duration::{Duration, Frac};
    use _core::ir::music::{Music, MusicDocument};
    use _core::ir::pitch::{Pitch, PitchStep};
    let note = Music::Note {
        pitch: Pitch::new(PitchStep::C, 4),
        duration: Duration::new(Frac::from_integer(1 << 30)),
        annotations: Vec::new(),
    };
    let doc = MusicDocument::new(Music::Sequential(vec![note]));
    let start = std::time::Instant::now();
    assert!(IrToAbcAdapter::new().convert_music(&doc).is_err());
    assert!(start.elapsed().as_secs() < 30, "took {:?}", start.elapsed());
}

/// Seconds `f` takes on `n` and on `4 * n` repetitions.
fn times(n: usize, f: impl Fn(usize)) -> (f64, f64) {
    let time = |k: usize| {
        let start = std::time::Instant::now();
        f(k);
        start.elapsed().as_secs_f64()
    };
    (time(n), time(4 * n))
}

#[test]
fn repeated_constructs_scale_linearly() {
    // Found by J1's hunt as quadratic: meter changes (`Grid::build` scanned
    // every meter at each), runs of tuplets in the ABC writer (each group
    // counted the run to its end), runs of grace notes in the Music-path
    // LilyPond writer (each copied the group so far), diagnostics on one long
    // line (each counted its column from the line's start). Four times the
    // input must take about four times as long, not sixteen.
    let meters = |n: usize| {
        let src = format!(
            "{{ {} }}",
            r"\time 3/8 c'8 d' e' \time 2/4 c'4 d' ".repeat(n)
        );
        let score = LyToIrAdapter::new().convert_str(&src).unwrap();
        let _ = _core::ir::lower::lower_to_score(&_core::ir::lift::lift_to_music(&score));
    };
    let tuplets = |n: usize| {
        let src = format!("{{ {} }}", r"\tuplet 3/2 { c'8 d' e' } ".repeat(n));
        let score = LyToIrAdapter::new().convert_str(&src).unwrap();
        let doc = _core::ir::lift::lift_to_music(&score);
        IrToAbcAdapter::new().convert_music(&doc).unwrap();
    };
    let graces = |n: usize| {
        let grace = "<note><grace/><pitch><step>C</step><octave>4</octave></pitch><type>eighth</type></note>";
        let xml = format!(
            "<?xml version=\"1.0\"?><score-partwise version=\"4.0\"><part-list>\
             <score-part id=\"P1\"><part-name>P</part-name></score-part></part-list>\
             <part id=\"P1\"><measure number=\"1\"><attributes><divisions>1</divisions>\
             </attributes>{}<note><pitch><step>C</step><octave>4</octave></pitch>\
             <duration>1</duration></note></measure></part></score-partwise>",
            grace.repeat(n)
        );
        let score = MxmlToIrAdapter::new().convert_str(&xml).unwrap();
        let doc = _core::ir::lift::lift_to_music(&score);
        IrToLyAdapter::new().convert_music(&doc).unwrap();
    };
    let diagnostics = |n: usize| {
        let src = format!("{{ {} }}", r"c'3 \foo ".repeat(n));
        assert_eq!(check(&src, true).len(), 2 * n);
    };
    for (name, (small, big)) in [
        ("diagnostics on one line", times(2000, diagnostics)),
        ("meter changes", times(1000, meters)),
        ("tuplet runs", times(2000, tuplets)),
        ("grace runs", times(2000, graces)),
    ] {
        assert!(
            big < 8.0 * small + 0.05,
            "{name}: {small:.3} s, then {big:.3} s"
        );
    }
}
