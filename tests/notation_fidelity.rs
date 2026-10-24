//! Notation-fidelity boards (Epic M, M0): what the note boards in
//! `fidelity.rs` don't measure. Each fixture's lyric syllables, chord
//! symbols, dynamics and clefs, at their onsets (`common::notation_signature`),
//! must survive each conversion. A family counts for a fixture only when the
//! fixture has some of it. The baselines may only rise.
//!
//! Also gated: how many MusicXML fixtures read with a voice longer than its
//! bar (`common::overfull_voice`), which may only fall. A reader that places
//! notes by file order instead of by time makes such voices, and the note
//! boards can't see it: lytk reads its own output the same wrong way.
//!
//! Run with output: `cargo test --test notation_fidelity -- --nocapture`

mod common;

use common::{notation_signature, overfull_voice, safe, NotationSig};

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::humdrum_to_ir::HumdrumToIrAdapter;
use _core::adapters::ir_to_abc::IrToAbcAdapter;
use _core::adapters::ir_to_humdrum::IrToHumdrumAdapter;
use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
use _core::ir::score::Score;

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Committed baselines: fixtures whose family survives, per direction, as
// (lyrics, chord symbols, dynamics, clefs). Measured 2026-10-01 (0.4.0);
// M1/M2 (positioned MusicXML reading, mid-bar attributes merged) raised
// the clefs and XML→ABC dynamics; M7 (chord symbols) the chords. Chords
// stacked before one note now change during it (suite 71g), which ABC can't
// write: three files that matched by sitting on beat 1 on both sides don't.
// ---------------------------------------------------------------------------
const XML_XML: [usize; 4] = [38, 8, 13, 136];
const XML_LY: [usize; 4] = [33, 7, 8, 131];
const XML_ABC: [usize; 4] = [31, 4, 9, 108];
const XML_KRN: [usize; 4] = [38, 0, 0, 110];
/// MIDI carries the first verse only, and nothing else of these.
const XML_MIDI: [usize; 4] = [33, 0, 0, 0];
const LY_LY: [usize; 4] = [1, 1, 2, 16];
const LY_XML: [usize; 4] = [2, 1, 2, 17];
/// MusicXML fixtures read with a voice longer than its bar. May only fall.
const OVERFULL_MAX: usize = 4;

const FAMILIES: [&str; 4] = ["lyrics", "chords", "dynamics", "clefs"];

#[derive(Default)]
struct Board {
    /// Fixtures having each family.
    total: [usize; 4],
    /// Fixtures whose family survived.
    ok: [usize; 4],
    /// First failures per family, for the report.
    fails: [Vec<String>; 4],
}

fn families(s: &NotationSig) -> [bool; 4] {
    [
        !s.lyrics.is_empty(),
        !s.harmonies.is_empty(),
        !s.dynamics.is_empty(),
        !s.clefs.is_empty(),
    ]
}

fn same(a: &NotationSig, b: &NotationSig, family: usize) -> bool {
    match family {
        0 => a.lyrics == b.lyrics,
        1 => a.harmonies == b.harmonies,
        2 => a.dynamics == b.dynamics,
        _ => a.clefs == b.clefs,
    }
}

/// What a MIDI file can carry of the signature: the first verse, without
/// extender lines.
fn midi_view(s: &NotationSig) -> NotationSig {
    let s = no_extenders(s);
    NotationSig {
        lyrics: s.lyrics.into_iter().filter(|l| l.0 == 1).collect(),
        ..NotationSig::default()
    }
}

/// The signature without extender lines, which kern's `**text` has no way
/// to write.
fn no_extenders(s: &NotationSig) -> NotationSig {
    let mut s = s.clone();
    for l in &mut s.lyrics {
        l.4 = false;
    }
    s
}

fn record(b: &mut Board, name: &str, before: &NotationSig, after: Option<&NotationSig>) {
    for (f, has) in families(before).into_iter().enumerate() {
        if !has {
            continue;
        }
        b.total[f] += 1;
        if after.is_some_and(|a| same(before, a, f)) {
            b.ok[f] += 1;
        } else if b.fails[f].len() < 6 {
            b.fails[f].push(name.to_string());
        }
    }
}

fn report(name: &str, b: &Board, base: [usize; 4]) {
    let cells: Vec<String> = (0..4)
        .map(|f| {
            format!(
                "{} {}/{} (base {})",
                FAMILIES[f], b.ok[f], b.total[f], base[f]
            )
        })
        .collect();
    println!("{name:10}: {}", cells.join(", "));
    for (family, fails) in FAMILIES.iter().zip(&b.fails) {
        if !fails.is_empty() {
            println!("    {family} lost in: {}", fails.join(", "));
        }
    }
}

fn gate(name: &str, b: &Board, base: [usize; 4]) {
    for ((family, ok), base) in FAMILIES.iter().zip(b.ok).zip(base) {
        assert!(
            ok >= base,
            "{name} {family} fidelity regressed: {ok} < {base}"
        );
    }
}

fn list(dir: &str, exts: &[&str]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.retain(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
    });
    v.sort();
    v
}

fn xml_sources() -> Vec<(String, Score)> {
    let mut out = Vec::new();
    for p in list("tests/fixtures/xml", &["xml"])
        .into_iter()
        .chain(list("tests/fixtures/mxl", &["mxl"]))
        .chain(list("tests/fixtures/musicxml", &["mxl"]))
    {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if let Some(s) = safe(|| MxmlToIrAdapter::new().convert_file(&p).ok()) {
            out.push((name, s));
        }
    }
    out
}

fn ly_sources() -> Vec<(String, String, Score)> {
    let mut out = Vec::new();
    for p in list("tests/fixtures/ly", &["ly"]) {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let Ok(src) = std::fs::read_to_string(&p) else {
            continue;
        };
        if let Some(s) = safe(|| LyToIrAdapter::new().convert_str(&src).ok()) {
            out.push((name, src, s));
        }
    }
    out
}

#[test]
fn notation_fidelity_scoreboard() {
    let xml = xml_sources();
    let (mut xml_xml, mut xml_ly, mut xml_abc, mut xml_krn, mut xml_midi) = (
        Board::default(),
        Board::default(),
        Board::default(),
        Board::default(),
        Board::default(),
    );
    for (name, score) in &xml {
        let before = notation_signature(score);
        let after = |f: &dyn Fn() -> Option<Score>| safe(f).map(|s| notation_signature(&s));

        let a = after(&|| {
            let out = IrToMxmlAdapter::new().convert(score).ok()?;
            MxmlToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut xml_xml, name, &before, a.as_ref());

        let a = after(&|| {
            let out = IrToLyAdapter::new().convert(score).ok()?;
            LyToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut xml_ly, name, &before, a.as_ref());

        let a = after(&|| {
            let doc = _core::ir::lift::lift_to_music(score);
            let out = IrToAbcAdapter::new().convert_music(&doc).ok()?;
            AbcToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut xml_abc, name, &before, a.as_ref());

        let a = after(&|| {
            let doc = _core::ir::lift::lift_to_music(score);
            let out = IrToHumdrumAdapter::new().convert_music(&doc).ok()?;
            HumdrumToIrAdapter::new().convert_str(&out).ok()
        });
        record(
            &mut xml_krn,
            name,
            &no_extenders(&before),
            a.as_ref().map(no_extenders).as_ref(),
        );

        let a = after(&|| {
            let bytes = IrToMidiAdapter::new()
                .with_unfold_repeats(false)
                .convert_bytes(score)
                .ok()?;
            MidiToIrAdapter::new().convert_bytes(&bytes).ok()
        });
        record(
            &mut xml_midi,
            name,
            &midi_view(&before),
            a.as_ref().map(midi_view).as_ref(),
        );
    }

    let (mut ly_ly, mut ly_xml) = (Board::default(), Board::default());
    for (name, src, score) in &ly_sources() {
        let before = notation_signature(score);
        let a = safe(|| {
            let doc = LyToIrAdapter::new().convert_str_to_music(src).ok()?;
            let out = IrToLyAdapter::new().convert_music(&doc).ok()?;
            LyToIrAdapter::new().convert_str(&out).ok()
        })
        .map(|s| notation_signature(&s));
        record(&mut ly_ly, name, &before, a.as_ref());
        let a = safe(|| {
            let out = IrToMxmlAdapter::new().convert(score).ok()?;
            MxmlToIrAdapter::new().convert_str(&out).ok()
        })
        .map(|s| notation_signature(&s));
        record(&mut ly_xml, name, &before, a.as_ref());
    }

    let boards = [
        ("XML→XML", &xml_xml, XML_XML),
        ("XML→LY", &xml_ly, XML_LY),
        ("XML→ABC", &xml_abc, XML_ABC),
        ("XML→KRN", &xml_krn, XML_KRN),
        ("XML→MIDI", &xml_midi, XML_MIDI),
        ("LY→LY", &ly_ly, LY_LY),
        ("LY→XML", &ly_xml, LY_XML),
    ];
    println!("\n===== Notation fidelity scoreboard =====");
    for (name, b, base) in boards {
        report(name, b, base);
    }

    let overfull: Vec<String> = xml
        .iter()
        .filter_map(|(name, s)| {
            overfull_voice(s).map(|(p, m, v)| format!("{name} ({p} bar {m} voice {v})"))
        })
        .collect();
    println!(
        "overfull voices after reading: {} (max {OVERFULL_MAX})",
        overfull.len()
    );
    for o in &overfull {
        println!("    {o}");
    }

    for (name, b, base) in boards {
        gate(name, b, base);
    }
    assert!(
        overfull.len() <= OVERFULL_MAX,
        "more MusicXML fixtures read with overfull voices: {} > {OVERFULL_MAX}",
        overfull.len()
    );
}

// ---------------------------------------------------------------------------
// Stems and beams (M4): each MusicXML fixture that has them, stripped and
// engraved again by the MusicXML writer, against what its source's engraver
// (MuseScore, Finale, Sibelius, …) drew. Per note: the stem direction, the
// first-level beam (begin, continue, end or none). Agreement in ‰, which may
// only rise. Engravers disagree among themselves, so 1000 isn't the target.
// ---------------------------------------------------------------------------
const STEMS_AGREE_PERMILLE: usize = 980;
const BEAMS_AGREE_PERMILLE: usize = 941;

#[test]
fn engraved_stems_and_beams_agree_with_the_sources() {
    use _core::ir::note::VoiceElement;
    // (stem, first-level beam) per note, in score order.
    fn marks(score: &Score) -> Vec<Vec<(String, String)>> {
        score
            .parts()
            .iter()
            .flat_map(|p| &p.measures)
            .flat_map(|m| &m.voices)
            .map(|v| {
                v.elements
                    .iter()
                    .filter(|e| !matches!(e, VoiceElement::Rest(_)))
                    .map(|e| {
                        let n = &e.notes()[0];
                        let beam = n.beams.iter().find(|b| b.number == 1);
                        (
                            n.stem_direction.map_or(String::new(), |s| s.to_string()),
                            beam.map_or(String::new(), |b| b.beam_type.to_string()),
                        )
                    })
                    .collect()
            })
            .collect()
    }
    let (mut stems, mut beams, mut total) = (0usize, 0usize, 0usize);
    for (_, source) in xml_sources() {
        let given = marks(&source);
        if given
            .iter()
            .flatten()
            .all(|(s, b)| s.is_empty() && b.is_empty())
        {
            continue;
        }
        let mut bare = source.clone();
        for e in bare
            .parts_mut()
            .into_iter()
            .flat_map(|p| &mut p.measures)
            .flat_map(|m| &mut m.voices)
            .flat_map(|v| &mut v.elements)
        {
            for n in e.notes_mut() {
                n.stem_direction = None;
                n.beams.clear();
                n.no_auto_beam = false;
            }
        }
        let Some(drawn) = safe(|| {
            let xml = IrToMxmlAdapter::new().convert(&bare).ok()?;
            MxmlToIrAdapter::new().convert_str(&xml).ok()
        }) else {
            continue;
        };
        for (a, b) in given.iter().zip(marks(&drawn)) {
            if a.len() != b.len() {
                continue;
            }
            for ((sa, ba), (sb, bb)) in a.iter().zip(&b) {
                // A note the source gave no stem (a whole note) isn't scored.
                if !sa.is_empty() {
                    total += 1;
                    stems += usize::from(sa == sb);
                    beams += usize::from(ba == bb);
                }
            }
        }
    }
    let permille = |n: usize| n * 1000 / total.max(1);
    println!(
        "engraved vs source: stems {}‰ (base {STEMS_AGREE_PERMILLE}), beams {}‰ (base {BEAMS_AGREE_PERMILLE}) over {total} notes",
        permille(stems),
        permille(beams),
    );
    assert!(
        permille(stems) >= STEMS_AGREE_PERMILLE,
        "stem agreement regressed"
    );
    assert!(
        permille(beams) >= BEAMS_AGREE_PERMILLE,
        "beam agreement regressed"
    );
}
