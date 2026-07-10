//! Integration tests for the `lytk` CLI binary.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Helper to get a `Command` for the `lytk` binary.
fn lytk() -> Command {
    Command::cargo_bin("lytk").expect("binary `lytk` not found")
}

// ---------------------------------------------------------------------------
// Basic invocation
// ---------------------------------------------------------------------------

#[test]
fn cli_no_args_shows_help_and_fails() {
    lytk()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn cli_help_flag() {
    lytk()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("music notation"));
}

#[test]
fn cli_version_flag() {
    lytk()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("lytk"));
}

// ---------------------------------------------------------------------------
// `info` subcommand
// ---------------------------------------------------------------------------

#[test]
fn info_xml_file() {
    lytk()
        .args(["info", "tests/fixtures/xml/01a-Pitches-Pitches.xml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parts:"));
}

#[test]
fn info_missing_file() {
    lytk()
        .args(["info", "nonexistent.xml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn info_unsupported_format() {
    let tmp = TempDir::new().unwrap();
    let bad_file = tmp.path().join("score.txt");
    fs::write(&bad_file, "not music").unwrap();

    lytk()
        .args(["info", bad_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsupported"));
}

// ---------------------------------------------------------------------------
// `convert` subcommand — single-file
// ---------------------------------------------------------------------------

#[test]
fn convert_xml_to_ly() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.ly");

    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.exists(), "output file should be created");
    let content = fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("\\version"),
        "LilyPond output should contain \\version"
    );
}

#[test]
fn convert_ly_to_xml() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.xml");

    // Pick a LilyPond fixture.
    let ly_fixtures: Vec<_> = fs::read_dir("tests/fixtures/ly")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ly"))
        .collect();
    assert!(!ly_fixtures.is_empty(), "need at least one .ly fixture");
    let ly_input = &ly_fixtures[0];

    lytk()
        .args([
            "convert",
            ly_input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.exists(), "output file should be created");
    let content = fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("<score-partwise"),
        "MusicXML output should contain <score-partwise"
    );
}

#[test]
fn convert_with_explicit_format() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.dat"); // unusual extension

    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
            "--format",
            "ly",
        ])
        .assert()
        .success();

    assert!(out.exists());
    let content = fs::read_to_string(&out).unwrap();
    assert!(content.contains("\\version"));
}

#[test]
fn convert_missing_input() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.ly");

    lytk()
        .args(["convert", "nonexistent.xml", "-o", out.to_str().unwrap()])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// `convert` subcommand — batch (directory → directory)
// ---------------------------------------------------------------------------

#[test]
fn convert_batch_directory() {
    let tmp = TempDir::new().unwrap();
    let out_dir = tmp.path().join("out");

    lytk()
        .args([
            "convert",
            "tests/fixtures/xml",
            "-o",
            out_dir.to_str().unwrap(),
            "--jobs",
            "1",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Processed"));

    // At least some .ly files should have been created.
    let ly_files: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "ly"))
        .collect();
    assert!(
        !ly_files.is_empty(),
        "batch convert should produce at least one .ly file"
    );
}

#[test]
fn convert_batch_partial_failure_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    let in_dir = tmp.path().join("in");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&in_dir).unwrap();

    // One valid fixture + one malformed file in the same batch.
    fs::copy(
        "tests/fixtures/xml/01a-Pitches-Pitches.xml",
        in_dir.join("good.xml"),
    )
    .unwrap();
    fs::write(in_dir.join("bad.xml"), "this is not valid musicxml").unwrap();

    lytk()
        .args([
            "convert",
            in_dir.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
            "--jobs",
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Processed 2 files"))
        .stderr(predicate::str::contains("failed to convert"));

    // The valid file is still converted even though the batch reports failure.
    assert!(
        out_dir.join("good.ly").exists(),
        "the valid file should still be converted"
    );
}

// ---------------------------------------------------------------------------
// `transpose` subcommand
// ---------------------------------------------------------------------------

#[test]
fn transpose_xml_file() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("transposed.ly");

    lytk()
        .args([
            "transpose",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
            "--semitones",
            "3",
        ])
        .assert()
        .success();

    assert!(out.exists());
    let content = fs::read_to_string(&out).unwrap();
    assert!(content.contains("\\version"));
}

#[test]
fn transpose_negative_semitones() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("transposed.ly");

    lytk()
        .args([
            "transpose",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
            "--semitones",
            "-5",
        ])
        .assert()
        .success();

    assert!(out.exists());
}

#[test]
fn transpose_zero_is_identity() {
    let tmp = TempDir::new().unwrap();
    let original_out = tmp.path().join("original.ly");
    let transposed_out = tmp.path().join("transposed.ly");

    // Convert without transpose.
    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            original_out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Transpose by 0 semitones.
    lytk()
        .args([
            "transpose",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            transposed_out.to_str().unwrap(),
            "--semitones",
            "0",
        ])
        .assert()
        .success();

    let original = fs::read_to_string(&original_out).unwrap();
    let transposed = fs::read_to_string(&transposed_out).unwrap();
    assert_eq!(original, transposed, "transpose(0) should be identity");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn convert_unknown_output_format_fails() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.pdf"); // unsupported

    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot infer output format"));
}

// ---------------------------------------------------------------------------
// MIDI CLI tests
// ---------------------------------------------------------------------------

#[test]
fn convert_xml_to_midi() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.mid");

    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let bytes = fs::read(&out).unwrap();
    assert_eq!(&bytes[0..4], b"MThd", "output should be a valid MIDI file");
}

#[test]
fn convert_midi_to_ly() {
    let tmp = TempDir::new().unwrap();
    let mid_out = tmp.path().join("test.mid");
    let ly_out = tmp.path().join("output.ly");

    // First create a MIDI file from XML
    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            mid_out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Then convert MIDI back to LilyPond
    lytk()
        .args([
            "convert",
            mid_out.to_str().unwrap(),
            "-o",
            ly_out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let ly = fs::read_to_string(&ly_out).unwrap();
    assert!(
        ly.contains("\\version"),
        "LilyPond output should contain version"
    );
}

#[test]
fn convert_midi_to_xml() {
    let tmp = TempDir::new().unwrap();
    let mid_out = tmp.path().join("test.mid");
    let xml_out = tmp.path().join("output.xml");

    // First create a MIDI file
    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            mid_out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // MIDI → MusicXML
    lytk()
        .args([
            "convert",
            mid_out.to_str().unwrap(),
            "-o",
            xml_out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let xml = fs::read_to_string(&xml_out).unwrap();
    assert!(
        xml.contains("score-partwise"),
        "should produce valid MusicXML"
    );
}

#[test]
fn info_midi_file() {
    let tmp = TempDir::new().unwrap();
    let mid_out = tmp.path().join("test.mid");

    // Create a MIDI file
    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            mid_out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Run info on it
    lytk()
        .args(["info", mid_out.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parts:"));
}

#[test]
fn convert_abc_to_ly() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.ly");

    lytk()
        .args([
            "convert",
            "tests/fixtures/abc/simple.abc",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&out).unwrap();
    assert!(content.contains("\\new Staff"), "ABC→LY missing staff");
}

#[test]
fn convert_ly_to_abc() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("output.abc");

    lytk()
        .args([
            "convert",
            "tests/fixtures/abc/simple.abc",
            "-o",
            out.to_str().unwrap(),
            "-f",
            "ly",
        ])
        .assert()
        .success();
    // And the reverse: any LilyPond fixture → ABC.
    let abc_out = tmp.path().join("from_ly.abc");
    lytk()
        .args([
            "convert",
            "tests/fixtures/abc/simple.abc",
            "-o",
            abc_out.to_str().unwrap(),
            "-f",
            "abc",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&abc_out).unwrap();
    assert!(content.contains("K:"), "ABC output missing key header");
}

// ---------------------------------------------------------------------------
// stdin/stdout streaming + transform subcommands (Epic P1)
// ---------------------------------------------------------------------------

/// A single-staff, single-voice absolute-entry snippet — relative threading is
/// reliable for it, so `abs2rel` can emit a `\relative` wrapper.
const SIMPLE_LY: &str = "\\version \"2.24.0\"\n{ c'4 e'4 g'4 }\n";

#[test]
fn convert_stdin_to_stdout() {
    lytk()
        .args(["convert", "-", "-o", "-", "--from", "ly", "-f", "xml"])
        .write_stdin(SIMPLE_LY)
        .assert()
        .success()
        .stdout(predicate::str::contains("score-partwise"));
}

#[test]
fn convert_stdin_without_from_fails() {
    lytk()
        .args(["convert", "-", "-o", "out.xml"])
        .write_stdin(SIMPLE_LY)
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires --from"));
}

#[test]
fn convert_stdout_without_format_fails() {
    lytk()
        .args([
            "convert",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            "-",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires --format"));
}

#[test]
fn invert_subcommand_writes_output() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "invert",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--axis",
            "c4",
        ])
        .assert()
        .success();
    assert!(out.exists(), "invert should write the output file");
}

#[test]
fn invert_bad_axis_fails() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    lytk()
        .args([
            "invert",
            inp.to_str().unwrap(),
            "-o",
            "-",
            "-f",
            "ly",
            "--axis",
            "zzz",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("axis"));
}

#[test]
fn retrograde_subcommand_writes_output() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "retrograde",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(out.exists(), "retrograde should write the output file");
}

#[test]
fn change_language_subcommand() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "change-language",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "-l",
            "italiano",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("italiano"),
        "expected \\language \"italiano\" in output"
    );
}

#[test]
fn change_language_unknown_fails() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    lytk()
        .args([
            "change-language",
            inp.to_str().unwrap(),
            "-o",
            "-",
            "-f",
            "ly",
            "-l",
            "klingon",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown pitch language"));
}

#[test]
fn abs2rel_emits_relative() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap(); // absolute entry
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "abs2rel",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("\\relative"),
        "abs2rel output should use \\relative, got:\n{content}"
    );
}

#[test]
fn rel2abs_drops_relative() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, "\\version \"2.24.0\"\n\\relative c' { c4 d4 e4 }\n").unwrap();
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "rel2abs",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&out).unwrap();
    assert!(
        !content.contains("\\relative"),
        "rel2abs output should not wrap in \\relative, got:\n{content}"
    );
}

#[test]
fn abs2rel_rejects_non_lilypond() {
    lytk()
        .args([
            "abs2rel",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            "-",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("LilyPond"));
}

#[test]
fn transpose_by_interval() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "transpose",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--interval",
            "M3",
        ])
        .assert()
        .success();
    assert!(out.exists());
}

#[test]
fn transpose_to_key_subcommand() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    let out = tmp.path().join("out.ly");
    lytk()
        .args([
            "transpose",
            inp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--to-key",
            "D",
        ])
        .assert()
        .success();
    assert!(out.exists());
}

#[test]
fn transpose_requires_exactly_one_mode() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    // No mode given.
    lytk()
        .args(["transpose", inp.to_str().unwrap(), "-o", "-", "-f", "ly"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("exactly one"));
    // Two modes given.
    lytk()
        .args([
            "transpose",
            inp.to_str().unwrap(),
            "-o",
            "-",
            "-f",
            "ly",
            "-s",
            "2",
            "--interval",
            "M3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("exactly one"));
}

#[test]
fn transpose_bad_interval_fails() {
    let tmp = TempDir::new().unwrap();
    let inp = tmp.path().join("in.ly");
    fs::write(&inp, SIMPLE_LY).unwrap();
    lytk()
        .args([
            "transpose",
            inp.to_str().unwrap(),
            "-o",
            "-",
            "-f",
            "ly",
            "--interval",
            "Q9",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Automation outputs (Epic P10): info --json, diff
// ---------------------------------------------------------------------------

#[test]
fn info_json_output() {
    lytk()
        .args([
            "info",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"part_count\""))
        .stdout(predicate::str::contains("\"note_count\""))
        .stdout(predicate::str::contains("\"parts\""));
}

#[test]
fn diff_equal_exits_zero() {
    let f = "tests/fixtures/xml/01a-Pitches-Pitches.xml";
    lytk()
        .args(["diff", f, f])
        .assert()
        .success()
        .stdout(predicate::str::contains("semantically equal"));
}

#[test]
fn diff_differ_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    let f = "tests/fixtures/xml/01a-Pitches-Pitches.xml";
    let transposed = tmp.path().join("t.xml");
    lytk()
        .args([
            "transpose",
            f,
            "-o",
            transposed.to_str().unwrap(),
            "-s",
            "2",
        ])
        .assert()
        .success();
    lytk()
        .args(["diff", f, transposed.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"equal\": false"));
}

// ---------------------------------------------------------------------------
// JSON batch-job API (Epic P11)
// ---------------------------------------------------------------------------

#[test]
fn batch_jobs_runs() {
    let tmp = TempDir::new().unwrap();
    let out1 = tmp.path().join("a.ly");
    let out2 = tmp.path().join("b.ly");
    let jobs = tmp.path().join("jobs.json");
    let spec = format!(
        r#"[
            {{"in":"tests/fixtures/xml/01a-Pitches-Pitches.xml","out":"{}","format":"ly"}},
            {{"in":"tests/fixtures/xml/01a-Pitches-Pitches.xml","out":"{}","format":"ly","interval":"M3"}}
        ]"#,
        out1.display(),
        out2.display()
    );
    fs::write(&jobs, spec).unwrap();
    lytk()
        .args(["batch", jobs.to_str().unwrap(), "-j", "1"])
        .assert()
        .success();
    assert!(out1.exists() && out2.exists());
}

#[test]
fn batch_partial_failure_exits_nonzero_and_reports() {
    let tmp = TempDir::new().unwrap();
    let good = tmp.path().join("good.ly");
    let bad = tmp.path().join("bad.ly");
    let report = tmp.path().join("report.json");
    let jobs = tmp.path().join("jobs.json");
    let spec = format!(
        r#"[
            {{"in":"tests/fixtures/xml/01a-Pitches-Pitches.xml","out":"{}","format":"ly"}},
            {{"in":"nonexistent.xml","out":"{}","format":"ly"}}
        ]"#,
        good.display(),
        bad.display()
    );
    fs::write(&jobs, spec).unwrap();
    lytk()
        .args([
            "batch",
            jobs.to_str().unwrap(),
            "-j",
            "1",
            "--report",
            report.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed"));
    // The valid job still produced its output.
    assert!(good.exists());
    let report_text = fs::read_to_string(&report).unwrap();
    assert!(report_text.contains("\"ok\": false"));
    assert!(report_text.contains("\"ok\": true"));
}

#[test]
fn positions_json_output() {
    lytk()
        .args(["positions", "tests/fixtures/xml/01a-Pitches-Pitches.xml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"measures\""))
        .stdout(predicate::str::contains("\"start\""))
        .stdout(predicate::str::contains("\"unit\": \"quarter\""));
}

#[test]
fn bundle_exports_parts() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("parts");
    lytk()
        .args([
            "bundle",
            "tests/fixtures/xml/01a-Pitches-Pitches.xml",
            "-o",
            out.to_str().unwrap(),
            "-f",
            "ly",
        ])
        .assert()
        .success();
    let files: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ly"))
        .collect();
    assert!(
        !files.is_empty(),
        "bundle should write at least one part file"
    );
}

/// Regression (review R6): converting a multi-`\score` input must create the
/// requested output path (first movement), not only `_01`/`_02` siblings —
/// scripted pipelines read the path they asked for.
#[test]
fn multi_score_input_writes_requested_path() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("two.ly");
    std::fs::write(
        &src,
        r#"\score { \new Staff { c'1 } } \score { \new Staff { d'1 } }"#,
    )
    .unwrap();
    let out = dir.path().join("out.xml");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_lytk"))
        .args(["convert"])
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        out.exists(),
        "requested path must contain the first movement"
    );
    assert!(
        dir.path().join("out_02.xml").exists(),
        "second movement goes to the _02 sibling"
    );
}
