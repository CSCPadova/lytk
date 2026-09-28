//! Tokens of LilyPond text: the syntax tree's leaves, with strings, comments
//! and quoted identifiers kept whole. Embedded Scheme is tokenized as Scheme,
//! and LilyPond embedded in it (`#{ … #}`) as LilyPond.

use tree_sitter::Node;

use crate::diagnostics::Columns;
use crate::parser::LilyPondParser;

use super::{AdapterError, Result};

/// What a [`Token`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// `% …` or `%{ … %}`; in Scheme `; …`, `#| … |#` or `#;datum`.
    Comment,
    /// `"…"`, whole.
    String,
    /// The `#`, `$`, `#@` or `$@` that starts an embedded Scheme expression,
    /// whose tokens follow.
    Scheme,
    /// A backslashed word or mark: `\relative`, `\!`, `\(`, `\1`.
    Command,
    /// A word: a note name, a context, a lyric.
    Symbol,
    /// `4`, `2.5`.
    Number,
    /// `3/4`.
    Fraction,
    /// Brackets, octave marks, articulations and the other marks; Scheme's
    /// brackets and quotes.
    Punctuation,
    /// Scheme's `#t`, `#f`, `#true`, `#false`.
    Boolean,
    /// A Scheme character: `#\a`, `#\space`.
    Character,
    /// A Scheme keyword: `#:key`.
    Keyword,
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
            TokenKind::Boolean => "boolean",
            TokenKind::Character => "character",
            TokenKind::Keyword => "keyword",
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
    /// Whether the token is Scheme (embedded Scheme, its `#` or `$`
    /// included) rather than LilyPond.
    pub scheme: bool,
}

/// The kind of token `node` is, if it is one (a node the walk does not
/// descend into); `None` for a node made of tokens.
fn kind_of(node: Node, text: &str) -> Option<TokenKind> {
    let kind = match node.kind() {
        "comment" | "scheme_comment" => TokenKind::Comment,
        "string" | "scheme_string" => TokenKind::String,
        "embedded_scheme_prefix" => TokenKind::Scheme,
        "quoted_identifier"
        | "escaped_word"
        | "dynamic"
        | "ligature"
        | "instrument_string_number" => TokenKind::Command,
        "symbol" | "scheme_symbol" => TokenKind::Symbol,
        "unsigned_integer" | "decimal_number" | "scheme_number" => TokenKind::Number,
        "fraction" => TokenKind::Fraction,
        "scheme_boolean" => TokenKind::Boolean,
        "scheme_character" => TokenKind::Character,
        "scheme_keyword" => TokenKind::Keyword,
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
    // Whether each ancestor of the current node is Scheme.
    let mut parents: Vec<bool> = Vec::new();
    loop {
        let node = cursor.node();
        let scheme = match node.kind() {
            "embedded_scheme" => true,
            "scheme_embedded_lilypond_text" => false,
            _ => parents.last().copied().unwrap_or(false),
        };
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
                scheme,
            });
        } else if cursor.goto_first_child() {
            parents.push(scheme);
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(out);
            }
            parents.pop();
        }
    }
}

/// `text` without its comments, LilyPond's and embedded Scheme's. A block
/// comment between two tokens becomes a space, as LilyPond reads it; a line
/// comment leaves its line break.
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
    fn scheme_is_tokenized_as_scheme() {
        let src = "#(define x \"a\") ##f $y #(list #\\a #:k 'q ; c\n #{ c'4 #})";
        let tokens: Vec<(&str, &str, bool)> = tokenize(src)
            .unwrap()
            .into_iter()
            .map(|t| (t.kind.as_str(), &src[t.start..t.end], t.scheme))
            .collect();
        assert_eq!(
            tokens,
            [
                ("scheme", "#", true),
                ("punctuation", "(", true),
                ("symbol", "define", true),
                ("symbol", "x", true),
                ("string", "\"a\"", true),
                ("punctuation", ")", true),
                ("scheme", "#", true),
                ("boolean", "#f", true),
                ("scheme", "$", true),
                ("symbol", "y", true),
                ("scheme", "#", true),
                ("punctuation", "(", true),
                ("symbol", "list", true),
                ("character", "#\\a", true),
                ("keyword", "#:k", true),
                ("punctuation", "'", true),
                ("symbol", "q", true),
                ("comment", "; c", true),
                ("punctuation", "#{", true),
                ("symbol", "c", false),
                ("punctuation", "'", false),
                ("number", "4", false),
                ("punctuation", "#}", true),
                ("punctuation", ")", true),
            ]
        );
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
                ("scheme", "#"),
                ("punctuation", "("),
                ("symbol", "define"),
                ("symbol", "x"),
                ("number", "1"),
                ("punctuation", ")"),
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
            (32, 3, 13, 2, 1, 0)
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
        let src = "c4%{x%}d4 % tail\n#(display \"%\" ; s\n#|b|#1)\n";
        assert_eq!(
            strip_comments(src).unwrap(),
            "c4 d4 \n#(display \"%\" \n1)\n"
        );
    }
}
