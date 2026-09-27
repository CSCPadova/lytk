//! The LilyPond grammar against corpora: how often valid LilyPond comes out
//! of the parser with syntax errors, and how often broken LilyPond is caught.
//! Epic J (0.3.0) builds strict mode and diagnostics on these two numbers.
//!
//! - `mutation_board` breaks the committed `.ly` fixtures one token at a time
//!   (a brace, a quote, a `>>`, a chord's `>`, a Scheme parenthesis) and
//!   counts how many of the broken files the tree flags. It runs in every
//!   `cargo test`.
//! - `syntax_board` parses LilyPond's own regression tests and documentation
//!   snippets, all valid LilyPond, and counts the files the tree flags;
//!   `reader_board` reads them all and counts the ones the reader refuses as
//!   too large (its bounds must not catch real music). Both need LilyPond's
//!   sources, so they are ignored by default:
//!   `LYTK_LILYPOND_SRC=/path/to/lilypond cargo test --test ly_corpus -- --ignored --nocapture`
//!   (`LYTK_LILYPOND_SRC` defaults to the `lilypond/` reference checkout; CI
//!   sparse-clones v2.26.0).
//!
//! "Flagged" means the tree has an ERROR or MISSING node. The grammar is
//! nearly token-level (`c4` is `symbol` + `unsigned_integer`), so these
//! boards measure structure only: unknown commands or `c3` are for the
//! reader's own diagnostics (J3).

use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::ToIrAdapter;
use _core::parser::LilyPondParser;
use tree_sitter::{Node, Tree};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

// Files of LilyPond v2.26.0's `input/regression` (recursive) and
// `Documentation/snippets` whose tree has an ERROR or MISSING node
// (2026-09-27): `bom-mark.ly` (a byte-order mark mid-file, which LilyPond
// reads as whitespace) and `other/display-lily-tests.ly` (`##[ #]`).
// It may only fall.
const SYNTAX_FLAGGED_FILES: usize = 2;
/// The corpus must be at least this large, so a wrong path fails loudly
/// instead of passing on an empty directory.
const SYNTAX_MIN_FILES: usize = 2500;

// Broken fixtures the tree flags, per kind of deleted token, out of the
// mutations made (2026-09-27: 100, 100, 46, 46, 21, 105, 47, 47). They may
// only rise. Deleting `<<` or a Scheme `(` is rarely flagged: what is left,
// a lone `>>` or `)`, parses as `punctuation`, and a lone `)` or `>` is
// legal LilyPond (a slur end, the `->` accent). Telling them from unmatched
// ones takes the reader's context: J3's diagnostics.
const MUTATIONS_FLAGGED: &[(&str, usize)] = &[
    ("open brace `{`", 100),
    ("close brace `}`", 100),
    ("open `<<`", 4),
    ("close `>>`", 46),
    ("chord `>`", 21),
    ("string quote", 105),
    ("Scheme `(`", 4),
    ("Scheme `)`", 34),
];

/// Every ERROR or MISSING node of `tree`, as (line, column, what), 1-based.
fn flagged(tree: &Tree) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    if !tree.root_node().has_error() {
        return out;
    }
    let mut stack: Vec<Node> = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            let p = node.start_position();
            let what = if node.is_missing() {
                format!("MISSING {}", node.kind())
            } else {
                "ERROR".to_string()
            };
            out.push((p.row + 1, p.column + 1, what));
            continue;
        }
        if node.has_error() {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }
    out.sort();
    out
}

fn parse_flags(parser: &mut LilyPondParser, src: &str) -> Vec<(usize, usize, String)> {
    match parser.parse(src) {
        Ok(tree) => flagged(&tree),
        Err(e) => vec![(0, 0, format!("parse error: {e}"))],
    }
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

    let mut parser = LilyPondParser::new().expect("grammar loads");
    let (mut total, mut flagged_files) = (0, Vec::new());
    for (name, files) in &sets {
        let start = Instant::now();
        let mut bad = 0;
        for path in files {
            let flags = parse_flags(&mut parser, &read_lossy(path));
            if !flags.is_empty() {
                bad += 1;
                let rel = path.strip_prefix(&root).unwrap_or(path);
                flagged_files.push(format!(
                    "{}: {:?}",
                    rel.display(),
                    &flags[..flags.len().min(3)]
                ));
            }
        }
        let ms = start.elapsed().as_secs_f64() * 1e3;
        println!(
            "{name:28} {:5} files  {bad:3} flagged  {:.3} ms/file",
            files.len(),
            ms / files.len().max(1) as f64
        );
        total += files.len();
    }
    for f in &flagged_files {
        println!("  flagged: {f}");
    }
    assert!(
        total >= SYNTAX_MIN_FILES,
        "only {total} files under {}: wrong LYTK_LILYPOND_SRC?",
        root.display()
    );
    assert!(
        flagged_files.len() <= SYNTAX_FLAGGED_FILES,
        "{} valid files flagged, baseline {SYNTAX_FLAGGED_FILES}",
        flagged_files.len()
    );
    if flagged_files.len() < SYNTAX_FLAGGED_FILES {
        println!(
            "SYNTAX_FLAGGED_FILES can be lowered to {}",
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
    let (mut other_errors, mut refused) = (0, Vec::new());
    for path in &files {
        match reader.convert_str(&read_lossy(path)) {
            Ok(_) => {}
            Err(e) if e.to_string().contains("refuses") => {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                refused.push(format!("{}: {e}", rel.display()));
            }
            Err(_) => other_errors += 1,
        }
    }
    println!(
        "{} files read in {:.1} s: {} refused, {other_errors} other errors",
        files.len(),
        start.elapsed().as_secs_f64(),
        refused.len()
    );
    for r in &refused {
        println!("  refused: {r}");
    }
    // The reader's bounds are orders of magnitude above real music: no valid
    // file may reach one.
    assert!(refused.is_empty(), "{} valid files refused", refused.len());
}

/// The token kinds a mutation deletes, with their board label. A token is a
/// leaf of the tree, so a `{` inside a string or a comment is never picked.
const MUTATION_KINDS: &[(&str, &str, Option<&str>)] = &[
    // (label, token kind, required parent kind)
    ("open brace `{`", "{", None),
    ("close brace `}`", "}", None),
    ("open `<<`", "<<", None),
    ("close `>>`", ">>", None),
    ("chord `>`", ">", Some("chord")),
    ("string quote", "\"", None),
    ("Scheme `(`", "(", None),
    ("Scheme `)`", ")", None),
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
    let mut board: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut missed = Vec::new();
    for path in &files {
        let src = read_lossy(path);
        let tree = parser.parse(&src).expect("fixture parses");
        assert!(
            flagged(&tree).is_empty(),
            "{} is flagged before any mutation",
            path.display()
        );
        for &(label, kind, parent) in MUTATION_KINDS {
            let ranges = token_ranges(&tree, kind, parent);
            for i in picks(ranges.len()) {
                let (a, b) = ranges[i];
                let broken = format!("{}{}", &src[..a], &src[b..]);
                let caught = !parse_flags(&mut parser, &broken).is_empty();
                let entry = board.entry(label).or_default();
                entry.1 += 1;
                if caught {
                    entry.0 += 1;
                } else {
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

    for (label, (caught, made)) in &board {
        println!("{label:18} {caught:4} / {made:4} flagged");
    }
    for m in &missed {
        println!("  not flagged: {m}");
    }
    for &(label, baseline) in MUTATIONS_FLAGGED {
        let (caught, made) = board.get(label).copied().unwrap_or_default();
        assert!(
            made > 0,
            "no `{label}` token in the fixtures: grammar changed?"
        );
        assert!(
            caught >= baseline,
            "{label}: {caught} of {made} flagged, baseline {baseline}"
        );
    }
}
