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
        .filter(|p| p.extension().map_or(false, |e| e == "ly"))
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
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "ly")
        })
        .collect();
    assert!(
        !ly_files.is_empty(),
        "batch convert should produce at least one .ly file"
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
