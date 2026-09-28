//! The source-level LilyPond API (Epic K): `\version` (K1), tokens (K3).

mod common;

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ly_to_ir::{check, LilyPondVersion, LyToIrAdapter};
use _core::adapters::{FromIrAdapter, ToIrAdapter};

#[test]
fn every_movement_carries_the_files_version() {
    let src = "\\version \"2.24\"\n\\score { { c'1 } }\n{ d'1 }\n";
    let scores = LyToIrAdapter::new().convert_str_multi(src).unwrap();
    assert_eq!(scores.len(), 2);
    for score in &scores {
        assert_eq!(score.metadata.lilypond_version.as_deref(), Some("2.24.0"));
    }
    // An invalid one is an error, and no version.
    let bad = "\\version \"2.x\"\n{ c'1 }";
    let score = LyToIrAdapter::new().convert_str(bad).unwrap();
    assert_eq!(score.metadata.lilypond_version, None);
    let d = check(bad, false).remove(0);
    assert_eq!((d.code, d.line, d.column), ("invalid-version", 1, 10));
}

#[test]
fn the_writer_writes_its_own_version_unless_told() {
    let score = LyToIrAdapter::new()
        .convert_str("\\version \"2.18.2\"\n{ c'1 }")
        .unwrap();
    let out = IrToLyAdapter::new().convert(&score).unwrap();
    assert!(out.contains("\\version \"2.24.0\""), "{out}");
    let v: LilyPondVersion = "2.26".parse().unwrap();
    let out = IrToLyAdapter::new()
        .with_version(&v.to_string())
        .convert(&score)
        .unwrap();
    assert!(out.contains("\\version \"2.26.0\""), "{out}");
}

#[test]
fn tokens_cover_every_character_of_the_fixtures() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ly");
    let mut files = 0;
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let src = std::fs::read_to_string(entry.path()).unwrap();
        assert_eq!(common::token_gap(&src), None, "{}", entry.path().display());
        files += 1;
    }
    assert!(files >= 30);
    // Broken input too: what the grammar cannot tokenize is an error token.
    for src in [
        "}}} INVALID {{{",
        "{ c'4 d'4",
        "\\score { >> }",
        "#(a (b",
        "é ü ☃ ?!",
    ] {
        assert_eq!(common::token_gap(src), None, "{src:?}");
    }
}
