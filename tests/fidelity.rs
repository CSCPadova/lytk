//! Semantic-fidelity scoreboard (Epic C / ECT3).
//!
//! Round-trips every fixture and measures whether *musical content* survives —
//! note count and the pitch multiset — per conversion direction. Prints a
//! report and gates on a committed baseline so fidelity can only improve, never
//! regress (the non-decreasing-fidelity gate).
//!
//! Directions measured:
//!   - LY → IR → LY (Music path, the CLI route) → IR
//!   - XML/MXL → IR → XML → IR
//!
//! Run with output: `cargo test --test fidelity -- --nocapture`

mod common;

use common::{pitch_multiset, signature};

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
use _core::ir::score::Score;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Committed baselines (the non-decreasing-fidelity gate).
// Bump these UP when fidelity improves; never down.
// ---------------------------------------------------------------------------
const LY_NOTES_BASELINE: usize = 33;
const LY_PITCHES_BASELINE: usize = 33;
const XML_NOTES_BASELINE: usize = 152;
const XML_PITCHES_BASELINE: usize = 152;

#[derive(Default)]
struct Score2 {
    total: usize,
    notes_ok: usize,
    pitches_ok: usize,
    /// fixtures whose pitch multiset changed (for the report)
    pitch_fails: Vec<String>,
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

/// Round-trip a score back to a fresh Score via `f`, returning None on panic/err.
fn safe<F: FnOnce() -> Option<Score>>(f: F) -> Option<Score> {
    catch_unwind(AssertUnwindSafe(f)).ok().flatten()
}

fn record(board: &mut Score2, fixture: &str, before: &Score, after: Option<&Score>) {
    board.total += 1;
    let Some(after) = after else {
        board
            .pitch_fails
            .push(format!("{fixture} (round-trip failed)"));
        return;
    };
    let sb = signature(before);
    let sa = signature(after);
    if sb.notes == sa.notes {
        board.notes_ok += 1;
    }
    if pitch_multiset(before) == pitch_multiset(after) {
        board.pitches_ok += 1;
    } else if board.pitch_fails.len() < 12 {
        board
            .pitch_fails
            .push(format!("{fixture} ({} → {} notes)", sb.notes, sa.notes));
    }
}

#[test]
fn fidelity_scoreboard() {
    std::panic::set_hook(Box::new(|_| {}));

    let mut ly = Score2::default();
    for path in list("tests/fixtures/ly", &["ly"]) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(before) = safe(|| LyToIrAdapter::new().convert_str(&src).ok()) else {
            ly.total += 1;
            continue;
        };
        // Round-trip via the Music path (the CLI route), then re-parse.
        let after = safe(|| {
            let doc = LyToIrAdapter::new().convert_str_to_music(&src).ok()?;
            let out = IrToLyAdapter::new().convert_music(&doc).ok()?;
            LyToIrAdapter::new().convert_str(&out).ok()
        });
        record(&mut ly, &name, &before, after.as_ref());
    }

    let mut xml = Score2::default();
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
        record(&mut xml, &name, &before, after.as_ref());
    }

    // ----- Report -----
    println!("\n===== Semantic fidelity scoreboard =====");
    println!(
        "LY  → IR → LY  → IR : {}/{} note-count, {}/{} pitch-multiset (baseline {}/{})",
        ly.notes_ok, ly.total, ly.pitches_ok, ly.total, LY_NOTES_BASELINE, LY_PITCHES_BASELINE
    );
    println!(
        "XML → IR → XML → IR : {}/{} note-count, {}/{} pitch-multiset (baseline {}/{})",
        xml.notes_ok,
        xml.total,
        xml.pitches_ok,
        xml.total,
        XML_NOTES_BASELINE,
        XML_PITCHES_BASELINE
    );
    if !ly.pitch_fails.is_empty() {
        println!("\nLY pitch-multiset changes (sample):");
        for f in &ly.pitch_fails {
            println!("  {f}");
        }
    }
    if !xml.pitch_fails.is_empty() {
        println!("\nXML pitch-multiset changes (sample):");
        for f in &xml.pitch_fails {
            println!("  {f}");
        }
    }
    println!("========================================\n");

    // ----- Gate: fidelity must not regress below the committed baseline -----
    assert!(
        ly.notes_ok >= LY_NOTES_BASELINE,
        "LY note-count fidelity regressed: {} < {}",
        ly.notes_ok,
        LY_NOTES_BASELINE
    );
    assert!(
        ly.pitches_ok >= LY_PITCHES_BASELINE,
        "LY pitch fidelity regressed: {} < {}",
        ly.pitches_ok,
        LY_PITCHES_BASELINE
    );
    assert!(
        xml.notes_ok >= XML_NOTES_BASELINE,
        "XML note-count fidelity regressed: {} < {}",
        xml.notes_ok,
        XML_NOTES_BASELINE
    );
    assert!(
        xml.pitches_ok >= XML_PITCHES_BASELINE,
        "XML pitch fidelity regressed: {} < {}",
        xml.pitches_ok,
        XML_PITCHES_BASELINE
    );
}
