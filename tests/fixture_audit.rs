//! Fixture conversion audit harness.
//!
//! Runs the full convert matrix over every fixture file and reports parse /
//! convert errors and panics per stage. This is the running scoreboard for the
//! v1.0.0 conversion-fidelity work (roadmap Epics A/B/C): unlike
//! `fixture_regression.rs` (which only asserts "non-empty"), this surfaces
//! *every* failure so progress is measurable.
//!
//! Behaviour:
//!   - Reads each fixture into IR (read stage), then emits IR → LilyPond,
//!     IR → MusicXML, IR → MIDI (write stages).
//!   - Conversion `Err`s are tallied but do NOT fail the test (they are the
//!     backlog being worked down; CI gates non-increase separately — ECT3).
//!   - Panics DO fail the test: the converters must never crash.
//!
//! Run with output: `cargo test --test fixture_audit -- --nocapture`

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::score::Score;

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Tally
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    ok: usize,
    err: usize,
    panic: usize,
    /// First few error/panic samples, for the printed report.
    samples: Vec<String>,
}

impl Tally {
    fn record_ok(&mut self) {
        self.ok += 1;
    }
    fn record_err(&mut self, fixture: &str, msg: String) {
        self.err += 1;
        if self.samples.len() < 6 {
            self.samples.push(format!("ERR  {fixture}: {msg}"));
        }
    }
    fn record_panic(&mut self, fixture: &str, msg: String) {
        self.panic += 1;
        // Always keep panic samples (they fail the test).
        self.samples.push(format!("PANIC {fixture}: {msg}"));
    }
    fn total(&self) -> usize {
        self.ok + self.err + self.panic
    }
}

fn panic_msg(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Run a conversion that yields a value; record outcome; return the value on success.
fn run_read(
    tally: &mut Tally,
    fixture: &str,
    parse: impl FnOnce() -> _core::adapters::Result<Score>,
) -> Option<Score> {
    match catch_unwind(AssertUnwindSafe(parse)) {
        Ok(Ok(score)) => {
            tally.record_ok();
            Some(score)
        }
        Ok(Err(e)) => {
            tally.record_err(fixture, e.to_string());
            None
        }
        Err(p) => {
            tally.record_panic(fixture, panic_msg(p));
            None
        }
    }
}

/// Run a write conversion (value discarded); record outcome.
fn run_write<T>(
    tally: &mut Tally,
    fixture: &str,
    emit: impl FnOnce() -> _core::adapters::Result<T>,
) {
    match catch_unwind(AssertUnwindSafe(emit)) {
        Ok(Ok(_)) => tally.record_ok(),
        Ok(Err(e)) => tally.record_err(fixture, e.to_string()),
        Err(p) => tally.record_panic(fixture, panic_msg(p)),
    }
}

// ---------------------------------------------------------------------------
// Fixture discovery
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum InputKind {
    LyText,
    XmlText,
    MxlFile,
    MidiBytes,
}

fn collect(dir: &str, exts: &[&str], kind: InputKind, out: &mut Vec<(InputKind, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| exts.contains(&e))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for p in paths {
        out.push((kind, p));
    }
}

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

#[test]
fn fixture_conversion_audit() {
    // Silence panic noise; we capture and report panics ourselves.
    std::panic::set_hook(Box::new(|_| {}));

    let mut fixtures: Vec<(InputKind, PathBuf)> = Vec::new();
    collect(
        "tests/fixtures/ly",
        &["ly"],
        InputKind::LyText,
        &mut fixtures,
    );
    collect(
        "tests/fixtures/xml",
        &["xml"],
        InputKind::XmlText,
        &mut fixtures,
    );
    collect(
        "tests/fixtures/musicxml",
        &["xml", "musicxml"],
        InputKind::XmlText,
        &mut fixtures,
    );
    collect(
        "tests/fixtures/mxl",
        &["mxl"],
        InputKind::MxlFile,
        &mut fixtures,
    );
    collect(
        "tests/fixtures/midi",
        &["mid", "midi"],
        InputKind::MidiBytes,
        &mut fixtures,
    );
    collect(
        "tests/fixtures/ly",
        &["mid", "midi"],
        InputKind::MidiBytes,
        &mut fixtures,
    );

    // Stages, ordered for the report.
    let mut stages: BTreeMap<&'static str, Tally> = BTreeMap::new();

    for (kind, path) in &fixtures {
        let fixture = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<?>")
            .to_string();

        let (read_stage, score) = read_fixture(*kind, path, &fixture, &mut stages);
        let Some(score) = score else { continue };
        // Avoid an unused warning path; read_stage already recorded above.
        let _ = read_stage;

        run_write(
            stages.entry("write: ir -> ly").or_default(),
            &fixture,
            || IrToLyAdapter::new().convert(&score),
        );
        run_write(
            stages.entry("write: ir -> xml").or_default(),
            &fixture,
            || IrToMxmlAdapter::new().convert(&score),
        );
        run_write(
            stages.entry("write: ir -> midi").or_default(),
            &fixture,
            || IrToMidiAdapter::new().convert_bytes(&score),
        );
    }

    // ----- Report -----
    println!(
        "\n===== Fixture conversion audit ({} fixtures) =====",
        fixtures.len()
    );
    println!(
        "{:<22} {:>6} {:>6} {:>6} {:>6}",
        "stage", "total", "ok", "err", "panic"
    );
    let mut total_panics = 0usize;
    for (stage, t) in &stages {
        total_panics += t.panic;
        println!(
            "{:<22} {:>6} {:>6} {:>6} {:>6}",
            stage,
            t.total(),
            t.ok,
            t.err,
            t.panic
        );
    }
    println!("\n----- samples (first errors / all panics per stage) -----");
    for (stage, t) in &stages {
        if t.samples.is_empty() {
            continue;
        }
        println!("[{stage}]");
        for s in &t.samples {
            println!("  {s}");
        }
    }
    println!("=================================================\n");

    assert_eq!(
        total_panics, 0,
        "converters panicked on some fixtures (see audit report above)"
    );
}

fn read_fixture(
    kind: InputKind,
    path: &Path,
    fixture: &str,
    stages: &mut BTreeMap<&'static str, Tally>,
) -> (&'static str, Option<Score>) {
    match kind {
        InputKind::LyText => {
            let stage = "read: ly";
            let score = match std::fs::read_to_string(path) {
                Ok(text) => run_read(stages.entry(stage).or_default(), fixture, || {
                    LyToIrAdapter::new().convert_str(&text)
                }),
                Err(e) => {
                    stages
                        .entry(stage)
                        .or_default()
                        .record_err(fixture, format!("read file: {e}"));
                    None
                }
            };
            (stage, score)
        }
        InputKind::XmlText => {
            let stage = "read: xml";
            let score = match std::fs::read_to_string(path) {
                Ok(text) => run_read(stages.entry(stage).or_default(), fixture, || {
                    MxmlToIrAdapter::new().convert_str(&text)
                }),
                Err(e) => {
                    stages
                        .entry(stage)
                        .or_default()
                        .record_err(fixture, format!("read file: {e}"));
                    None
                }
            };
            (stage, score)
        }
        InputKind::MxlFile => {
            let stage = "read: mxl";
            let p = path.to_path_buf();
            let score = run_read(stages.entry(stage).or_default(), fixture, move || {
                MxmlToIrAdapter::new().convert_file(&p)
            });
            (stage, score)
        }
        InputKind::MidiBytes => {
            let stage = "read: midi";
            let score = match std::fs::read(path) {
                Ok(bytes) => run_read(stages.entry(stage).or_default(), fixture, || {
                    MidiToIrAdapter::new().convert_bytes(&bytes)
                }),
                Err(e) => {
                    stages
                        .entry(stage)
                        .or_default()
                        .record_err(fixture, format!("read file: {e}"));
                    None
                }
            };
            (stage, score)
        }
    }
}
