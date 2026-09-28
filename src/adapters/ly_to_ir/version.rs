//! `\version`: the LilyPond version a file states, read and edited on the
//! syntax tree, so a commented-out statement does not count.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use tree_sitter::Node;

use crate::parser::LilyPondParser;

use super::{AdapterError, Result};

/// A LilyPond version, compared numerically (`2.24` is `2.24.0`). Accepted as
/// LilyPond 2.24 accepts it (`parse-lily-version` in `scm/lily-library.scm`):
/// `major.minor.patch`, with an optional fourth part (any text, which is not
/// compared), or `major.minor` for a stable series (an even minor). The
/// numbers are ASCII digits.
#[derive(Debug, Clone)]
pub struct LilyPondVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub extra: Option<String>,
}

impl LilyPondVersion {
    fn key(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }
}

impl FromStr for LilyPondVersion {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let invalid = || format!("invalid version string \"{s}\"");
        let number = |p: &str| {
            p.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| p.parse::<u32>().ok())
                .flatten()
                .ok_or_else(invalid)
        };
        match s.split('.').collect::<Vec<_>>()[..] {
            [major, minor] => {
                let (major, minor) = (number(major)?, number(minor)?);
                if minor % 2 == 1 {
                    return Err(format!(
                        "version \"{s}\" leaves out the third number, which only a stable \
                         release (an even second number) may"
                    ));
                }
                Ok(Self {
                    major,
                    minor,
                    patch: 0,
                    extra: None,
                })
            }
            [major, minor, patch, ref extra @ ..] if extra.len() <= 1 => Ok(Self {
                major: number(major)?,
                minor: number(minor)?,
                patch: number(patch)?,
                extra: extra.first().map(|e| e.to_string()),
            }),
            _ => Err(invalid()),
        }
    }
}

impl fmt::Display for LilyPondVersion {
    /// `2.24.0`, the fourth part after a dot when there is one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match &self.extra {
            Some(extra) => write!(f, ".{extra}"),
            None => Ok(()),
        }
    }
}

impl PartialEq for LilyPondVersion {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for LilyPondVersion {}

impl Hash for LilyPondVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

impl PartialOrd for LilyPondVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LilyPondVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

/// A `\version` statement: the command, and the string after it when one
/// follows directly (LilyPond allows only whitespace between them).
pub(crate) struct Statement<'t> {
    pub word: Node<'t>,
    pub string: Option<Node<'t>>,
}

impl Statement<'_> {
    /// The text between the quotes, as LilyPond's lexer takes it (no escapes).
    pub fn value<'s>(&self, source: &'s str) -> Option<&'s str> {
        let raw = source.get(self.string?.byte_range())?;
        raw.strip_prefix('"')
            .map(|r| r.strip_suffix('"').unwrap_or(r))
    }

    fn end(&self) -> usize {
        self.string.unwrap_or(self.word).end_byte()
    }
}

/// Whether `node` is a `\version` command.
pub(crate) fn is_statement(source: &str, node: Node) -> bool {
    node.kind() == "escaped_word" && source.get(node.byte_range()) == Some("\\version")
}

/// The `\version` statement `word` starts.
pub(crate) fn statement<'t>(word: Node<'t>) -> Statement<'t> {
    let string = word.next_sibling().filter(|n| n.kind() == "string");
    Statement { word, string }
}

/// Every `\version` statement under `root`, in source order.
fn statements<'t>(source: &str, root: Node<'t>) -> Vec<Statement<'t>> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if is_statement(source, node) {
            out.push(statement(node));
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// The version the first `\version` statement under `root` states, if valid.
pub(crate) fn version_of(source: &str, root: Node) -> Option<LilyPondVersion> {
    statements(source, root)
        .first()?
        .value(source)?
        .parse()
        .ok()
}

fn parse(text: &str) -> Result<tree_sitter::Tree> {
    LilyPondParser::new()
        .and_then(|mut p| p.parse(text))
        .map_err(|e| AdapterError::Parse(e.to_string()))
}

/// The version the first `\version` statement of `text` states: `None` when
/// there is none, or when it is not a valid version (`lytk check` reports
/// it).
pub fn lilypond_version(text: &str) -> Option<LilyPondVersion> {
    let tree = parse(text).ok()?;
    version_of(text, tree.root_node())
}

/// `text` with every `\version` statement stating `version`, or with one
/// added at the top (after a byte-order mark) when there is none.
pub fn set_lilypond_version(text: &str, version: &LilyPondVersion) -> Result<String> {
    let statement = format!("\\version \"{version}\"");
    let tree = parse(text)?;
    let found = statements(text, tree.root_node());
    if found.is_empty() {
        let (bom, rest) = match text.strip_prefix('\u{feff}') {
            Some(rest) => ("\u{feff}", rest),
            None => ("", text),
        };
        return Ok(format!("{bom}{statement}\n{rest}"));
    }
    let mut out = text.to_string();
    for s in found.iter().rev() {
        out.replace_range(s.word.start_byte()..s.end(), &statement);
    }
    Ok(out)
}

/// `text` without its `\version` statements. A statement alone on its line
/// takes the line with it.
pub fn strip_lilypond_version(text: &str) -> Result<String> {
    let tree = parse(text)?;
    let mut out = text.to_string();
    for s in statements(text, tree.root_node()).iter().rev() {
        let (mut start, mut end) = (s.word.start_byte(), s.end());
        let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
        let rest = &text[end..];
        let trailing = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let after = &rest[trailing..];
        let newline = [("\r\n", 2), ("\n", 1)]
            .into_iter()
            .find(|(nl, _)| after.starts_with(nl))
            .map(|(_, n)| n);
        if text[line_start..start].trim().is_empty() && (newline.is_some() || after.is_empty()) {
            start = line_start;
            end += trailing + newline.unwrap_or(0);
        }
        out.replace_range(start..end, "");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> LilyPondVersion {
        s.parse().unwrap()
    }

    #[test]
    fn versions_parse_and_compare_as_lilypond_does() {
        assert_eq!(v("2.24"), v("2.24.0"));
        assert!(v("2.24.0") < v("2.24.1") && v("2.24.10") > v("2.24.9"));
        assert_eq!(v("2.25.3.foo").to_string(), "2.25.3.foo");
        assert_eq!(v("2.25.3.foo"), v("2.25.3"));
        for bad in [
            "2.25",
            "2",
            "2.x.0",
            "",
            "2..0",
            "2.24.0.a.b",
            " 2.24.0",
            "-2.24.0",
        ] {
            assert!(bad.parse::<LilyPondVersion>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn statements_are_found_on_the_tree_and_edited() {
        let src = "% \\version \"1.0.0\"\n\\version \"2.24.0\"\n{ c'4 }\n";
        assert_eq!(lilypond_version(src), Some(v("2.24.0")));
        assert_eq!(lilypond_version("{ c'4 }"), None);
        assert_eq!(lilypond_version("\\version \"2.x\" { c'4 }"), None);
        assert_eq!(
            set_lilypond_version(src, &v("2.26")).unwrap(),
            "% \\version \"1.0.0\"\n\\version \"2.26.0\"\n{ c'4 }\n"
        );
        assert_eq!(
            set_lilypond_version("\u{feff}{ c'4 }", &v("2.24.0")).unwrap(),
            "\u{feff}\\version \"2.24.0\"\n{ c'4 }"
        );
        assert_eq!(
            strip_lilypond_version(src).unwrap(),
            "% \\version \"1.0.0\"\n{ c'4 }\n"
        );
        assert_eq!(
            strip_lilypond_version("{ c'4 } \\version \"2.24.0\" % x").unwrap(),
            "{ c'4 }  % x"
        );
    }
}
