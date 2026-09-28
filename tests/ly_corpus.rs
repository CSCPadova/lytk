//! LilyPond's diagnostics against corpora: how often valid LilyPond is
//! reported as wrong, and how often broken LilyPond is caught (Epic J).
//!
//! - `mutation_board` breaks the committed `.ly` fixtures one token at a time
//!   (a brace, a quote, a `<<`, a `>>`, a chord's `>`, a Scheme parenthesis)
//!   and counts the broken files with an error: from the syntax check alone
//!   (`check(text, false)`) and from a reading (`check(text, true)`). It runs
//!   in every `cargo test`.
//! - `syntax_board` checks the syntax of LilyPond's own regression tests and
//!   documentation snippets, all valid LilyPond, and counts the files with an
//!   error; `reader_board` reads them all, counts the files with an error and
//!   the ones refused as too large (its bounds must not catch real music), and
//!   tallies the warnings per code. Both need LilyPond's sources, so they are
//!   ignored by default:
//!   `LYTK_LILYPOND_SRC=/path/to/lilypond cargo test --test ly_corpus -- --ignored --nocapture`
//!   (`LYTK_LILYPOND_SRC` defaults to the `lilypond/` reference checkout; CI
//!   sparse-clones v2.26.0).

use _core::adapters::ly_to_ir::{check, header_fields, LyToIrAdapter};
use _core::parser::LilyPondParser;
use tree_sitter::Tree;

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

// Files of LilyPond v2.26.0's `input/regression` (recursive) and
// `Documentation/snippets` with a syntax error (2026-09-27):
// `other/display-lily-tests.ly` (`##[ #]`, which the grammar does not know).
// It may only fall.
const SYNTAX_ERROR_FILES: usize = 1;
/// Valid files the reader reports an error in (syntax or semantic): the same
/// one. It may only fall.
const READER_ERROR_FILES: usize = 1;
/// Files named per warning code in the reader board's output.
const WARNING_SAMPLES: usize = 5;
/// The corpus must be at least this large, so a wrong path fails loudly
/// instead of passing on an empty directory.
const SYNTAX_MIN_FILES: usize = 2500;

// Broken fixtures caught per kind of deleted token, by the syntax check and by
// a reading, out of the mutations made (2026-09-27: 100, 100, 46, 46, 21, 105,
// 42, 42). They may only rise. The three Scheme `(` missed leave valid
// LilyPond: `#'(4 4)` without its `(` is `#'4` and the note `4)`.
const MUTATIONS_CAUGHT: &[(&str, usize, usize)] = &[
    ("open brace `{`", 100, 100),
    ("close brace `}`", 100, 100),
    ("open `<<`", 46, 46),
    ("close `>>`", 46, 46),
    ("chord `>`", 21, 21),
    ("string quote", 105, 105),
    ("Scheme `(`", 39, 39),
    ("Scheme `)`", 42, 42),
];

/// LilyPond's own tests of its errors (`expect-error = ##t`): LilyPond fails
/// them on purpose, so they are no valid files. The boards count them apart.
fn expects_error(src: &str) -> bool {
    src.contains("expect-error = ##t")
}

fn read_lossy(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    String::from_utf8_lossy(&bytes).into_owned()
}

fn ly_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                out.extend(ly_files(&path, true));
            }
        } else if path.extension().is_some_and(|e| e == "ly") {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn corpus_root() -> Option<PathBuf> {
    let root = std::env::var_os("LYTK_LILYPOND_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("lilypond"));
    root.join("input/regression").is_dir().then_some(root)
}

#[test]
#[ignore = "needs LilyPond's sources (set LYTK_LILYPOND_SRC)"]
fn syntax_board() {
    let Some(root) = corpus_root() else {
        eprintln!("syntax_board skipped: no LilyPond sources (set LYTK_LILYPOND_SRC)");
        return;
    };
    let regression = root.join("input/regression");
    let sets = [
        ("input/regression/*.ly", ly_files(&regression, false)),
        (
            "input/regression/*/**.ly",
            ly_files(&regression, true)
                .into_iter()
                .filter(|p| p.parent() != Some(regression.as_path()))
                .collect(),
        ),
        (
            "Documentation/snippets/*.ly",
            ly_files(&root.join("Documentation/snippets"), false),
        ),
    ];

    let (mut total, mut flagged_files) = (0, Vec::new());
    let (mut expected, mut caught) = (0, 0);
    let mut token_gaps = Vec::new();
    let mut times = Vec::new();
    for (name, files) in &sets {
        let start = Instant::now();
        let mut bad = 0;
        for path in files {
            let src = read_lossy(path);
            let t = Instant::now();
            let errors: Vec<String> = check(&src, false)
                .iter()
                .filter(|d| d.is_error())
                .map(|d| d.to_string())
                .collect();
            times.push(t.elapsed());
            if let Some(gap) = common::token_gap(&src) {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                token_gaps.push(format!("{}: {gap}", rel.display()));
            }
            if expects_error(&src) {
                expected += 1;
                caught += usize::from(!errors.is_empty());
            } else if !errors.is_empty() {
                bad += 1;
                let rel = path.strip_prefix(&root).unwrap_or(path);
                flagged_files.push(format!(
                    "{}: {:?}",
                    rel.display(),
                    &errors[..errors.len().min(3)]
                ));
            }
        }
        let ms = start.elapsed().as_secs_f64() * 1e3;
        println!(
            "{name:28} {:5} files  {bad:3} with errors  {:.3} ms/file",
            files.len(),
            ms / files.len().max(1) as f64
        );
        total += files.len();
    }
    times.sort();
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    println!(
        "check_lilypond median: {:.3} ms",
        median.as_secs_f64() * 1e3
    );
    println!("files expecting an error: {expected}, a syntax error found in {caught}");
    for gap in &token_gaps {
        println!("  token gap: {gap}");
    }
    for f in &flagged_files {
        println!("  error: {f}");
    }
    assert!(
        total >= SYNTAX_MIN_FILES,
        "only {total} files under {}: wrong LYTK_LILYPOND_SRC?",
        root.display()
    );
    // lytk.tokenize loses no text.
    assert!(
        token_gaps.is_empty(),
        "{} files lose text in tokens",
        token_gaps.len()
    );
    assert!(
        flagged_files.len() <= SYNTAX_ERROR_FILES,
        "{} valid files with syntax errors, baseline {SYNTAX_ERROR_FILES}",
        flagged_files.len()
    );
    if flagged_files.len() < SYNTAX_ERROR_FILES {
        println!(
            "SYNTAX_ERROR_FILES can be lowered to {}",
            flagged_files.len()
        );
    }
}

/// Every valid file of the corpus, for the boards that read them all.
fn corpus_files(root: &Path) -> Vec<PathBuf> {
    let mut files = ly_files(&root.join("input/regression"), true);
    files.extend(ly_files(&root.join("Documentation/snippets"), false));
    files
}

#[test]
#[ignore = "needs LilyPond's sources (set LYTK_LILYPOND_SRC)"]
fn reader_board() {
    let Some(root) = corpus_root() else {
        eprintln!("reader_board skipped: no LilyPond sources (set LYTK_LILYPOND_SRC)");
        return;
    };
    let files = corpus_files(&root);
    assert!(files.len() >= SYNTAX_MIN_FILES, "wrong LYTK_LILYPOND_SRC?");
    let reader = LyToIrAdapter::new();
    let start = Instant::now();
    let (mut other_errors, mut refused, mut with_errors) = (0, Vec::new(), Vec::new());
    let (mut expected, mut caught) = (0, 0);
    // Board (d): warnings per code, as (count, files).
    let mut warnings: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut samples: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        let src = read_lossy(path);
        match reader.read_str(&src) {
            Ok(reading) => {
                let errors: Vec<_> = reading
                    .diagnostics
                    .iter()
                    .filter(|d| d.is_error())
                    .collect();
                if expects_error(&src) {
                    expected += 1;
                    caught += usize::from(!errors.is_empty());
                    continue;
                }
                if !errors.is_empty() {
                    with_errors.push(format!("{rel}: {}", errors[0]));
                }
                let mut seen = std::collections::BTreeSet::new();
                for d in reading.diagnostics.iter().filter(|d| !d.is_error()) {
                    let entry = warnings.entry(d.code).or_default();
                    entry.0 += 1;
                    if seen.insert(d.code) {
                        entry.1 += 1;
                        let list = samples.entry(d.code).or_default();
                        if list.len() < WARNING_SAMPLES {
                            list.push(format!("{rel}: {d}"));
                        }
                    }
                }
            }
            Err(e) if e.to_string().contains("refuses") => {
                refused.push(format!("{rel}: {e}"));
            }
            Err(_) => other_errors += 1,
        }
    }
    println!(
        "{} files read in {:.1} s: {} refused, {other_errors} other errors, {} with errors",
        files.len(),
        start.elapsed().as_secs_f64(),
        refused.len(),
        with_errors.len()
    );
    println!("files expecting an error: {expected}, an error found in {caught}");
    for r in &refused {
        println!("  refused: {r}");
    }
    for e in &with_errors {
        println!("  error: {e}");
    }
    println!("warnings (count, files):");
    for (code, (count, in_files)) in &warnings {
        println!("  {code:20} {count:6} {in_files:5}");
        for sample in &samples[code] {
            println!("      {sample}");
        }
    }
    // The reader's bounds are orders of magnitude above real music: no valid
    // file may reach one.
    assert!(refused.is_empty(), "{} valid files refused", refused.len());
    assert!(
        with_errors.len() <= READER_ERROR_FILES,
        "{} valid files with errors, baseline {READER_ERROR_FILES}",
        with_errors.len()
    );
}

/// `key = "…"` of `src`, found by text search alone and decoded with
/// LilyPond's escapes (`lily/lexer.ll`): an oracle independent of the grammar.
fn decode_field(src: &str, key: &str) -> Option<String> {
    let at = src.find(&format!("{key} = \""))? + key.len() + 4;
    let mut out = String::new();
    let mut chars = src[at..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                e @ ('\\' | '"' | '\'') => out.push(e),
                e => {
                    out.push('\\');
                    out.push(e);
                }
            },
            c => out.push(c),
        }
    }
    None
}

#[test]
#[ignore = "needs LilyPond's sources (set LYTK_LILYPOND_SRC)"]
fn snippet_headers() {
    let Some(root) = corpus_root() else {
        eprintln!("snippet_headers skipped: no LilyPond sources (set LYTK_LILYPOND_SRC)");
        return;
    };
    let files = ly_files(&root.join("Documentation/snippets"), false);
    assert!(files.len() >= 380, "wrong LYTK_LILYPOND_SRC?");
    let (mut compared, mut differ) = (0, Vec::new());
    for path in &files {
        let src = read_lossy(path);
        let fields = header_fields(&src);
        for key in ["texidoc", "categories"] {
            let expected = decode_field(&src, key);
            let found = fields
                .iter()
                .find(|f| f.key == key && f.score.is_none())
                .map(|f| f.value.clone());
            compared += usize::from(expected.is_some());
            if found != expected {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                differ.push(format!("{name} {key}: {found:?} != {expected:?}"));
            }
        }
    }
    println!(
        "{compared} texidoc/categories values of {} snippets compared",
        files.len()
    );
    for d in &differ {
        println!("  differs: {d}");
    }
    assert!(differ.is_empty(), "{} fields differ", differ.len());
}

/// The token kinds a mutation deletes, with their board label. A token is a
/// leaf of the tree, so a `{` inside a string or a comment is never picked.
/// A slur's `(` or `)` is not one: without it, the music is still valid.
const MUTATION_KINDS: &[(&str, &str, Option<&str>)] = &[
    // (label, token kind, required parent kind)
    ("open brace `{`", "{", None),
    ("close brace `}`", "}", None),
    ("open `<<`", "<<", None),
    ("close `>>`", ">>", None),
    ("chord `>`", ">", Some("chord")),
    ("string quote", "\"", None),
    ("Scheme `(`", "(", Some("scheme_list")),
    ("Scheme `)`", ")", Some("scheme_list")),
];

fn token_ranges(tree: &Tree, kind: &str, parent: Option<&str>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.child_count() == 0 {
            if !node.is_named()
                && node.kind() == kind
                && parent.is_none_or(|p| node.parent().is_some_and(|n| n.kind() == p))
            {
                out.push((node.start_byte(), node.end_byte()));
            }
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    out.sort();
    out
}

/// Up to three occurrences per file and kind: the first, the middle and the last.
fn picks(n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = [0, n / 2, n.saturating_sub(1)]
        .into_iter()
        .filter(|&i| i < n)
        .collect();
    v.dedup();
    v
}

fn has_error(text: &str, semantic: bool) -> bool {
    check(text, semantic).iter().any(|d| d.is_error())
}

#[test]
fn mutation_board() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ly");
    let files = ly_files(&dir, false);
    assert!(
        files.len() >= 30,
        "fixtures missing under {}",
        dir.display()
    );

    let mut parser = LilyPondParser::new().expect("grammar loads");
    // label → (caught by the syntax check, caught by the reading, made)
    let mut board: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    let mut missed = Vec::new();
    for path in &files {
        let src = read_lossy(path);
        let tree = parser.parse(&src).expect("fixture parses");
        let before = check(&src, true);
        assert!(
            !before.iter().any(|d| d.is_error()),
            "{} has an error before any mutation: {}",
            path.display(),
            before.iter().find(|d| d.is_error()).unwrap()
        );
        for &(label, kind, parent) in MUTATION_KINDS {
            let ranges = token_ranges(&tree, kind, parent);
            for i in picks(ranges.len()) {
                let (a, b) = ranges[i];
                let broken = format!("{}{}", &src[..a], &src[b..]);
                // The reading reports every syntax error too.
                let by_syntax = has_error(&broken, false);
                let by_reading = by_syntax || has_error(&broken, true);
                let entry = board.entry(label).or_default();
                entry.0 += usize::from(by_syntax);
                entry.1 += usize::from(by_reading);
                entry.2 += 1;
                if !by_reading {
                    let line = src[..a].matches('\n').count() + 1;
                    missed.push(format!(
                        "{} deleted at {}:{line}",
                        label,
                        path.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
        }
    }

    println!(
        "{:18} {:>6} {:>7} {:>5}",
        "deleted", "syntax", "reading", "made"
    );
    for (label, (syntax, reading, made)) in &board {
        println!("{label:18} {syntax:6} {reading:7} {made:5}");
    }
    for m in &missed {
        println!("  not caught: {m}");
    }
    for &(label, syntax_baseline, reading_baseline) in MUTATIONS_CAUGHT {
        let (syntax, reading, made) = board.get(label).copied().unwrap_or_default();
        assert!(
            made > 0,
            "no `{label}` token in the fixtures: grammar changed?"
        );
        assert!(
            syntax >= syntax_baseline && reading >= reading_baseline,
            "{label}: {syntax} (syntax) and {reading} (reading) of {made} caught, \
             baselines {syntax_baseline} and {reading_baseline}"
        );
    }
}
