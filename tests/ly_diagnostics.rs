//! Diagnostics of the LilyPond reader (Epic J3): each code on the input that
//! raises it, and no false alarm on the constructs it must leave alone.

use _core::adapters::ly_to_ir::{check, read_source, LyToIrAdapter};
use _core::adapters::AdapterError;
use _core::diagnostics::{Diagnostic, Severity};

fn codes(text: &str) -> Vec<(Severity, &'static str)> {
    check(text, true)
        .iter()
        .map(|d| (d.severity, d.code))
        .collect()
}

#[test]
fn each_code_on_its_input() {
    use Severity::{Error, Warning};
    let cases: &[(&str, Severity, &str)] = &[
        // Syntax, from the tree.
        ("{ c'4 d'4", Error, "missing-token"),
        ("\\score { \\new Staff { c'1 } >> }", Error, "syntax-error"),
        ("#set-global-staff-size 20)\n{ c'1 }", Error, "syntax-error"),
        (
            "{ \\override Beam.positions = #'1 . 2) c'1 }",
            Error,
            "syntax-error",
        ),
        (
            "\\markup { \\override #'box-padding \"x\" }",
            Error,
            "syntax-error",
        ),
        // Semantic errors, from the walk.
        ("{ c'3 }", Error, "invalid-duration"),
        ("{ \\tuplet 0/2 { c'8 d' e' } }", Error, "invalid-ratio"),
        ("{ c'4*1/0 }", Error, "invalid-ratio"),
        ("Hello world, this is not music.", Error, "not-lilypond"),
        // Warnings: what the reader does not read.
        ("{ c'2048 }", Warning, "unsupported-value"),
        ("{ \\time 3/256 c'4 }", Warning, "unsupported-value"),
        ("{ \\time 1/0 c'1 }", Warning, "unsupported-value"),
        ("{ \\tuplet 300/2 { c'8 } }", Warning, "unsupported-value"),
        (
            "\\include \"notes.ily\"\n{ c'1 }",
            Warning,
            "ignored-include",
        ),
        ("{ c'4 \\noSuchCommand d'4 }", Warning, "unknown-command"),
        (
            "\\language \"klingon\"\n{ c'1 }",
            Warning,
            "unknown-language",
        ),
        (
            "\\score { { c'1 } \\midi { } }\n\\score { { d'1 } \\layout { } }",
            Warning,
            "skipped-score",
        ),
        ("c'4 d' e'", Error, "syntax-error"),
        ("\\drums { bd4 xyz }", Warning, "unrecognized-token"),
    ];
    for &(text, severity, code) in cases {
        let found = codes(text);
        assert!(
            found.contains(&(severity, code)),
            "{text:?}: expected {} [{code}], got {found:?}",
            severity.as_str()
        );
    }
}

#[test]
fn valid_lilypond_raises_nothing() {
    let cases = [
        "\\version \"2.24.0\"\n\\header { title = \"T\" }\n\\relative c' { c4( d) e-> f\\p }",
        // A post-event function's parenthesis, and a tweak's.
        "{ c1 \\vshape #'((0 . 0) (0 . 1)) ( d1) e-\\tweak color #red ( f) }",
        // Markup is text: marks and words after Scheme values are fine.
        "\\markup { \\hspace #-1.0 . \\teeny .org }",
        // Commands the file defines, in Scheme or as quoted identifiers.
        "#(define (twice m) m)\nfoo = #(define-music-function (m) (ly:music?) m)\n\
         \"\\\\|\" = \\bar \"|\"\n{ \\foo c'1 \\| }",
        // LilyPond reads a byte-order mark anywhere as whitespace.
        "{ c'4 \u{feff}d'4 }",
        "violin.1 = { c'1 }",
        "\\layout { \\context { \\Score \\override SpacingSpanner.x = #1 } }\n{ c'1 }",
    ];
    for text in cases {
        let found = check(text, true);
        assert!(found.is_empty(), "{text:?}: {found:?}");
    }
}

#[test]
fn markup_words_are_text_not_notes() {
    // LilyPond reads `a8` here as the markup's word: its bar check fails.
    let reader = LyToIrAdapter::new();
    let reading = reader
        .read_str("{ \\time 2/4 c'4 -\\markup \\bold a8 d'4 \\markup { e f } }")
        .unwrap();
    let notes: usize = reading.scores[0].parts()[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .map(|v| v.elements.len())
        .sum();
    assert_eq!(notes, 2, "{:?}", reading.scores[0]);
}

#[test]
fn a_single_score_reading_warns_about_the_movements_it_drops() {
    let reading = LyToIrAdapter::new()
        .read_str("\\score { { c'1 } }\n\\score { { d'1 } }\n\\score { { e'1 } }")
        .unwrap();
    assert_eq!(reading.scores.len(), 2 + 1);
    assert!(reading.diagnostics.is_empty(), "{:?}", reading.diagnostics);
    let (_, diagnostics) = reading.into_first();
    let lines: Vec<usize> = diagnostics
        .iter()
        .filter(|d| d.code == "dropped-music")
        .map(|d| d.line)
        .collect();
    assert_eq!(lines, [2, 3]);
}

#[test]
fn position_and_display() {
    // The column counts characters, not bytes; the range is in bytes.
    let text = "{ é c'4 >> }";
    let d = check(text, false).remove(0);
    assert_eq!((d.line, d.column), (1, 9));
    assert_eq!(&text[d.start..d.end], ">>");
    assert_eq!(
        d.to_string(),
        "1:9: error: `>>` without a matching `<<` [syntax-error]"
    );
    // A brace left open at the end is reported at the brace.
    let d = check("\\version \"2.24.0\"\n{ c'4 d'4", false).remove(0);
    assert_eq!((d.code, d.line, d.column), ("missing-token", 2, 1));
    assert!(d.message.starts_with("missing `}`"), "{d}");
}

#[test]
fn input_past_the_bounds_is_a_diagnostic_when_checked() {
    let deep = format!("{}{}", "{ ".repeat(3000), " }".repeat(3000));
    for semantic in [false, true] {
        let found = check(&deep, semantic);
        assert!(
            found
                .iter()
                .any(|d: &Diagnostic| d.code == "too-large" && d.is_error()),
            "{found:?}"
        );
    }
}

#[test]
fn a_file_that_is_not_utf8_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin1.ly");
    std::fs::write(&path, b"{ c'4 \xe9 }").unwrap();
    assert!(matches!(read_source(&path), Err(AdapterError::Parse(_))));
    assert!(matches!(
        read_source(&dir.path().join("missing.ly")),
        Err(AdapterError::Io(_))
    ));
}
