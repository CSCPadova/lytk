//! Tokens of LilyPond text: the syntax tree's leaves, with strings, embedded
//! Scheme and quoted identifiers kept whole.

use tree_sitter::Node;

use crate::diagnostics::Columns;
use crate::parser::LilyPondParser;

use super::{AdapterError, Result};

/// What a [`Token`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// `% …` or `%{ … %}`.
    Comment,
    /// `"…"`, whole.
    String,
    /// An embedded Scheme expression (`#…`, `$…`), whole.
    Scheme,
    /// A backslashed word or mark: `\relative`, `\!`, `\(`, `\1`.
    Command,
    /// A word: a note name, a context, a lyric.
    Symbol,
    /// `4`, `2.5`.
    Number,
    /// `3/4`.
    Fraction,
    /// Brackets, octave marks, articulations and the other marks.
    Punctuation,
    /// Text the grammar cannot make a token of.
    Error,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Comment => "comment",
            TokenKind::String => "string",
            TokenKind::Scheme => "scheme",
            TokenKind::Command => "command",
            TokenKind::Symbol => "symbol",
            TokenKind::Number => "number",
            TokenKind::Fraction => "fraction",
            TokenKind::Punctuation => "punctuation",
            TokenKind::Error => "error",
        }
    }
}

/// A token of LilyPond text: `text[start..end]` (bytes), at a 1-based line
/// and character column.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

/// The kind of token `node` is, if it is one (a node the walk does not
/// descend into); `None` for a node made of tokens.
fn kind_of(node: Node, text: &str) -> Option<TokenKind> {
    let kind = match node.kind() {
        "comment" => TokenKind::Comment,
        "string" => TokenKind::String,
        "embedded_scheme" => TokenKind::Scheme,
        "quoted_identifier"
        | "escaped_word"
        | "dynamic"
        | "ligature"
        | "instrument_string_number" => TokenKind::Command,
        "symbol" => TokenKind::Symbol,
        "unsigned_integer" | "decimal_number" => TokenKind::Number,
        "fraction" => TokenKind::Fraction,
        _ if node.child_count() > 0 => return None,
        _ if node.is_error() => TokenKind::Error,
        _ if text.starts_with('\\') => TokenKind::Command,
        _ => TokenKind::Punctuation,
    };
    Some(kind)
}

fn parse(text: &str) -> Result<tree_sitter::Tree> {
    LilyPondParser::new()
        .and_then(|mut p| p.parse(text))
        .map_err(|e| AdapterError::Parse(e.to_string()))
}

/// Every token of `text`, in order. Whitespace is no token; every other
/// character is in exactly one.
pub fn tokenize(text: &str) -> Result<Vec<Token>> {
    let tree = parse(text)?;
    let mut out = Vec::new();
    let mut columns = Columns::default();
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let (start, end) = (node.start_byte(), node.end_byte());
        let kind = if node.is_missing() || start == end {
            None
        } else {
            kind_of(node, text.get(start..end).unwrap_or(""))
        };
        if let Some(kind) = kind {
            let point = node.start_position();
            out.push(Token {
                kind,
                start,
                end,
                line: point.row + 1,
                column: columns.column(text, start - point.column, start),
            });
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(out);
            }
        }
    }
}

/// `text` without its LilyPond comments. A block comment between two tokens
/// becomes a space, as LilyPond reads it; a line comment leaves its line
/// break. Comments inside embedded Scheme are Scheme's, and stay.
pub fn strip_comments(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for token in tokenize(text)? {
        if token.kind != TokenKind::Comment {
            continue;
        }
        out.push_str(&text[at..token.start]);
        let glued = |c: Option<char>| c.is_some_and(|c| !c.is_whitespace());
        if glued(text[..token.start].chars().next_back()) && glued(text[token.end..].chars().next())
        {
            out.push(' ');
        }
        at = token.end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

/// Counts of LilyPond source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStats {
    pub bytes: usize,
    pub lines: usize,
    pub tokens: usize,
    pub comments: usize,
    /// Embedded Scheme expressions.
    pub scheme: usize,
    /// Text the grammar cannot tokenize.
    pub error_tokens: usize,
}

/// The counts of `text`, from its tokens.
pub fn source_stats(text: &str) -> Result<SourceStats> {
    let tokens = tokenize(text)?;
    let count = |kind| tokens.iter().filter(|t| t.kind == kind).count();
    Ok(SourceStats {
        bytes: text.len(),
        lines: text.lines().count(),
        tokens: tokens.len(),
        comments: count(TokenKind::Comment),
        scheme: count(TokenKind::Scheme),
        error_tokens: count(TokenKind::Error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&'static str, &str)> {
        tokenize(text)
            .unwrap()
            .into_iter()
            .map(|t| (t.kind.as_str(), &text[t.start..t.end]))
            .collect()
    }

    #[test]
    fn leaves_become_tokens() {
        assert_eq!(
            kinds("\\relative c'' { \\time 3/4 c4.\\p( d) % end\n}"),
            [
                ("command", "\\relative"),
                ("symbol", "c"),
                ("punctuation", "'"),
                ("punctuation", "'"),
                ("punctuation", "{"),
                ("command", "\\time"),
                ("fraction", "3/4"),
                ("symbol", "c"),
                ("number", "4"),
                ("punctuation", "."),
                ("command", "\\p"),
                ("punctuation", "("),
                ("symbol", "d"),
                ("punctuation", ")"),
                ("comment", "% end"),
                ("punctuation", "}"),
            ]
        );
        assert_eq!(
            kinds("t = \"a \\\"b\" #(define x 1) \\< \\!"),
            [
                ("symbol", "t"),
                ("punctuation", "="),
                ("string", "\"a \\\"b\""),
                ("scheme", "#(define x 1)"),
                ("command", "\\<"),
                ("command", "\\!"),
            ]
        );
    }

    #[test]
    fn source_stats_count_tokens_by_kind() {
        let s = source_stats("% a\n#(define x 1)\n{ c'4 %{b%} }\n").unwrap();
        assert_eq!(
            (
                s.bytes,
                s.lines,
                s.tokens,
                s.comments,
                s.scheme,
                s.error_tokens
            ),
            (32, 3, 8, 2, 1, 0)
        );
    }

    #[test]
    fn positions_count_characters() {
        let t = tokenize("é %{x%}\n  d").unwrap();
        assert_eq!(
            t.iter().map(|t| (t.line, t.column)).collect::<Vec<_>>(),
            [(1, 1), (1, 3), (2, 3)]
        );
    }

    #[test]
    fn comments_are_stripped_as_lilypond_skips_them() {
        let src = "c4%{x%}d4 % tail\n#(display \"%\" ; s\n)\n";
        assert_eq!(
            strip_comments(src).unwrap(),
            "c4 d4 \n#(display \"%\" ; s\n)\n"
        );
    }
}
