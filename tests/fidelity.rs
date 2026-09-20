//! Semantic-fidelity scoreboard (Epic C / ECT3, extended in the 1.0 hardening).
//!
//! Round-trips every fixture and measures whether *musical content* survives —
//! note count, the pitch multiset, AND the (onset, duration, pitch) note
//! signature (so duration/onset corruption is caught, not just pitch) — per
//! conversion direction. Prints a report and gates on a committed baseline so
//! fidelity can only improve, never regress (the non-decreasing-fidelity gate).
//!
//! Directions measured:
//!   - LY  → IR → LY  → IR   (Music path, the CLI route)
//!   - XML → IR → XML → IR
//!   - ABC → IR → ABC → IR
//!   - MIDI→ IR → MIDI→ IR
//!
//! Run with output: `cargo test --test fidelity -- --nocapture`

mod common;

use common::{note_signature, pitch_multiset, signature};

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

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Committed baselines (the non-decreasing-fidelity gate).
// Bump these UP when fidelity improves; never down.
// Each direction: (note-count, pitch-multiset, onset+duration signature).
// ---------------------------------------------------------------------------
const LY_NOTES_BASELINE: usize = 35;
const LY_PITCHES_BASELINE: usize = 35;
// 8 complex multi-voice LY fixtures (chopin/example/pedal) still drift on
// onset/duration through the Music-path round-trip — a known limitation, gated
// at the current floor so it can't get worse.
const LY_DUR_BASELINE: usize = 27;
const XML_NOTES_BASELINE: usize = 152;
const XML_PITCHES_BASELINE: usize = 152;
const XML_DUR_BASELINE: usize = 152; // full onset+duration fidelity
                                     // Cross-format: the same MusicXML corpus written out as ABC / Humdrum and read
                                     // back. Both formats are narrower than MusicXML (no grace notes in ABC, no
                                     // inner polyphony in either writer), so these sit well below the XML numbers —
                                     // the point is that they can only go up. Filled in from the measured run.
                                     // The drifters that remain are the un-notatable-duration fixtures (a 31/8 bar
                                     // note has no single spelling, so the Layer-1 lowering splits it into tied
                                     // notes) and the inner-polyphony ones (both writers are one stream per staff).
const XML_ABC_NOTES_BASELINE: usize = 124;
const XML_ABC_PITCHES_BASELINE: usize = 124;
const XML_ABC_DUR_BASELINE: usize = 122;
const XML_KRN_NOTES_BASELINE: usize = 131;
const XML_KRN_PITCHES_BASELINE: usize = 129;
const XML_KRN_DUR_BASELINE: usize = 130;
// ABC now includes a multi-voice fixture (multivoice.abc) that round-trips.
const ABC_NOTES_BASELINE: usize = 4;
const ABC_PITCHES_BASELINE: usize = 4;
const ABC_DUR_BASELINE: usize = 4;
// MIDI round-trip after the export-side carried-meter fix (`ir_to_midi`
// `build_part_track` now sizes every bar by the running time signature instead
// of padding un-declared bars to 4/4). This re-aligns the non-4/4 pieces:
// `example2_1` now matches on note-count AND pitch-multiset (3/5 each), up from
// 2/5. The two remaining drifters are documented limitations, gated so they
// can't regress:
//   - `example2_1`: one note's *duration* still drifts (960→1200 ticks) — a
//     tie re-fuses across a meter boundary on re-import (onset+dur stays 2/5).
//   - `pedal`: a per-voice quantized-budget cascade starves one A4 in a dense
//     bar, swapping a 480-tick note for a 30-tick sliver (off by 1 note).
//   - `chopin_n`: INHERENT — the source MIDI's time-signature changes sit on
//     non-bar-aligned ticks (6/8→4/4 at tick 100416, not a 6/8-bar multiple),
//     so its notated meter and actual bar lengths disagree. An exact
//     onset/duration round-trip would need arbitrary mid-bar re-gridding, which
//     itself breaks fidelity — not fixable without abandoning the exact metric.
// Review R5 (2026-07-10): the reader now FIFO-pairs overlapping same-pitch
// notes per (key, channel) and the writer orders note-offs before note-ons at
// equal ticks — cross-voice unisons no longer drop notes. Baselines raised
// 3/3/2 → 4/4/3 (only chopin_n's inherent meter drift remains).
const MIDI_NOTES_BASELINE: usize = 4;
const MIDI_PITCHES_BASELINE: usize = 4;
const MIDI_DUR_BASELINE: usize = 3;

#[derive(Default)]
struct Board {
    total: usize,
    notes_ok: usize,
    pitches_ok: usize,
    dur_ok: usize,
    /// fixtures whose content changed (for the report)
    fails: Vec<String>,
}

fn list(dir: &str, exts: &[&str]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| exts.contains(&e))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

fn safe<T, F: FnOnce() -> Option<T>>(f: F) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok().flatten()
}

fn record(board: &mut Board, fixture: &str, before: &Score, after: Option<&Score>) {
    board.total += 1;
    let Some(after) = after else {
        board.fails.push(format!("{fixture} (round-trip failed)"));
        return;
    };
    let sb = signature(before);
    let sa = signature(after);
    if sb.notes == sa.notes {
        board.notes_ok += 1;
    }
    let pitch_ok = pitch_multiset(before) == pitch_multiset(after);
    if pitch_ok {
        board.pitches_ok += 1;
    }
    if note_signature(before) == note_signature(after) {
        board.dur_ok += 1;
    }
    if !pitch_ok && board.fails.len() < 12 {
        board
            .fails
            .push(format!("{fixture} ({} → {} notes)", sb.notes, sa.notes));
    }
}

fn report(name: &str, b: &Board, base: (usize, usize, usize)) {
    println!(
        "{name} : {}/{} note-count, {}/{} pitch, {}/{} onset+dur (baseline {}/{}/{})",
        b.notes_ok, b.total, b.pitches_ok, b.total, b.dur_ok, b.total, base.0, base.1, base.2
    );
}

fn gate(name: &str, b: &Board, base: (usize, usize, usize)) {
    assert!(
        b.notes_ok >= base.0,
        "{name} note-count fidelity regressed: {} < {}",
        b.notes_ok,
        base.0
    );
    assert!(
        b.pitches_ok >= base.1,
        "{name} pitch fidelity regressed: {} < {}",
        b.pitches_ok,
        base.1
    );
    assert!(
        b.dur_ok >= base.2,
        "{name} onset+duration fidelity regressed: {} < {}",
        b.dur_ok,
        base.2
    );
}

#[test]
fn fidelity_scoreboard() {
    std::panic::set_hook(Box::new(|_| {}));

    // ----- LY → IR → LY → IR (Music path) -----
    let mut ly = Board::default();
    for path in list("tests/fixtures/ly", &["ly"]) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(before) = safe(|| LyToIrAdapter::new().convert_str(&src).ok()) else {
            ly.total += 1;
            continue;
        };
        let after = safe(|| {
            let doc = LyToIrAdapter::new().convert_str_to_music(&src).ok()?;
            let out = IrToLyAdapter::new().convert_music(&doc).ok()?;
            LyToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut ly, &name, &before, after.as_ref());
    }

    // ----- XML/MXL → IR → XML → IR -----
    let mut xml = Board::default();
    let mut xml_scores: Vec<(String, Score)> = Vec::new();
    let mut xml_fixtures: Vec<(PathBuf, bool)> = Vec::new();
    for p in list("tests/fixtures/xml", &["xml"]) {
        xml_fixtures.push((p, false));
    }
    for p in list("tests/fixtures/mxl", &["mxl"]) {
        xml_fixtures.push((p, true));
    }
    for (path, is_mxl) in &xml_fixtures {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let before = if *is_mxl {
            let p = path.clone();
            safe(|| MxmlToIrAdapter::new().convert_file(&p).ok())
        } else {
            let Ok(src) = std::fs::read_to_string(path) else {
                continue;
            };
            safe(|| MxmlToIrAdapter::new().convert_str(&src).ok())
        };
        let Some(before) = before else {
            xml.total += 1;
            continue;
        };
        let after = safe(|| {
            let out = IrToMxmlAdapter::new().convert(&before).ok()?;
            MxmlToIrAdapter::new().convert_str(&out).ok()
        });
        // Keep the parsed score for the cross-format boards below.
        xml_scores.push((name.clone(), before.clone()));
        record(&mut xml, &name, &before, after.as_ref());
    }

    // ----- XML → IR → ABC → IR, and XML → IR → KRN → IR -----
    let mut xml_abc = Board::default();
    let mut xml_krn = Board::default();
    for (name, before) in &xml_scores {
        let doc = _core::ir::lift::lift_to_music(before);
        let d = doc.clone();
        let after_abc = safe(move || {
            let out = IrToAbcAdapter::new().convert_music(&d).ok()?;
            AbcToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut xml_abc, name, before, after_abc.as_ref());
        let d = doc.clone();
        let after_krn = safe(move || {
            let out = IrToHumdrumAdapter::new().convert_music(&d).ok()?;
            HumdrumToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut xml_krn, name, before, after_krn.as_ref());
    }

    // ----- ABC → IR → ABC → IR -----
    let mut abc = Board::default();
    for path in list("tests/fixtures/abc", &["abc"]) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(before) = safe(|| AbcToIrAdapter::new().convert_str(&src).ok()) else {
            abc.total += 1;
            continue;
        };
        let after = safe(|| {
            let doc = _core::ir::lift::lift_to_music(&before);
            let out = IrToAbcAdapter::new().convert_music(&doc).ok()?;
            AbcToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut abc, &name, &before, after.as_ref());
    }

    // ----- MIDI → IR → MIDI → IR -----
    let mut midi = Board::default();
    for path in list("tests/fixtures/midi", &["mid", "midi"]) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(before) = safe(|| MidiToIrAdapter::new().convert_bytes(&bytes).ok()) else {
            midi.total += 1;
            continue;
        };
        let after = safe(|| {
            let out = IrToMidiAdapter::new().convert_bytes(&before).ok()?;
            MidiToIrAdapter::new().convert_bytes(&out).ok()
        });
        record(&mut midi, &name, &before, after.as_ref());
    }

    // ----- Report -----
    println!("\n===== Semantic fidelity scoreboard =====");
    report(
        "LY  → IR → LY  → IR",
        &ly,
        (LY_NOTES_BASELINE, LY_PITCHES_BASELINE, LY_DUR_BASELINE),
    );
    report(
        "XML → IR → XML → IR",
        &xml,
        (XML_NOTES_BASELINE, XML_PITCHES_BASELINE, XML_DUR_BASELINE),
    );
    report(
        "ABC → IR → ABC → IR",
        &abc,
        (ABC_NOTES_BASELINE, ABC_PITCHES_BASELINE, ABC_DUR_BASELINE),
    );
    report(
        "XML → IR → ABC → IR",
        &xml_abc,
        (
            XML_ABC_NOTES_BASELINE,
            XML_ABC_PITCHES_BASELINE,
            XML_ABC_DUR_BASELINE,
        ),
    );
    report(
        "XML → IR → KRN → IR",
        &xml_krn,
        (
            XML_KRN_NOTES_BASELINE,
            XML_KRN_PITCHES_BASELINE,
            XML_KRN_DUR_BASELINE,
        ),
    );
    report(
        "MIDI→ IR → MIDI→ IR",
        &midi,
        (
            MIDI_NOTES_BASELINE,
            MIDI_PITCHES_BASELINE,
            MIDI_DUR_BASELINE,
        ),
    );
    for (label, b) in [
        ("LY", &ly),
        ("XML", &xml),
        ("ABC", &abc),
        ("XML→ABC", &xml_abc),
        ("XML→KRN", &xml_krn),
        ("MIDI", &midi),
    ] {
        if !b.fails.is_empty() {
            println!("\n{label} content changes (sample):");
            for f in &b.fails {
                println!("  {f}");
            }
        }
    }
    println!("========================================\n");

    // ----- Gate: fidelity must not regress below the committed baseline -----
    gate(
        "LY",
        &ly,
        (LY_NOTES_BASELINE, LY_PITCHES_BASELINE, LY_DUR_BASELINE),
    );
    gate(
        "XML",
        &xml,
        (XML_NOTES_BASELINE, XML_PITCHES_BASELINE, XML_DUR_BASELINE),
    );
    gate(
        "ABC",
        &abc,
        (ABC_NOTES_BASELINE, ABC_PITCHES_BASELINE, ABC_DUR_BASELINE),
    );
    gate(
        "XML→ABC",
        &xml_abc,
        (
            XML_ABC_NOTES_BASELINE,
            XML_ABC_PITCHES_BASELINE,
            XML_ABC_DUR_BASELINE,
        ),
    );
    gate(
        "XML→KRN",
        &xml_krn,
        (
            XML_KRN_NOTES_BASELINE,
            XML_KRN_PITCHES_BASELINE,
            XML_KRN_DUR_BASELINE,
        ),
    );
    gate(
        "MIDI",
        &midi,
        (
            MIDI_NOTES_BASELINE,
            MIDI_PITCHES_BASELINE,
            MIDI_DUR_BASELINE,
        ),
    );
}
