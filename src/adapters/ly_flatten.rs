//! LilyPond include flattener.
//!
//! Recursively expands `\include "file.ly"` directives in a LilyPond source
//! file, producing a single self-contained flat file.
//!
//! # Include semantics
//! Plain `\include` always inlines the referenced file (faithful to LilyPond's
//! own behaviour). There is no include-once deduplication. If file A includes
//! file B which in turn includes file A, a [`FlattenError::Circular`] is
//! returned. Without include-once, a diamond (a file including the next one
//! twice) doubles the output at every level, so flattening stops with
//! [`FlattenError::TooLarge`] after [`MAX_INCLUDES`] includes or
//! [`MAX_OUTPUT_BYTES`] of output.
//!
//! # Normalization (post-processing)
//! After full expansion the output is scanned for duplicate command lines:
//!
//! | Command      | Behaviour                                      |
//! |--------------|------------------------------------------------|
//! | `\version`   | Last occurrence kept; earlier ones removed; warning emitted if multiple |
//!
//! `\language` lines all stay (each applies from where it stands), and so do
//! several `\header` blocks (LilyPond merges them).
//!
//! # Finding includes
//! Includes are found on the syntax tree: anywhere in a line, never in a
//! comment or a string. An include of a file not found is an error, except
//! for LilyPond's own files (`english.ly`, `gregorian.ly`), which LilyPond
//! finds in its installation, and with [`FlattenOpts::keep_missing`].
//!
//! # Output markers
//! When `FlattenOpts::add_markers` is `true` (the default), each included file
//! is wrapped with comment lines:
//! ```text
//! % === BEGIN INCLUDE: path/to/file.ly ===
//! …content…
//! % === END INCLUDE: path/to/file.ly ===
//! ```

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Most `\include` expansions one flatten performs; real projects use tens.
pub const MAX_INCLUDES: usize = 10_000;
/// Most bytes of source one flatten emits.
pub const MAX_OUTPUT_BYTES: usize = 64 << 20;

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

/// Options controlling the flattening process.
#[derive(Debug, Clone)]
pub struct FlattenOpts {
    /// Additional directories to search when resolving `\include` paths,
    /// analogous to LilyPond's `-I` flag. Each directory is searched in order
    /// *after* the directory of the file that contains the include directive.
    pub include_paths: Vec<PathBuf>,

    /// Wrap each inlined file with `% === BEGIN/END INCLUDE: … ===` comment
    /// markers. Defaults to `true`.
    pub add_markers: bool,

    /// Keep an `\include` of a file not found as it is, instead of failing
    /// with [`FlattenError::NotFound`]. An include of one of LilyPond's own
    /// files (`english.ly`) is kept either way. Defaults to `false`.
    pub keep_missing: bool,
}

impl Default for FlattenOpts {
    fn default() -> Self {
        Self {
            include_paths: Vec::new(),
            add_markers: true,
            keep_missing: false,
        }
    }
}

/// Errors produced by the flattener.
#[derive(Debug, Error)]
pub enum FlattenError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("include file not found: {0}")]
    NotFound(PathBuf),

    #[error("circular include detected: {chain}")]
    Circular { chain: String },

    #[error("{0}; lytk refuses input this large")]
    TooLarge(String),
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Flatten a LilyPond file at `path`, expanding all `\include` directives
/// recursively and returning the combined text.
///
/// Warnings about duplicate `\version` directives are written to `stderr`.
pub fn flatten(path: &Path, opts: FlattenOpts) -> Result<String, FlattenError> {
    let src = read_file(path)?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    flatten_str(&src, Some(base_dir), opts)
}

/// Flatten LilyPond source text `src`, resolving relative `\include` paths
/// against `base_dir` (when given) and then `opts.include_paths`.
///
/// This is the lower-level entry point; use [`flatten`] when working with
/// files on disk.
pub fn flatten_str(
    src: &str,
    base_dir: Option<&Path>,
    opts: FlattenOpts,
) -> Result<String, FlattenError> {
    let (expanded, _) = flatten_mapped(src, base_dir, &opts)?;
    normalize(expanded)
}

/// Where the text from an offset of the flattened output on comes from: the
/// source itself, from a byte offset on; or the expansion of the `\include`
/// statement at a byte range of the source, naming the file.
#[derive(Debug, Clone)]
pub(crate) enum Origin {
    Source(usize),
    Include {
        statement: std::ops::Range<usize>,
        path: String,
    },
}

/// The pieces of a flattened text, in order: where each starts in the output,
/// and where it comes from. A piece runs to the next one's start.
pub(crate) type SourceMap = Vec<(usize, Origin)>;

/// `src` with its includes expanded (no normalization), and where each piece
/// of the result comes from.
pub(crate) fn flatten_mapped(
    src: &str,
    base_dir: Option<&Path>,
    opts: &FlattenOpts,
) -> Result<(String, SourceMap), FlattenError> {
    let mut ctx = ExpandCtx {
        opts,
        ancestors: Vec::new(),
        includes: 0,
        bytes: src.len(),
    };
    let mut map = SourceMap::new();
    let expanded = expand(src, base_dir, &mut ctx, Some(&mut map))?;
    Ok((expanded, map))
}

// ---------------------------------------------------------------------------
// Internal recursive expander
// ---------------------------------------------------------------------------

struct ExpandCtx<'a> {
    opts: &'a FlattenOpts,
    /// Canonical absolute paths of files currently open (ancestor chain).
    /// Used for circular-dependency detection.
    ancestors: Vec<PathBuf>,
    /// Includes expanded and bytes read so far, against the bounds above.
    includes: usize,
    bytes: usize,
}

/// The `\include "…"` statements of `src`, from its syntax tree (so not in
/// comments or strings): each statement's byte range and its path, the text
/// between the quotes, as LilyPond's lexer takes it (no escapes).
fn include_statements(src: &str) -> Result<Vec<(std::ops::Range<usize>, String)>, FlattenError> {
    let tree = crate::parser::LilyPondParser::new()
        .and_then(|mut p| p.parse(src))
        .map_err(|e| FlattenError::TooLarge(e.to_string()))?;
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "escaped_word" && &src[node.byte_range()] == "\\include" {
            let string = node.next_sibling().filter(|n| n.kind() == "string");
            if let Some(string) = string {
                let raw = &src[string.byte_range()];
                let path = raw.trim_start_matches('"').trim_end_matches('"');
                if !path.is_empty() {
                    out.push((node.start_byte()..string.end_byte(), path.to_string()));
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(out);
            }
        }
    }
}

/// The part of `src` an expansion replaces: the whole line when the
/// statement stands alone on it, else the statement itself.
fn replaced_span(src: &str, statement: &std::ops::Range<usize>) -> (usize, usize, bool) {
    let line_start = src[..statement.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[statement.end..]
        .find('\n')
        .map_or(src.len(), |i| statement.end + i + 1);
    let alone = src[line_start..statement.start].trim().is_empty()
        && src[statement.end..line_end].trim().is_empty();
    if alone {
        (line_start, line_end, true)
    } else {
        (statement.start, statement.end, false)
    }
}

/// Whether `raw` names a file of LilyPond's own `ly/` directory, which
/// LilyPond finds when nothing else does (`english.ly`, `gregorian.ly`).
fn is_lilypond_file(raw: &str) -> bool {
    super::ly_to_ir::LILYPOND_FILES.binary_search(&raw).is_ok()
}

fn expand(
    src: &str,
    base_dir: Option<&Path>,
    ctx: &mut ExpandCtx<'_>,
    mut map: Option<&mut SourceMap>,
) -> Result<String, FlattenError> {
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for (statement, raw_path) in include_statements(src)? {
        let resolved = match resolve_include(&raw_path, base_dir, ctx.opts) {
            Ok(resolved) => resolved,
            // Kept as written: LilyPond's own files, and missing files when
            // asked (the readers warn about them).
            Err(FlattenError::NotFound(_))
                if ctx.opts.keep_missing || is_lilypond_file(&raw_path) =>
            {
                continue
            }
            Err(e) => return Err(e),
        };
        let canonical = canonicalize(&resolved)?;

        // Circular dependency check
        if ctx.ancestors.contains(&canonical) {
            let mut chain: Vec<String> = ctx
                .ancestors
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            chain.push(canonical.display().to_string());
            return Err(FlattenError::Circular {
                chain: chain.join(" -> "),
            });
        }

        let inc_src = read_file(&resolved)?;
        ctx.includes += 1;
        ctx.bytes += inc_src.len() + 2 * raw_path.len();
        if ctx.includes > MAX_INCLUDES {
            return Err(FlattenError::TooLarge(format!(
                "the includes expand more than {MAX_INCLUDES} times"
            )));
        }
        if ctx.bytes > MAX_OUTPUT_BYTES {
            return Err(FlattenError::TooLarge(format!(
                "the flattened source exceeds {} MiB",
                MAX_OUTPUT_BYTES >> 20
            )));
        }
        let inc_base = resolved.parent().unwrap_or(Path::new("."));

        ctx.ancestors.push(canonical);
        let inner = expand(&inc_src, Some(inc_base), ctx, None)?;
        ctx.ancestors.pop();

        let (cut_start, cut_end, alone) = replaced_span(src, &statement);
        if let Some(map) = map.as_deref_mut() {
            map.push((out.len(), Origin::Source(at)));
        }
        out.push_str(&src[at..cut_start]);
        if let Some(map) = map.as_deref_mut() {
            let path = raw_path.clone();
            map.push((out.len(), Origin::Include { statement, path }));
        }
        if !alone && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if ctx.opts.add_markers {
            out.push_str(&format!("% === BEGIN INCLUDE: {raw_path} ===\n"));
        }
        out.push_str(inner.trim_end_matches('\n'));
        out.push('\n');
        if ctx.opts.add_markers {
            out.push_str(&format!("% === END INCLUDE: {raw_path} ===\n"));
        }
        at = cut_end;
    }
    if let Some(map) = map {
        map.push((out.len(), Origin::Source(at)));
    }
    out.push_str(&src[at..]);
    Ok(out)
}

/// Resolve an include path relative to `base_dir`, then to the include
/// paths, with extension fallback (`.ly` then `.ily`). An absolute path is
/// taken as it is.
fn resolve_include(
    raw: &str,
    base_dir: Option<&Path>,
    opts: &FlattenOpts,
) -> Result<PathBuf, FlattenError> {
    let search_dirs: Vec<&Path> = if Path::new(raw).is_absolute() {
        vec![Path::new("")]
    } else {
        base_dir
            .into_iter()
            .chain(opts.include_paths.iter().map(PathBuf::as_path))
            .collect()
    };

    for dir in search_dirs {
        let base = dir.join(raw);
        // 1. Exact path as given
        if base.is_file() {
            return Ok(base);
        }
        // 2. With .ly extension
        let with_ly = base.with_extension("ly");
        if with_ly.is_file() {
            return Ok(with_ly);
        }
        // 3. With .ily extension (only if raw doesn't already end in .ily)
        if !raw.ends_with(".ily") {
            let with_ily = base.with_extension("ily");
            if with_ily.is_file() {
                return Ok(with_ily);
            }
        }
    }

    Err(FlattenError::NotFound(PathBuf::from(raw)))
}

fn read_file(path: &Path) -> Result<String, FlattenError> {
    std::fs::read_to_string(path).map_err(|source| FlattenError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn canonicalize(path: &Path) -> Result<PathBuf, FlattenError> {
    std::fs::canonicalize(path).map_err(|source| FlattenError::Io {
        path: path.to_path_buf(),
        source,
    })
}

// ---------------------------------------------------------------------------
// Post-processing normalization pass
// ---------------------------------------------------------------------------

/// Mark every line matching `is_match` for removal except the last, warning
/// (using the directive `name`) when more than one occurrence is found.
fn dedup_keep_last(
    lines: &[&str],
    is_match: impl Fn(&str) -> bool,
    name: &str,
    remove: &mut std::collections::HashSet<usize>,
) {
    let idxs: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_match(l))
        .map(|(i, _)| i)
        .collect();
    if idxs.len() > 1 {
        eprintln!(
            "warning: multiple {name} directives found — keeping last, removing {} earlier occurrence(s)",
            idxs.len() - 1
        );
        for &i in &idxs[..idxs.len() - 1] {
            remove.insert(i);
        }
    }
}

/// Keep one `\version` line, the last, in the fully expanded text.
/// `\language` lines all stay: each applies from where it stands. Several
/// `\header` blocks are fine too: LilyPond merges them.
fn normalize(text: String) -> Result<String, FlattenError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    dedup_keep_last(&lines, is_version_line, "\\version", &mut remove);

    // Reconstruct the text, skipping removed lines.
    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if !remove.contains(&i) {
            out.push_str(line);
            out.push('\n');
        }
    }

    Ok(out)
}

fn is_version_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with(r#"\version ""#)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    // -----------------------------------------------------------------------
    // Include resolution
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_includes() {
        let src = r#"\version "2.24.0"
{ c' d' e' f' }
"#;
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default()).unwrap();
        assert!(result.contains(r#"\version "2.24.0""#));
        assert!(result.contains("c' d' e' f'"));
    }

    #[test]
    fn test_single_include() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ly", "{ c' d' }\n");
        write(
            dir.path(),
            "main.ly",
            r#"\include "part.ly"
{ e' f' }
"#,
        );
        let result = flatten(&dir.path().join("main.ly"), FlattenOpts::default()).unwrap();
        assert!(result.contains("c' d'"));
        assert!(result.contains("e' f'"));
    }

    #[test]
    fn test_markers_present() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ly", "{ c' }\n");
        write(dir.path(), "main.ly", r#"\include "part.ly""#);
        let result = flatten(&dir.path().join("main.ly"), FlattenOpts::default()).unwrap();
        assert!(result.contains("% === BEGIN INCLUDE: part.ly ==="));
        assert!(result.contains("% === END INCLUDE: part.ly ==="));
    }

    #[test]
    fn test_markers_absent() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ly", "{ c' }\n");
        write(dir.path(), "main.ly", r#"\include "part.ly""#);
        let opts = FlattenOpts {
            add_markers: false,
            ..Default::default()
        };
        let result = flatten(&dir.path().join("main.ly"), opts).unwrap();
        assert!(!result.contains("BEGIN INCLUDE"));
        assert!(!result.contains("END INCLUDE"));
        assert!(result.contains("c'"));
    }

    #[test]
    fn test_extension_fallback_ly() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ly", "{ c' }\n");
        // Include without extension
        write(dir.path(), "main.ly", r#"\include "part""#);
        let result = flatten(&dir.path().join("main.ly"), FlattenOpts::default()).unwrap();
        assert!(result.contains("c'"));
    }

    #[test]
    fn test_extension_fallback_ily() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ily", "{ d' }\n");
        // Include without extension — no .ly exists, should find .ily
        write(dir.path(), "main.ly", r#"\include "part""#);
        let result = flatten(&dir.path().join("main.ly"), FlattenOpts::default()).unwrap();
        assert!(result.contains("d'"));
    }

    #[test]
    fn test_include_path_searched() {
        let dir = TempDir::new().unwrap();
        let lib_dir = TempDir::new().unwrap();
        write(lib_dir.path(), "common.ly", "{ e' }\n");
        write(dir.path(), "main.ly", r#"\include "common.ly""#);
        let opts = FlattenOpts {
            include_paths: vec![lib_dir.path().to_path_buf()],
            add_markers: false,
            ..Default::default()
        };
        let result = flatten(&dir.path().join("main.ly"), opts).unwrap();
        assert!(result.contains("e'"));
    }

    #[test]
    fn test_nested_includes() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "c.ly", "{ g' }\n");
        write(dir.path(), "b.ly", "\\include \"c.ly\"\n{ f' }\n");
        write(dir.path(), "a.ly", "\\include \"b.ly\"\n{ e' }\n");
        let result = flatten(
            &dir.path().join("a.ly"),
            FlattenOpts {
                add_markers: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.contains("g'"));
        assert!(result.contains("f'"));
        assert!(result.contains("e'"));
        // g' must appear before f' which must appear before e'
        let g_pos = result.find("g'").unwrap();
        let f_pos = result.find("f'").unwrap();
        let e_pos = result.find("e'").unwrap();
        assert!(g_pos < f_pos && f_pos < e_pos);
    }

    #[test]
    fn test_relative_path_resolution() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        write(&sub, "inner.ly", "{ a' }\n");
        // b.ly is in sub/ and includes inner.ly relative to itself
        write(&sub, "b.ly", "\\include \"inner.ly\"\n");
        // main.ly includes sub/b.ly
        write(dir.path(), "main.ly", "\\include \"sub/b.ly\"\n");
        let result = flatten(
            &dir.path().join("main.ly"),
            FlattenOpts {
                add_markers: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.contains("a'"));
    }

    // -----------------------------------------------------------------------
    // Error conditions
    // -----------------------------------------------------------------------

    #[test]
    fn test_not_found_error() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "main.ly", r#"\include "nonexistent.ly""#);
        let err = flatten(&dir.path().join("main.ly"), FlattenOpts::default()).unwrap_err();
        assert!(matches!(err, FlattenError::NotFound(_)));
    }

    #[test]
    fn test_circular_include_error() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a.ly", "\\include \"b.ly\"\n");
        write(dir.path(), "b.ly", "\\include \"a.ly\"\n");
        let err = flatten(&dir.path().join("a.ly"), FlattenOpts::default()).unwrap_err();
        assert!(
            matches!(&err, FlattenError::Circular { chain } if chain.contains("a.ly") && chain.contains("b.ly"))
        );
    }

    #[test]
    fn test_include_diamond_is_refused() {
        // f0 includes f1 twice, f1 includes f2 twice, …: 2^20 expansions.
        let dir = TempDir::new().unwrap();
        for i in 0..20 {
            let next = format!("\\include \"f{}.ly\"\n", i + 1);
            write(dir.path(), &format!("f{i}.ly"), &next.repeat(2));
        }
        write(dir.path(), "f20.ly", "c4\n");
        let err = flatten(&dir.path().join("f0.ly"), FlattenOpts::default()).unwrap_err();
        assert!(matches!(err, FlattenError::TooLarge(_)), "got {err:?}");
    }

    #[test]
    fn test_self_include_error() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "self.ly", "\\include \"self.ly\"\n");
        let err = flatten(&dir.path().join("self.ly"), FlattenOpts::default()).unwrap_err();
        assert!(matches!(err, FlattenError::Circular { .. }));
    }

    // -----------------------------------------------------------------------
    // Normalization
    // -----------------------------------------------------------------------

    #[test]
    fn test_single_version_kept() {
        let src = r#"\version "2.24.0"
{ c' }
"#;
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default()).unwrap();
        assert_eq!(
            result.matches(r#"\version"#).count(),
            1,
            "single version should be preserved"
        );
    }

    #[test]
    fn test_duplicate_version_last_wins() {
        let src = r#"\version "2.22.0"
{ c' }
\version "2.24.0"
{ d' }
"#;
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default()).unwrap();
        assert_eq!(result.matches(r#"\version"#).count(), 1);
        assert!(result.contains(r#"\version "2.24.0""#));
        assert!(!result.contains(r#"\version "2.22.0""#));
    }

    #[test]
    fn test_every_language_stays() {
        // Each applies from where it stands: dropping the first would read
        // `c'` in German.
        let src = r#"\language "english"
{ cs' }
\language "deutsch"
{ cis' }
"#;
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default()).unwrap();
        assert_eq!(result, src);
    }

    #[test]
    fn test_single_header_ok() {
        let src = r#"\header {
  title = "Test"
}
{ c' }
"#;
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_several_headers_stay() {
        // LilyPond merges top-level headers; a score's own is its own.
        let src = "\\header { title = \"A\" }\n\\header { composer = \"B\" }\n\\score { \\header { piece = \"I\" } { c' } }\n";
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default()).unwrap();
        assert_eq!(result, src);
    }

    // -----------------------------------------------------------------------
    // Comment skipping
    // -----------------------------------------------------------------------

    #[test]
    fn test_include_in_line_comment_skipped() {
        let src = r#"% \include "foo.ly"
{ c' }
"#;
        // No file foo.ly exists — should not error because the include is commented out.
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default());
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(!text.contains("BEGIN INCLUDE"));
    }

    #[test]
    fn test_include_in_block_comment_skipped() {
        let src = "%{\n\\include \"foo.ly\"\n%}\n{ c' }\n";
        let result = flatten_str(src, Some(Path::new(".")), FlattenOpts::default());
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(!text.contains("BEGIN INCLUDE"));
    }

    #[test]
    fn test_includes_are_found_on_the_tree() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ily", "d'4\n");
        let opts = || FlattenOpts {
            add_markers: false,
            ..Default::default()
        };
        // Mid-line, and after a one-line block comment (which left the line
        // scanner "inside a comment" for the rest of the file).
        let src = "%{ note %}\n{ c'4 \\include \"part.ily\" e'4 }\n";
        let result = flatten_str(src, Some(dir.path()), opts()).unwrap();
        assert_eq!(result, "%{ note %}\n{ c'4 \nd'4\n e'4 }\n");
        // Not in a string, and a commented one is not followed.
        let src = "{ c'4^\"\\\\include \\\"x.ly\\\"\" } % \\include \"y.ly\"\n";
        assert_eq!(flatten_str(src, Some(dir.path()), opts()).unwrap(), src);
    }

    #[test]
    fn test_lilypond_files_and_missing_ones_are_kept() {
        let src = "\\include \"english.ly\"\n{ cs'4 }\n";
        assert_eq!(flatten_str(src, None, FlattenOpts::default()).unwrap(), src);
        let src = "\\include \"lib.ily\"\n{ c'4 }\n";
        assert!(matches!(
            flatten_str(src, None, FlattenOpts::default()),
            Err(FlattenError::NotFound(_))
        ));
        let keep = FlattenOpts {
            keep_missing: true,
            ..Default::default()
        };
        assert_eq!(flatten_str(src, None, keep).unwrap(), src);
    }

    #[test]
    fn test_the_map_tells_where_each_piece_comes_from() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "part.ily", "d'4\n");
        let src = "{ c'4 }\n\\include \"part.ily\"\n{ e'4 }\n";
        let opts = FlattenOpts {
            add_markers: false,
            ..Default::default()
        };
        let (out, map) = flatten_mapped(src, Some(dir.path()), &opts).unwrap();
        assert_eq!(out, "{ c'4 }\nd'4\n{ e'4 }\n");
        let starts: Vec<usize> = map.iter().map(|(at, _)| *at).collect();
        assert_eq!(starts, [0, 8, 12]);
        assert!(matches!(map[1].1, Origin::Include { ref statement, .. } if statement == &(8..27)));
        assert!(matches!(map[2].1, Origin::Source(28)));
    }
}
