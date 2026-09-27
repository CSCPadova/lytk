//! Text in LilyPond, decoded as LilyPond reads it: strings, Scheme strings,
//! markup, and the fields of `\header` blocks.

use tree_sitter::Node;

fn slice<'a>(source: &'a str, node: Node) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

/// A string node's text, decoded as LilyPond's lexer does
/// (`Lily_lexer::escaped_char`): `\n`, `\t`, `\\`, `\"` and `\'`; any other
/// backslash stays, with the character after it.
pub(crate) fn string_value(source: &str, node: Node) -> String {
    let mut out = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let t = slice(source, child);
        match child.kind() {
            "string_fragment" => out.push_str(t),
            "escape_sequence" => match t {
                "\\n" => out.push('\n'),
                "\\t" => out.push('\t'),
                "\\\\" => out.push('\\'),
                "\\\"" => out.push('"'),
                "\\'" => out.push('\''),
                other => out.push_str(other),
            },
            _ => {}
        }
    }
    out
}

/// The text of a Scheme string literal (`#"…"`), with its common escapes
/// decoded (`\\`, `\"`, `\n`, `\t`, `\r`; others stay); `None` for any other
/// Scheme value.
pub(crate) fn scheme_string(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix('#').unwrap_or(text).trim();
    let inner = inner.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(e @ ('\\' | '"')) => out.push(e),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// The index past a markup expression starting at `children[i]` (just after
/// `\markup`): markup commands with their Scheme arguments, then one block,
/// string or word. It stops at a command that starts a new top-level
/// construct.
pub(crate) fn markup_end(source: &str, children: &[Node], mut i: usize) -> usize {
    while let Some(node) = children.get(i) {
        match node.kind() {
            "escaped_word"
                if !matches!(
                    slice(source, *node),
                    "\\score"
                        | "\\book"
                        | "\\bookpart"
                        | "\\header"
                        | "\\paper"
                        | "\\layout"
                        | "\\midi"
                        | "\\version"
                        | "\\include"
                        | "\\language"
                        | "\\markup"
                        | "\\markuplist"
                        | "\\new"
                        | "\\context"
                        | "\\relative"
                        | "\\transpose"
                ) =>
            {
                i += 1
            }
            "embedded_scheme" => i += 1,
            "expression_block" | "string" | "symbol" => return i + 1,
            _ => return i,
        }
    }
    i
}

/// The plain text of markup: its strings (decoded) and words, with one space
/// where the source has anything between them; commands and their Scheme
/// arguments are left out. `\markup { \bold "J. S." Bach }` is `J. S. Bach`.
pub(crate) fn markup_text(source: &str, nodes: &[Node]) -> String {
    let mut out = String::new();
    let mut last_end = None;
    let mut piece = |out: &mut String, text: &str, start: usize, end: usize| {
        if text.is_empty() {
            return;
        }
        let gap = last_end.is_some_and(|e| e < start);
        if gap && !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push_str(text);
        last_end = Some(end);
    };
    for &root in nodes {
        let mut cursor = root.walk();
        'walk: loop {
            let n = cursor.node();
            let descend = match n.kind() {
                "string" => {
                    piece(
                        &mut out,
                        &string_value(source, n),
                        n.start_byte(),
                        n.end_byte(),
                    );
                    false
                }
                "escaped_word" | "embedded_scheme" | "comment" => false,
                "{" | "}" => false,
                _ if n.child_count() == 0 => {
                    piece(&mut out, slice(source, n), n.start_byte(), n.end_byte());
                    false
                }
                _ => true,
            };
            if descend && cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.node() == root {
                    break 'walk;
                }
                if cursor.goto_next_sibling() {
                    continue 'walk;
                }
                if !cursor.goto_parent() {
                    break 'walk;
                }
            }
        }
    }
    out.trim().to_string()
}

/// A field of a `\header` block: its key, its value as text, the byte range
/// of `key = value`, and the `\score` block it belongs to (by the order of
/// `\score` blocks in the file), if any.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderField {
    pub key: String,
    pub value: String,
    pub start: usize,
    pub end: usize,
    pub score: Option<usize>,
}

/// The fields of a `\header { … }` block, in order: a string value decoded,
/// a `\markup` value as its plain text, `#"…"` as its string. Fields with
/// other values (`##f`, Scheme) are left out.
pub(crate) fn header_block_fields(source: &str, block: Node) -> Vec<HeaderField> {
    let mut cursor = block.walk();
    let children: Vec<Node> = block
        .children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < children.len() {
        let n = children[i];
        let is_eq = |k: usize| {
            children
                .get(k)
                .is_some_and(|e| e.kind() == "punctuation" && slice(source, *e) == "=")
        };
        let key = match n.kind() {
            "assignment_lhs" if is_eq(i + 1) => slice(source, n).trim().to_string(),
            // A quoted key: `"key" = …`.
            "string" if is_eq(i + 1) => string_value(source, n),
            _ => {
                i += 1;
                continue;
            }
        };
        let j = i + 2;
        let Some(&v) = children.get(j) else {
            break;
        };
        let (value, end) = match v.kind() {
            "string" => (Some(string_value(source, v)), j + 1),
            "embedded_scheme" => (scheme_string(slice(source, v)), j + 1),
            "escaped_word" if matches!(slice(source, v), "\\markup" | "\\markuplist") => {
                let end = markup_end(source, &children, j + 1);
                (Some(markup_text(source, &children[j + 1..end])), end)
            }
            _ => (None, j + 1),
        };
        if let Some(value) = value {
            out.push(HeaderField {
                key,
                value,
                start: n.start_byte(),
                end: children[end - 1].end_byte(),
                score: None,
            });
        }
        i = end;
    }
    out
}

/// Every `\header` field of a syntax tree, in source order, each with the
/// `\score` block it is in.
pub(crate) fn header_fields_of(source: &str, root: Node) -> Vec<HeaderField> {
    let mut scores: Vec<(usize, usize)> = Vec::new();
    let mut out = Vec::new();
    let mut cursor = root.walk();
    'walk: loop {
        let n = cursor.node();
        if n.kind() == "escaped_word" {
            let mut next = n.next_sibling();
            while next.is_some_and(|x| x.kind() == "comment") {
                next = next.and_then(|x| x.next_sibling());
            }
            if let Some(block) = next.filter(|b| b.kind() == "expression_block") {
                match slice(source, n) {
                    "\\score" => scores.push((block.start_byte(), block.end_byte())),
                    "\\header" => {
                        let at = n.start_byte();
                        let score = scores.iter().rposition(|&(s, e)| s <= at && at < e);
                        out.extend(
                            header_block_fields(source, block)
                                .into_iter()
                                .map(|f| HeaderField { score, ..f }),
                        );
                    }
                    _ => {}
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                continue 'walk;
            }
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    out
}
