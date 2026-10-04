//! LilyPond as the oracle for lytk's LilyPond and MIDI writers.
//!
//! Each fixture is written as LilyPond by lytk and compiled to MIDI by the
//! real `lilypond`. That MIDI must match what lytk's own MIDI writer makes of
//! the same score, note by note. A difference means one of the two writers
//! is wrong: the LilyPond text doesn't say what the score says, or the MIDI
//! writer plays it differently from LilyPond.
//!
//! It needs the `lilypond` binary, so it is ignored by default:
//! `cargo test --test lilypond_oracle -- --ignored --nocapture`
//! (CI's Linux job installs LilyPond and runs it).

mod common;

use common::smf::{self, SmfNote};
use common::{common_count, safe};

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::score::Score;

use std::path::{Path, PathBuf};
use std::process::Command;

// Committed baselines, summed over every fixture LilyPond compiled: its
// notes matched by ours on (on, pitch), (on, off, pitch) and with velocity.
// They may only rise.
const ORACLE_ON: usize = 11669;
const ORACLE_ON_OFF: usize = 11650;
// Velocities differ by design where a score writes its dynamics in a
// `\new Dynamics` staff (chopin_n, pedal, repeats): LilyPond doesn't perform
// them (its Dynamics context has only the pedal performer), lytk plays them
// on the part's notes.
const ORACLE_FULL: usize = 9935;
/// Fixtures whose lytk LilyPond output compiles (LilyPond 2.22.1, 2026-09-26).
const ORACLE_COMPILED: usize = 148;
/// Syllables LilyPond sings where lytk's IR has them, writing lytk's
/// LilyPond (all verses, onset and text), of the IR's.
const ORACLE_SUNG: usize = 710;

fn list(dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.retain(|p| p.extension().is_some_and(|e| e == ext));
    v.sort();
    v
}

fn sources() -> Vec<(String, Score)> {
    let mut out = Vec::new();
    for p in list("tests/fixtures/xml", "xml") {
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        if let Some(s) = safe(|| MxmlToIrAdapter::new().convert_file(&p).ok()) {
            out.push((name, s));
        }
    }
    for ly in ["chopin_n", "example", "example2", "pedal", "repeats"] {
        let path = Path::new("tests/fixtures/ly").join(format!("{ly}.ly"));
        let scores =
            safe(|| LyToIrAdapter::new().convert_file_multi(&path).ok()).unwrap_or_default();
        for (i, s) in scores.into_iter().enumerate() {
            out.push((format!("{ly}-{i}"), s));
        }
    }
    out
}

fn keyed(notes: &[SmfNote], ppq: u32, what: u8, shift: u32) -> Vec<(u32, u32, u8, u8)> {
    notes
        .iter()
        .map(|n| {
            let (on, off) = (n.on * 384 / ppq, n.off * 384 / ppq);
            let (on, off) = (on.saturating_sub(shift), off.saturating_sub(shift));
            match what {
                0 => (on, 0, n.pitch, 0),
                1 => (on, off, n.pitch, 0),
                _ => (on, off, n.pitch, n.vel),
            }
        })
        .collect()
}

#[test]
#[ignore = "needs the lilypond binary"]
fn lilypond_plays_our_ly_like_our_midi() {
    let Ok(version) = Command::new("lilypond").arg("--version").output() else {
        eprintln!("lilypond not found: skipped");
        return;
    };
    let version = String::from_utf8_lossy(&version.stdout);
    println!("{}", version.lines().next().unwrap_or("lilypond"));

    let dir = tempfile::tempdir().expect("temp dir");
    let mut jobs: Vec<(String, Score)> = Vec::new();
    for (name, score) in sources() {
        let Some(ly) = safe(|| IrToLyAdapter::new().convert(&score).ok()) else {
            println!("{name}: lytk could not write LilyPond");
            continue;
        };
        // MIDI only: no page layout, so LilyPond runs fast.
        let ly = ly.replace("\\layout { }", "");
        std::fs::write(dir.path().join(format!("{name}.ly")), ly).expect("write");
        jobs.push((name, score));
    }
    // One LilyPond process per batch (Guile starts once); a file that fails
    // doesn't stop the others.
    for chunk in jobs.chunks(25) {
        let _ = Command::new("lilypond")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .current_dir(dir.path())
            .arg("-dno-point-and-click")
            .arg("-dno-print-pages")
            .arg("--loglevel=ERROR")
            .args(chunk.iter().map(|(n, _)| format!("{n}.ly")))
            .output()
            .expect("run lilypond");
    }

    // A file that crashes LilyPond can take the rest of its batch with it:
    // retry every missing one alone, keeping its first error for the report.
    let mut errors: Vec<String> = Vec::new();
    for (name, _) in &jobs {
        if dir.path().join(format!("{name}.midi")).exists() {
            continue;
        }
        let out = Command::new("lilypond")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .current_dir(dir.path())
            .arg("-dno-point-and-click")
            .arg("-dno-print-pages")
            .arg(format!("{name}.ly"))
            .output()
            .expect("run lilypond");
        if !dir.path().join(format!("{name}.midi")).exists() {
            let err = String::from_utf8_lossy(&out.stderr);
            // The version check always fails on an older LilyPond; skip it.
            let first = err
                .lines()
                .filter(|l| l.contains("error") && !l.contains("program too old"))
                .take(2)
                .collect::<Vec<_>>()
                .join(" / ");
            let first = if first.is_empty() {
                "no MIDI written".to_string()
            } else {
                first
            };
            errors.push(format!("{name}: {first}"));
        }
    }

    let (mut on, mut on_off, mut full, mut total) = (0, 0, 0, 0);
    let (mut sung_right, mut sung_total) = (0, 0);
    let mut unsung: Vec<(usize, String)> = Vec::new();
    let mut missing = Vec::new();
    let mut worst: Vec<(usize, String)> = Vec::new();
    let mut loudness: Vec<(usize, String)> = Vec::new();
    for (name, score) in &jobs {
        let Ok(theirs) = std::fs::read(dir.path().join(format!("{name}.midi"))) else {
            missing.push(name.clone());
            continue;
        };
        // lytk's `.ly` keeps repeats as written (no `\unfoldRepeats`), so
        // LilyPond plays them once through: compare with the written order.
        let written = || {
            IrToMidiAdapter::new()
                .with_unfold_repeats(false)
                .convert_bytes(score)
                .ok()
        };
        let Some(ours) = safe(written) else {
            missing.push(format!("{name} (lytk MIDI)"));
            continue;
        };
        let (t, o) = (smf::notes(&theirs), smf::notes(&ours));
        let (pt, po) = (smf::ppq(&theirs) as u32, smf::ppq(&ours) as u32);
        // LilyPond starts a piece that opens with a grace note that much
        // early and plays everything later (its time signature stays at 0):
        // compare at the offset that matches most onsets.
        let ours = keyed(&o, po, 0, 0);
        let shift = (0..t.len().min(4))
            .map(|i| t[i].on * 384 / pt)
            .flat_map(|x| ours.iter().take(4).map(move |y| x.saturating_sub(y.0)))
            .chain([0])
            .max_by_key(|&d| (common_count(&keyed(&t, pt, 0, d), &ours), d == 0))
            .unwrap_or(0);
        // The lyrics: every Lyrics context is a track of lyric events.
        let ir = sung(score);
        let lily: Vec<(_core::ir::duration::Frac, String)> = smf::lyrics(&theirs)
            .into_iter()
            .filter(|(_, text, _)| !text.trim().is_empty())
            .map(|(tick, text, _)| {
                let tick = (tick * 384 / pt).saturating_sub(shift) as i64;
                (_core::ir::duration::Frac::new(tick, 4 * 384), elided(&text))
            })
            .collect();
        let right = common_count(&lily, &ir);
        sung_right += right;
        sung_total += ir.len();
        if right < ir.len() {
            unsung.push((
                ir.len() - right,
                format!(
                    "{name}: {right}/{} syllables, LilyPond sang {}",
                    ir.len(),
                    lily.len()
                ),
            ));
        }
        let a = common_count(&keyed(&t, pt, 0, shift), &ours);
        let b = common_count(&keyed(&t, pt, 1, shift), &keyed(&o, po, 1, 0));
        let c = common_count(&keyed(&t, pt, 2, shift), &keyed(&o, po, 2, 0));
        on += a;
        on_off += b;
        full += c;
        total += t.len();
        if c < b {
            loudness.push((b - c, format!("{name}: {c}/{b} velocities")));
        }
        if a < t.len() || o.len() != t.len() {
            worst.push((
                t.len() - a + o.len().abs_diff(t.len()),
                format!("{name}: {a}/{} onsets, lytk {} notes", t.len(), o.len()),
            ));
        }
    }
    worst.sort_by_key(|x| std::cmp::Reverse(x.0));
    loudness.sort_by_key(|x| std::cmp::Reverse(x.0));

    println!("\n===== LilyPond plays lytk's LilyPond vs lytk's MIDI =====");
    println!(
        "{} fixtures compiled, {} not: {on}/{total} on+pitch, {on_off}/{total} +off, {full}/{total} +velocity",
        jobs.len() - missing.len(),
        missing.len()
    );
    println!("not compiled:");
    for e in &errors {
        println!("  {e}");
    }
    println!("largest differences:");
    for (_, w) in worst.iter().take(25) {
        println!("  {w}");
    }
    println!("velocities apart (of notes matching on and off):");
    for (_, w) in loudness.iter().take(15) {
        println!("  {w}");
    }
    unsung.sort_by_key(|x| std::cmp::Reverse(x.0));
    println!("lyrics: LilyPond sings {sung_right}/{sung_total} syllables where lytk has them (baseline {ORACLE_SUNG})");
    for (_, w) in unsung.iter().take(15) {
        println!("  {w}");
    }
    let compiled = jobs.len() - missing.len();
    assert!(
        compiled >= ORACLE_COMPILED,
        "fewer files compile: {compiled} < {ORACLE_COMPILED}"
    );
    assert!(on >= ORACLE_ON, "on+pitch regressed: {on} < {ORACLE_ON}");
    assert!(
        on_off >= ORACLE_ON_OFF,
        "+off regressed: {on_off} < {ORACLE_ON_OFF}"
    );
    assert!(
        full >= ORACLE_FULL,
        "+velocity regressed: {full} < {ORACLE_FULL}"
    );
    assert!(
        sung_right >= ORACLE_SUNG,
        "lyrics regressed: {sung_right} < {ORACLE_SUNG}"
    );
}

// ---------------------------------------------------------------------------
// Lyric alignment (Epic M, M0)
// ---------------------------------------------------------------------------

/// Cases where lytk's LilyPond reader puts each syllable on the note LilyPond
/// sings it on (from LilyPond's MIDI lyric events). May only rise.
const LYRICS_ALIGNED: usize = 31;

/// LilyPond lyric cases: each a full file. Every rule `\lyricsto` and
/// `\addlyrics` follow is covered once: melismas from slurs, ties, manual
/// beams under `\autoBeamOff` and `\melisma`; extenders, hyphens, skips,
/// elisions; several stanzas and voices; words with punctuation.
fn lyric_cases() -> Vec<(&'static str, String)> {
    let one = |music: &str, words: &str| {
        format!(
            "\\version \"2.22.0\"\n\\score {{ << \\new Voice = \"v\" {{ {music} }}\n\
             \\new Lyrics \\lyricsto \"v\" {{ {words} }} >> \\layout {{ }} \\midi {{ }} }}\n"
        )
    };
    let plain = "c'4 d' e' f' | g'1";
    vec![
        ("plain", one(plain, "a b c d e")),
        ("slur", one("c'4 d'( e') f' | g'1", "a b c d")),
        ("tie", one("c'4 d'~ d' f' | g'1", "a b c d")),
        ("tie-over-bar", one("c'2 d'~ | d'4 e' f' g'", "a b c d e")),
        ("extender", one(plain, "a __ b c d e")),
        ("extender-slur", one("c'4( d') e' f' | g'1", "a __ b c d")),
        ("hyphen", one(plain, "a -- b c -- d e")),
        ("skip", one(plain, "a _ b c d")),
        ("lyric-tie", one(plain, "a~b c d e f")),
        ("underscore-word", one(plain, "a_b c d e f")),
        ("punctuation", one(plain, "don't stop, now! go. on")),
        ("skip-command", one(plain, "\\skip 4 b c d e")),
        ("repeat-unfold", one(plain, "\\repeat unfold 2 { \\skip 4 } c d e")),
        ("melisma", one("c'4 d'\\melisma e'\\melismaEnd f' | g'1", "a b c d")),
        ("beam", one("c'8 d'[ e'] f' g'2 | a'1", "a b c d e")),
        (
            "beam-autobeamoff",
            one("\\autoBeamOff c'8 d'[ e'] f' g'2 | a'1", "a b c d e"),
        ),
        (
            "ignore-melismata",
            one("c'4 d'( e') f' | g'1", "\\set ignoreMelismata = ##t a b c d e"),
        ),
        ("chord", one("<c' e'>4 d' e' f' | g'1", "a b c d e")),
        ("chord-tie", one("<c' e'>4~ <c' e'> d' e' | f'1", "a b c d")),
        ("grace", one("c'4 \\grace d'8 e'4 f' g' | a'1", "a b c d e")),
        ("rests", one("c'4 r d' e' | f'1", "a b c d")),
        ("stanza", one(plain, "\\set stanza = \"1.\" a b c d e")),
        (
            "two-stanzas",
            "\\version \"2.22.0\"\n\\score { << \\new Voice = \"v\" { c'4 d' e' f' }\n\
             \\new Lyrics \\lyricsto \"v\" { a b c d }\n\
             \\new Lyrics \\lyricsto \"v\" { e f g h } >> \\layout { } \\midi { } }\n"
                .to_string(),
        ),
        (
            "two-voices",
            "\\version \"2.22.0\"\n\\score { \\new Staff << \\new Voice = \"s\" { \\voiceOne e''4 f'' g'' a'' }\n\
             \\new Voice = \"a\" { \\voiceTwo c''2 d'' }\n\
             \\new Lyrics \\lyricsto \"s\" { sa sb sc sd }\n\
             \\new Lyrics \\lyricsto \"a\" { aa ab } >> \\layout { } \\midi { } }\n"
                .to_string(),
        ),
        (
            "addlyrics-twice",
            "\\version \"2.22.0\"\n\\score { { c'4 d' e' f' } \\addlyrics { a b c d }\n\
             \\addlyrics { e f g h } \\layout { } \\midi { } }\n"
                .to_string(),
        ),
        // A `\\` passage is new voices: the lyrics' voice has no notes there.
        (
            "polyphony",
            one("c'4 d' << { e' f' } \\\\ { c' c' } >> | g'1", "a b c d e"),
        ),
        // The idiom that keeps the voice: the first branch continues it.
        (
            "polyphony-kept",
            one(
                "c'4 d' << { \\voiceOne e' f' } \\new Voice { \\voiceTwo c'2 } >> \\oneVoice | g'1",
                "a b c d e",
            ),
        ),
        // Music after a Voice stays in it.
        (
            "after-voice",
            "\\version \"2.22.0\"\n\\score { << \\new Staff { \\new Voice = \"v\" { \\time 4/4 } \
             c'4 d' e' f' }\n\\new Lyrics \\lyricsto \"v\" { a b c d } >> \\layout { } \\midi { } }\n"
                .to_string(),
        ),
        // Two named voices on a staff in a score, each with its lyrics.
        (
            "voices-in-score",
            "\\version \"2.22.0\"\n\\score { << \\new Staff << \
             \\new Voice = \"s\" { \\voiceOne e''4 f'' g'' a'' }\n\
             \\new Voice = \"a\" { \\voiceTwo c''2 d'' } >>\n\
             \\new Lyrics \\lyricsto \"s\" { sa sb sc sd }\n\
             \\new Lyrics \\lyricsto \"a\" { aa ab } >> \\layout { } \\midi { } }\n"
                .to_string(),
        ),
        // A NullVoice's rhythm under another voice's notes.
        (
            "null-voice",
            "\\version \"2.22.0\"\n\\score { << \\new Staff << \
             \\new Voice = \"s\" { e''4 f'' g'' a'' }\n\
             \\new NullVoice = \"a\" { c''2 d'' } >>\n\
             \\new Lyrics \\lyricsto \"a\" { aa ab } >> \\layout { } \\midi { } }\n"
                .to_string(),
        ),
        (
            "durations",
            "\\version \"2.22.0\"\n\\score { << \\new Voice { c'4 d' e' f' }\n\
             \\new Lyrics \\lyricmode { a4 b2 c4 } >> \\layout { } \\midi { } }\n"
                .to_string(),
        ),
    ]
}

/// An elision as `~`: LilyPond 2.22's MIDI writes `~`, 2.24's writes `‿`
/// (as lytk's IR does).
fn elided(text: &str) -> String {
    text.replace('\u{203f}', "~")
}

/// The syllables of a score as (onset in whole notes, text), sorted.
fn sung(score: &Score) -> Vec<(_core::ir::duration::Frac, String)> {
    let mut v: Vec<_> = common::notation_signature(score)
        .lyrics
        .into_iter()
        .map(|(_, onset, text, ..)| {
            let at = _core::ir::duration::Frac::new(onset as i64, 1920);
            (at, elided(&text))
        })
        .collect();
    v.sort();
    v
}

#[test]
#[ignore = "needs the lilypond binary"]
fn lilypond_sings_lyrics_where_lytk_reads_them() {
    if Command::new("lilypond").arg("--version").output().is_err() {
        eprintln!("lilypond not found: skipped");
        return;
    }
    let dir = tempfile::tempdir().expect("temp dir");
    let cases = lyric_cases();
    for (name, src) in &cases {
        std::fs::write(dir.path().join(format!("{name}.ly")), src).expect("write");
    }
    let _ = Command::new("lilypond")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .current_dir(dir.path())
        .arg("-dno-point-and-click")
        .arg("-dno-print-pages")
        .arg("--loglevel=ERROR")
        .args(cases.iter().map(|(n, _)| format!("{n}.ly")))
        .output()
        .expect("run lilypond");

    let mut aligned = 0;
    println!("\n===== LilyPond's lyric alignment vs lytk's =====");
    for (name, src) in &cases {
        let Ok(midi) = std::fs::read(dir.path().join(format!("{name}.midi"))) else {
            println!("{name}: LilyPond wrote no MIDI");
            continue;
        };
        let ppq = smf::ppq(&midi) as i64;
        let mut theirs: Vec<(_core::ir::duration::Frac, String)> = smf::lyrics(&midi)
            .into_iter()
            .filter(|(_, text, _)| !text.trim().is_empty()) // a `_` skip sings ""
            .map(|(t, text, _)| {
                (
                    _core::ir::duration::Frac::new(t as i64, 4 * ppq),
                    elided(&text),
                )
            })
            .collect();
        theirs.sort();
        let ours = safe(|| LyToIrAdapter::new().convert_str(src).ok())
            .map(|s| sung(&s))
            .unwrap_or_default();
        if ours == theirs {
            aligned += 1;
        } else {
            let show = |v: &[(_core::ir::duration::Frac, String)]| {
                v.iter()
                    .map(|(t, s)| format!("{t}:{s}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!(
                "{name}:\n    lilypond {}\n    lytk     {}",
                show(&theirs),
                show(&ours)
            );
        }
    }
    println!(
        "{aligned}/{} cases aligned (baseline {LYRICS_ALIGNED})",
        cases.len()
    );
    assert!(
        aligned >= LYRICS_ALIGNED,
        "lyric alignment regressed: {aligned} < {LYRICS_ALIGNED}"
    );
}
