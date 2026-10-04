//! ABC against the standard, not against lytk itself.
//!
//! lytk's ABC round trips compare its reader with its writer, so a mistake
//! the two share cancels out. Here an independent ABC 2.1 player
//! (`common::abc_oracle`) is the judge:
//!
//! - **writer board**: every MusicXML fixture and the LilyPond pieces are
//!   written as ABC and played by the oracle under both accidental rules
//!   (`octave`, what abcm2ps and abc2svg do, and `pitch`, the ABC 2.1 text's
//!   default). The notes must be the source's notes under both.
//! - **reader board**: the ABC fixtures read by lytk must sound like the
//!   oracle's reading.
//!
//! Counts are gated on committed baselines that may only rise.
//! Run with output: `cargo test --test abc_standard -- --nocapture`

mod common;

use common::abc_oracle::{play, OracleNote, Propagation};
use common::{common_count, note_signature, safe, steps};

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::ir_to_abc::IrToAbcAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromMusicAdapter, ToIrAdapter};
use _core::ir::duration::Frac;
use _core::ir::score::Score;

use std::path::{Path, PathBuf};

// Committed baselines (may only rise, except the misread-note count, which
// may only fall).
const WRITER_PITCHES: usize = 154;
const WRITER_NOTES: usize = 154;
const WRITER_MAX_MISREAD: usize = 57;
const READER_NOTES: usize = 4;

fn sig(notes: &[OracleNote]) -> Vec<(u32, u32, i32)> {
    let mut v: Vec<_> = notes
        .iter()
        .map(|n| (steps(n.onset), steps(n.duration), n.pitch))
        .collect();
    v.sort_unstable();
    v
}

fn pitches(notes: &[OracleNote]) -> Vec<i32> {
    notes.iter().map(|n| n.pitch).collect()
}

fn q(n: i64, d: i64) -> Frac {
    Frac::new(n, d)
}

// ---------------------------------------------------------------------------
// The oracle itself, checked by hand against ABC 2.1
// ---------------------------------------------------------------------------

fn octave(abc: &str) -> Vec<OracleNote> {
    play(abc, Propagation::Octave).unwrap_or_else(|e| panic!("{e}\n{abc}"))
}

#[test]
fn oracle_applies_the_key_signature() {
    assert_eq!(pitches(&octave("K:G\nF f F,|")), [66, 78, 54]);
    assert_eq!(pitches(&octave("K:F\nB b|")), [70, 82]);
    assert_eq!(pitches(&octave("K:Dmix\nc C F|")), [72, 60, 66]);
    assert_eq!(pitches(&octave("K:Am\nF G c|")), [65, 67, 72]);
    assert_eq!(pitches(&octave("K:G#m\nF C D|")), [66, 61, 63]);
}

#[test]
fn oracle_reads_explicit_key_accidentals() {
    // D Phrygian (B♭, E♭) plus F♯.
    assert_eq!(pitches(&octave("K:D Phr ^f\nF B E|")), [66, 70, 63]);
    // `exp`: only the listed accidentals.
    assert_eq!(pitches(&octave("K:D exp _b\nF B|")), [65, 70]);
    // A clef alone keeps the key.
    assert_eq!(pitches(&octave("K:D\nF|[K:clef=bass] F|")), [66, 66]);
}

#[test]
fn oracle_carries_accidentals_to_the_end_of_the_bar() {
    let abc = "K:C\n^F F f | F|";
    assert_eq!(pitches(&octave(abc)), [66, 66, 77, 65]);
    let by_pitch = play(abc, Propagation::Pitch).unwrap();
    assert_eq!(pitches(&by_pitch), [66, 66, 78, 65]);
    assert_eq!(pitches(&octave("K:D\n=F F | F|")), [65, 65, 66]);
    // The directive overrides the caller's rule.
    let directive = play(
        "%%propagate-accidentals pitch\nK:C\n^F f|",
        Propagation::Octave,
    )
    .unwrap();
    assert_eq!(pitches(&directive), [66, 78]);
}

#[test]
fn oracle_reads_lengths_and_rhythm() {
    let n = octave("L:1/8\nK:C\nA2 A/2 A/ A// A3/2|");
    let d: Vec<_> = n.iter().map(|x| x.duration).collect();
    assert_eq!(d, [q(1, 4), q(1, 16), q(1, 16), q(1, 32), q(3, 16)]);
    // Default unit: 1/16 below 3/4, 1/8 from 3/4 up.
    assert_eq!(octave("M:2/4\nK:C\nA|")[0].duration, q(1, 16));
    assert_eq!(octave("M:3/4\nK:C\nA|")[0].duration, q(1, 8));
    let broken: Vec<_> = octave("L:1/8\nK:C\nA>B A<B|")
        .iter()
        .map(|x| x.duration)
        .collect();
    assert_eq!(broken, [q(3, 16), q(1, 16), q(1, 16), q(3, 16)]);
    let trip = octave("L:1/8\nK:C\n(3ABc d|");
    assert_eq!(trip[1].onset, q(1, 12));
    assert_eq!(trip[3].onset, q(1, 4));
    // In compound meters `(5` puts five notes into the time of three.
    let quint = octave("M:6/8\nL:1/8\nK:C\n(5ABcde f|");
    assert_eq!(quint[5].onset, q(3, 8));
    let chords = octave("L:1/8\nK:C\n[CEG]2 [C2E2G2]|");
    assert!(chords.iter().all(|x| x.duration == q(1, 4)));
    assert_eq!(chords[3].onset, q(1, 4));
    // A multi-bar rest.
    assert_eq!(octave("M:4/4\nL:1/8\nK:C\nZ2|A|")[0].onset, q(2, 1));
}

#[test]
fn oracle_merges_ties_and_unfolds_repeats() {
    let tied = octave("L:1/8\nK:C\nA2-A2 A|");
    assert_eq!(tied.len(), 2);
    assert_eq!((tied[0].duration, tied[1].onset), (q(1, 2), q(1, 2)));
    let rep = octave("L:1/4\nK:C\n|:A B:|c|");
    assert_eq!(pitches(&rep), [69, 71, 69, 71, 72]);
    let endings = octave("L:1/4\nK:C\n|:A|1B:|2c|]");
    assert_eq!(pitches(&endings), [69, 71, 69, 72]);
    let bracket = octave("L:1/4\nK:C\n|:A|[1B:|[2c|]");
    assert_eq!(pitches(&bracket), [69, 71, 69, 72]);
}

#[test]
fn oracle_reads_voices_graces_and_only_the_first_tune() {
    let v = octave("X:1\nV:1\nV:2\nK:C\n[V:1] A|\n[V:2] C|");
    assert_eq!(v.len(), 2);
    assert!(v.iter().all(|n| n.onset == q(0, 1)));
    let g = octave("L:1/8\nK:C\n{g}A|");
    assert_eq!((g[0].pitch, g[0].onset, g[1].onset), (79, q(0, 1), q(0, 1)));
    assert_eq!(pitches(&octave("X:1\nK:C\nA|\nX:2\nK:C\nB|")), [69]);
    assert_eq!(pitches(&octave("K:C\nA|\nw: la\nB|")), [69, 71]);
    // `&` overlays a bar: both layers start with it, and the next bar starts
    // after the longer one (ABC 2.1 §7.4).
    let o = octave("L:1/4\nK:C\nC D E F & G2|A|");
    let at = |p: i32| o.iter().find(|n| n.pitch == p).map(|n| n.onset);
    assert_eq!(
        (at(60), at(67), at(69)),
        (Some(q(0, 1)), Some(q(0, 1)), Some(q(1, 1)))
    );
    // A finished repeat doesn't take a later section's ending count.
    let order = pitches(&octave("X:1\nM:2/4\nL:1/4\nK:C\n|:A2:|B2|1c2:|2d2:|3e2|]"));
    assert_eq!(order, [69, 69, 71, 72, 71, 74, 71, 76]);
}

// ---------------------------------------------------------------------------
// The boards
// ---------------------------------------------------------------------------

fn list(dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.retain(|p| p.extension().is_some_and(|e| e == ext));
    v.sort();
    v
}

/// The scores the writer board writes as ABC: every MusicXML fixture and the
/// LilyPond pieces.
fn sources() -> Vec<(String, Score)> {
    let mut out = Vec::new();
    for p in list("tests/fixtures/xml", "xml")
        .into_iter()
        .chain(list("tests/fixtures/mxl", "mxl"))
    {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if let Some(s) = safe(|| MxmlToIrAdapter::new().convert_file(&p).ok()) {
            out.push((name, s));
        }
    }
    for ly in ["chopin_n", "example", "example2", "pedal", "repeats"] {
        let path = Path::new("tests/fixtures/ly").join(format!("{ly}.ly"));
        let scores =
            safe(|| LyToIrAdapter::new().convert_file_multi(&path).ok()).unwrap_or_default();
        for (i, s) in scores.into_iter().enumerate() {
            out.push((format!("{ly}.ly#{i}"), s));
        }
    }
    out
}

#[test]
fn abc_standard_board() {
    // ----- Writer: source → ABC → the oracle, under both rules -----
    let (mut total, mut pitch_ok, mut notes_ok, mut misread, mut src_notes) = (0, 0, 0, 0, 0);
    let (mut write_failed, mut oracle_failed) = (Vec::new(), Vec::new());
    let mut wrong = Vec::new();
    for (name, score) in sources() {
        total += 1;
        let doc = _core::ir::lift::lift_to_music(&score);
        let Some(abc) = safe(|| IrToAbcAdapter::new().convert_music(&doc).ok()) else {
            write_failed.push(name);
            continue;
        };
        let (a, b) = (
            play(&abc, Propagation::Octave),
            play(&abc, Propagation::Pitch),
        );
        let (a, b) = match (a, b) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                oracle_failed.push(format!("{name}: {e}"));
                continue;
            }
        };
        let src = note_signature(&score);
        let (sa, sb) = (sig(&a), sig(&b));
        let mut src_p: Vec<i32> = src.iter().map(|n| n.2).collect();
        let (mut pa, mut pb) = (pitches(&a), pitches(&b));
        src_p.sort_unstable();
        pa.sort_unstable();
        pb.sort_unstable();
        if pa == src_p && pb == src_p {
            pitch_ok += 1;
        }
        if sa == src && sb == src {
            notes_ok += 1;
        } else if wrong.len() < 40 {
            // source notes, ABC notes, and how many match under each rule
            wrong.push(format!(
                "{name} ({} src, {} abc, {}/{} match, pitches {})",
                src.len(),
                sa.len(),
                common_count(&src, &sa),
                common_count(&src, &sb),
                if pa == src_p { "ok" } else { "differ" }
            ));
        }
        src_notes += src.len();
        misread += src.len() - common_count(&src, &sa).min(common_count(&src, &sb));
    }

    // ----- Reader: ABC fixtures read by lytk vs the oracle -----
    let (mut read_total, mut read_ok) = (0, 0);
    let mut read_wrong = Vec::new();
    for p in list("tests/fixtures/abc", "abc") {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&p).expect("fixture");
        read_total += 1;
        let Ok(want) = play(&text, Propagation::Octave) else {
            read_wrong.push(format!("{name} (oracle)"));
            continue;
        };
        let got = safe(|| AbcToIrAdapter::new().convert_str(&text).ok());
        if got.is_some_and(|s| note_signature(&s) == sig(&want)) {
            read_ok += 1;
        } else {
            read_wrong.push(name);
        }
    }

    println!("\n===== ABC against the standard =====");
    println!(
        "writer : {pitch_ok}/{total} right pitches, {notes_ok}/{total} right notes under both rules; \
         {misread}/{src_notes} source notes misread"
    );
    println!("         write failed: {write_failed:?}");
    println!("         oracle refused: {oracle_failed:?}");
    println!("         wrong:");
    for w in &wrong {
        println!("           {w}");
    }
    println!("reader : {read_ok}/{read_total} fixtures read as the standard reads them; wrong: {read_wrong:?}");

    assert!(
        pitch_ok >= WRITER_PITCHES,
        "writer pitches regressed: {pitch_ok} < {WRITER_PITCHES}"
    );
    assert!(
        notes_ok >= WRITER_NOTES,
        "writer notes regressed: {notes_ok} < {WRITER_NOTES}"
    );
    assert!(
        misread <= WRITER_MAX_MISREAD,
        "misread notes rose: {misread} > {WRITER_MAX_MISREAD}"
    );
    assert!(
        read_ok >= READER_NOTES,
        "reader regressed: {read_ok} < {READER_NOTES}"
    );
}

// ---------------------------------------------------------------------------
// The reader against the standard, construct by construct
// ---------------------------------------------------------------------------

/// What lytk's reader makes of `abc`, as (onset, duration, pitch) steps. It
/// goes through the Music tree only, so a lowering bug can't mask a reader one.
fn reader_sig(abc: &str) -> Vec<(u32, u32, i32)> {
    use _core::adapters::ToMusicAdapter;
    let doc = AbcToIrAdapter::new()
        .convert_str_to_music(abc)
        .unwrap_or_else(|e| panic!("{e}\n{abc}"));
    let mut v: Vec<_> = _core::representations::to_note_array(&doc, 480)
        .notes
        .iter()
        .map(|n| (n.onset, n.duration, n.pitch as i32))
        .collect();
    v.sort_unstable();
    v
}

fn assert_reads_as_standard(cases: &[&str]) {
    let mut wrong = Vec::new();
    for abc in cases {
        let want = sig(&octave(abc));
        let got = reader_sig(abc);
        if got != want {
            wrong.push(format!(
                "{abc:?}\n    lytk:     {got:?}\n    standard: {want:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "read differently from ABC 2.1:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn reader_applies_key_signatures() {
    assert_reads_as_standard(&[
        "X:1\nK:G\nF f F,|",
        "X:1\nK:F\nB b|",
        "X:1\nK:D\n[DFA]2 z2|",
        "X:1\nK:Dmix\nc C F|",
        "X:1\nK:Am\nF G c|",
        "X:1\nK:G#m\nF C D|",
        "X:1\nK:Eb\nE A B e|",
        "X:1\nK:D Phr ^f\nF B E|",
        "X:1\nK:D exp _b\nF B|",
        "X:1\nK:Hp\nF C G|",
        "X:1\nK:none\nF B|",
        "X:1\nK:G\nF|[K:F] B F|",
        "X:1\nK:G\nF|\nK:C\nF|",
    ]);
}

#[test]
fn reader_carries_accidentals_through_the_bar() {
    assert_reads_as_standard(&[
        "X:1\nK:C\n^F F f | F|",
        "X:1\nK:D\n=F F | F|",
        "X:1\nK:C\n[^CE] C | C|",
        "X:1\nK:C\n_B ^^C C =C C|",
        "%%propagate-accidentals pitch\nX:1\nK:C\n^F f|",
        "X:1\nK:C\n^F F || F|",
    ]);
}

#[test]
fn reader_keeps_the_key_when_k_sets_only_a_clef() {
    assert_reads_as_standard(&[
        "X:1\nK:D\nF|[K:clef=bass] F|",
        "X:1\nK:D\nF|\nK:bass\nF|",
        "X:1\nK:G clef=bass octave=-1\nG|",
        "X:1\nV:1 clef=bass octave=-1\nK:C\nG|",
    ]);
}

#[test]
fn reader_invents_no_notes() {
    assert_reads_as_standard(&[
        // Lyrics and other field lines are not music.
        "X:1\nK:C\nC D|\nw: six ten-der words\nE F|\nW: aged wax\n",
        "X:1\nK:C\nC D|\ns: !f! *\n+: more\nE|",
        // Only the first tune of the file.
        "X:1\nK:C\nA|\nX:2\nK:C\nB|",
        // Legacy +deco+ and the ABC 2.0 `!` line break.
        "X:1\nK:C\n+fermata+C D|",
        "X:1\nK:C\nCDEF|GABc!cBAG|FEDC|",
        "X:1\nK:C\n!trill!C !f!D|",
    ]);
}

#[test]
fn reader_follows_the_review_cases() {
    assert_reads_as_standard(&[
        // A tie inside a chord ties that note only (the writer emits it).
        "X:1\nL:1/8\nK:C\n[C-E]2 [CG]2|",
        // A chord tie ties every note.
        "X:1\nL:1/8\nK:C\n[CE]2- [CE]2|",
        // A tied note keeps its accidental over the bar line.
        "X:1\nL:1/8\nK:C\n^F2-|F2 F2|",
        "X:1\nL:1/8\nK:C\n[^FA]2-|[FA]2|",
        // A tie on the last note of a tuplet survives.
        "X:1\nL:1/8\nK:C\n(3ABc-c2|",
        // Notes inside a chord keep their lengths; decorations are not pitches.
        "X:1\nL:1/8\nK:C\n[C2E2G2] [!f!CE]|",
        // A -8 clef plays an octave down; octave= stays until changed.
        "X:1\nL:1/4\nK:C clef=treble-8\nc|",
        "X:1\nL:1/4\nK:C octave=-1\nC|[K:G] C|",
        "X:1\nL:1/4\nK:C\nC|[K:bass octave=-1] C|",
        // An empty line ends the tune.
        "X:1\nK:C\nCDEF|\n\nCollected in Cork.\n\nX:2\nK:G\nG|",
        // The I: form of the directive.
        "X:1\nI:propagate-accidentals pitch\nK:C\n^F f|",
    ]);
}

#[test]
fn reader_rejects_absurd_meters() {
    // `M:256/4` used to wrap to 0/4 and hang the lowering.
    let r = AbcToIrAdapter::new().convert_str("X:1\nM:256/4\nK:C\nCDEF|\n");
    assert!(r.is_ok());
    let r = AbcToIrAdapter::new().convert_str("X:1\nM:300/4\nK:C\nCDEF|\n");
    assert!(r.is_ok());
}

#[test]
fn reader_reads_durations() {
    assert_reads_as_standard(&[
        // Broken rhythm, on notes, chords and rests, and doubled.
        "X:1\nL:1/8\nK:C\nA>B C<D A>>B c<<d|",
        "X:1\nL:1/8\nK:C\n[CE]>G z<A B>z|",
        // Whole-bar rests fill their bars; `x` is a silent skip.
        "X:1\nM:4/4\nL:1/4\nK:C\nZ|C D E F|Z2|G4|",
        "X:1\nM:3/4\nL:1/4\nK:C\nX|C D E|",
        "X:1\nL:1/4\nK:C\nC x D x2 E|",
        // Voices switch mid-line and keep their own L: and M:.
        "X:1\nL:1/4\nV:1\nV:2\nK:C\n[V:1] C D|[V:2] E F|",
        "X:1\nL:1/4\nK:C\nV:1\nL:1/8\nC D|\nV:2\nE F|\nV:1\nG A|",
        "X:1\nM:4/4\nL:1/4\nK:C\nV:1\nM:3/4\nZ|C|\nV:2\nZ|C|",
        // In compound meter (5 is 5 in the time of 3; in simple, of 2.
        "X:1\nM:6/8\nL:1/8\nK:C\n(5CDEFG A|",
        "X:1\nM:4/4\nL:1/8\nK:C\n(5CDEFG A|",
        // Additive meters; the default unit comes from the whole meter.
        "X:1\nM:(2+3+2)/8\nK:C\nCDEF|Z|G|",
        "X:1\nM:2+3+2/8\nK:C\nCDEF|Z|G|",
        // A skip in a tuplet is a tuplet value; broken rhythm reaches into a
        // tuplet just closed; a `[V:` in a comment is a comment.
        "X:1\nL:1/8\nK:C\n(3xAB c|",
        "X:1\nL:1/8\nK:C\n(3ABc>d e|",
        "X:1\nK:C\nCDEF|% moves to [V:2] later\n",
        // `&` overlays: layers start together.
        "X:1\nL:1/4\nK:C\nC D & E F|G A|",
        "X:1\nL:1/4\nK:C\nC D E F & G2|A|",
        "X:1\nL:1/4\nK:C\nC2 & E2 & G2|C|",
    ]);
}

#[test]
fn inline_voices_alone_make_their_staves() {
    let staves = |abc: &str| -> Vec<u8> {
        let s = AbcToIrAdapter::new().convert_str(abc).unwrap();
        s.parts().iter().map(|p| p.staves).collect()
    };
    let declared = staves("X:1\nL:1/4\nV:S\nV:A\nK:C\n[V:S] C D|\n[V:A] E F|\n");
    assert_eq!(
        staves("X:1\nL:1/4\nK:C\n[V:S] C D|\n[V:A] E F|\n"),
        declared
    );
    assert_eq!(
        staves("X:1\nL:1/4\nK:C\n [V:S] C D|\n[V:A] E F|\n"),
        declared
    );
}
