//! LilyPond include flattener.
//!
//! Recursively expands `\include "file.ly"` directives in a LilyPond source
//! file, producing a single self-contained flat file.
//!
//! # Include semantics
//! Plain `\include` always inlines the referenced file (faithful to LilyPond's
//! own behaviour). There is no include-once deduplication. If file A includes
//! file B which in turn includes file A, a [`FlattenError::Circular`] is
//! returned.
//!
//! # Normalization (post-processing)
//! After full expansion the output is scanned for duplicate command lines:
//!
//! | Command      | Behaviour                                      |
//! |--------------|------------------------------------------------|
//! | `\version`   | Last occurrence kept; earlier ones removed; warning emitted if multiple |
//! | `\language`  | Last occurrence kept; earlier ones removed; warning emitted if multiple |
//! | `\header`    | Multiple blocks are an **error** |
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
}

impl Default for FlattenOpts {
    fn default() -> Self {
        Self {
            include_paths: Vec::new(),
            add_markers: true,
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

    #[error("multiple \\header blocks found in flattened output")]
    MultipleHeaders,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Flatten a LilyPond file at `path`, expanding all `\include` directives
/// recursively and returning the combined text.
///
/// Warnings about duplicate `\version` or `\language` directives are written
/// to `stderr`.
pub fn flatten(path: &Path, opts: FlattenOpts) -> Result<String, FlattenError> {
    let src = read_file(path)?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    flatten_str(&src, base_dir, opts)
}

/// Flatten LilyPond source text `src`, resolving relative `\include` paths
/// against `base_dir`.
///
/// This is the lower-level entry point; use [`flatten`] when working with
/// files on disk.
pub fn flatten_str(src: &str, base_dir: &Path, opts: FlattenOpts) -> Result<String, FlattenError> {
    let mut ctx = ExpandCtx {
        opts: &opts,
        ancestors: Vec::new(),
    };
    let expanded = expand(src, base_dir, &mut ctx)?;
    normalize(expanded)
}

// ---------------------------------------------------------------------------
// Internal recursive expander
// ---------------------------------------------------------------------------

struct ExpandCtx<'a> {
    opts: &'a FlattenOpts,
    /// Canonical absolute paths of files currently open (ancestor chain).
    /// Used for circular-dependency detection.
    ancestors: Vec<PathBuf>,
}

fn expand(src: &str, base_dir: &Path, ctx: &mut ExpandCtx<'_>) -> Result<String, FlattenError> {
    let mut out = String::with_capacity(src.len());
    let mut in_block_comment = false;

    for line in src.lines() {
        let trimmed = line.trim_start();

        // Track block comments: %{ ... %}
        // LilyPond requires %{ and %} to appear as the only non-whitespace
        // content on a line at the outermost level.
        if trimmed.starts_with("%{") {
            in_block_comment = true;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if trimmed.starts_with("%}") {
            in_block_comment = false;
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Inside block comment or line comment — pass through unchanged.
        if in_block_comment || trimmed.starts_with('%') {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Try to match \include "path"
        if let Some(raw_path) = parse_include_directive(trimmed) {
            let resolved = resolve_include(raw_path, base_dir, ctx.opts)?;
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
            let inc_base = resolved.parent().unwrap_or(Path::new("."));

            ctx.ancestors.push(canonical.clone());
            let inner = expand(&inc_src, inc_base, ctx)?;
            ctx.ancestors.pop();

            if ctx.opts.add_markers {
                out.push_str(&format!("% === BEGIN INCLUDE: {} ===\n", raw_path));
                out.push_str(inner.trim_end_matches('\n'));
                out.push('\n');
                out.push_str(&format!("% === END INCLUDE: {} ===\n", raw_path));
            } else {
                out.push_str(&inner);
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    Ok(out)
}

/// Extract the path string from a `\include "path"` directive line.
/// Returns `None` if the line does not match.
fn parse_include_directive(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(r"\include")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let path = &rest[..end];
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Resolve an include path relative to `base_dir`, with extension fallback
/// (`.ly` then `.ily`) and additional search paths.
fn resolve_include(
    raw: &str,
    base_dir: &Path,
    opts: &FlattenOpts,
) -> Result<PathBuf, FlattenError> {
    // Build candidate paths in order: base_dir first, then include_paths.
    let search_dirs =
        std::iter::once(base_dir).chain(opts.include_paths.iter().map(PathBuf::as_path));

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

/// Run deduplication and validation on the fully-expanded text.
fn normalize(text: String) -> Result<String, FlattenError> {
    let lines: Vec<&str> = text.lines().collect();

    // Phase A & B: collect indices of \version and \language lines.
    let version_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_version_line(l))
        .map(|(i, _)| i)
        .collect();

    let language_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_language_line(l))
        .map(|(i, _)| i)
        .collect();

    if version_indices.len() > 1 {
        eprintln!(
            "warning: multiple \\version directives found — keeping last, removing {} earlier occurrence(s)",
            version_indices.len() - 1
        );
    }
    if language_indices.len() > 1 {
        eprintln!(
            "warning: multiple \\language directives found — keeping last, removing {} earlier occurrence(s)",
            language_indices.len() - 1
        );
    }

    // Build the set of line indices to remove (all but the last occurrence).
    let mut remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    if version_indices.len() > 1 {
        for &i in &version_indices[..version_indices.len() - 1] {
            remove.insert(i);
        }
    }
    if language_indices.len() > 1 {
        for &i in &language_indices[..language_indices.len() - 1] {
            remove.insert(i);
        }
    }

    // Phase C: count \header blocks using brace depth tracking.
    let header_count = count_header_blocks(&lines);
    if header_count > 1 {
        return Err(FlattenError::MultipleHeaders);
    }

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

fn is_language_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with(r#"\language ""#)
}

/// Count top-level `\header { ... }` blocks in the flattened output.
/// Uses brace-depth tracking; assumes `\header` is not nested inside another
/// block context (which matches typical LilyPond score structure).
fn count_header_blocks(lines: &[&str]) -> usize {
    let mut count = 0usize;
    let mut depth = 0i32;
    let mut in_header = false;

    for line in lines {
        let trimmed = line.trim_start();

        // Skip line comments
        if trimmed.starts_with('%') {
            continue;
        }

        if !in_header && trimmed.starts_with(r"\header") {
            in_header = true;
            count += 1;
        }

        if in_header {
            for ch in line.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth <= 0 {
                            depth = 0;
                            in_header = false;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    count
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
        let result = flatten_str(src, Path::new("."), FlattenOpts::default()).unwrap();
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
        let result = flatten_str(src, Path::new("."), FlattenOpts::default()).unwrap();
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
        let result = flatten_str(src, Path::new("."), FlattenOpts::default()).unwrap();
        assert_eq!(result.matches(r#"\version"#).count(), 1);
        assert!(result.contains(r#"\version "2.24.0""#));
        assert!(!result.contains(r#"\version "2.22.0""#));
    }

    #[test]
    fn test_duplicate_language_last_wins() {
        let src = r#"\language "english"
{ c' }
\language "deutsch"
{ d' }
"#;
        let result = flatten_str(src, Path::new("."), FlattenOpts::default()).unwrap();
        assert_eq!(result.matches(r#"\language"#).count(), 1);
        assert!(result.contains(r#"\language "deutsch""#));
        assert!(!result.contains(r#"\language "english""#));
    }

    #[test]
    fn test_single_header_ok() {
        let src = r#"\header {
  title = "Test"
}
{ c' }
"#;
        let result = flatten_str(src, Path::new("."), FlattenOpts::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_headers_error() {
        let src = r#"\header { title = "A" }
\header { title = "B" }
{ c' }
"#;
        let err = flatten_str(src, Path::new("."), FlattenOpts::default()).unwrap_err();
        assert!(matches!(err, FlattenError::MultipleHeaders));
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
        let result = flatten_str(src, Path::new("."), FlattenOpts::default());
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(!text.contains("BEGIN INCLUDE"));
    }

    #[test]
    fn test_include_in_block_comment_skipped() {
        let src = "%{\n\\include \"foo.ly\"\n%}\n{ c' }\n";
        let result = flatten_str(src, Path::new("."), FlattenOpts::default());
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(!text.contains("BEGIN INCLUDE"));
    }
}
