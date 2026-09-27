//! Syntax diagnostics from the tree alone: what tree-sitter could not parse,
//! and tokens no LilyPond construct accepts where they stand.

use tree_sitter::{Node, Tree};

use crate::diagnostics::{Diagnostic, Severity};
use crate::ir::language::{parse_pitch_name, PitchLanguage};

/// Every syntax error in `tree`, parsed from `source`.
pub(crate) fn syntax_diagnostics(tree: &Tree, source: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let mut descend = true;
        if node.is_error() {
            out.push(
                unclosed(source, node).unwrap_or_else(|| unexpected(source, node, node.end_byte())),
            );
            descend = false;
        } else if node.is_missing() {
            out.push(Diagnostic::at(
                source,
                node,
                Severity::Error,
                "missing-token",
                format!("missing `{}`", node.kind()),
            ));
        } else if node.kind() == "punctuation" && text(source, node) == ">" {
            out.extend(stray_angle(source, node));
        } else if node.kind() == "embedded_scheme" {
            out.extend(after_scheme(source, node));
            out.extend(markup_override(source, node));
        }
        if descend && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

fn text<'a>(source: &'a str, node: Node) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

/// `unexpected `…`` for the text from `node` to byte `end` (its first line,
/// shortened).
fn unexpected(source: &str, node: Node, end: usize) -> Diagnostic {
    let snippet = source.get(node.start_byte()..end).unwrap_or("");
    let snippet = snippet.lines().next().unwrap_or("").trim();
    let short: String = snippet.chars().take(30).collect();
    let message = match (short.is_empty(), short.len() < snippet.len()) {
        (true, _) => "syntax error".to_string(),
        (false, true) => format!("unexpected `{short}…`"),
        (false, false) => format!("unexpected `{short}`"),
    };
    Diagnostic::spanning(source, node, end, Severity::Error, "syntax-error", message)
}

/// An ERROR node holding a bracket or quote it never closes, as tree-sitter
/// makes of `{ c'4 d'4` at the end of the input: `missing `}``, at the opener.
fn unclosed(source: &str, error: Node) -> Option<Diagnostic> {
    let mut open: Vec<(Node, &str)> = Vec::new();
    let mut cursor = error.walk();
    for child in error.children(&mut cursor) {
        let closer = match child.kind() {
            "{" => "}",
            "<<" => ">>",
            "<" => ">",
            "\"" if open.last().is_some_and(|(_, c)| *c == "\"") => {
                open.pop();
                continue;
            }
            "\"" => "\"",
            kind => {
                if open.last().is_some_and(|(_, c)| *c == kind) {
                    open.pop();
                }
                continue;
            }
        };
        open.push((child, closer));
    }
    let (opener, closer) = open.pop()?;
    Some(Diagnostic::at(
        source,
        opener,
        Severity::Error,
        "missing-token",
        format!("missing `{closer}`: `{}` is never closed", opener.kind()),
    ))
}

/// A `>` outside a chord that is not an accent (`->`, `^>`, `_>`): what is
/// left of `<< … >>` without its `<<`, or of a chord without its `<`. The
/// grammar reads it as punctuation.
fn stray_angle(source: &str, node: Node) -> Option<Diagnostic> {
    let is_angle = |n: Option<Node>, adjacent: usize| {
        n.is_some_and(|n| {
            n.kind() == "punctuation" && text(source, n) == ">" && {
                let (a, b) = (n.start_byte(), n.end_byte());
                a == adjacent || b == adjacent
            }
        })
    };
    let prev = previous(node);
    // The second `>` of a `>>`: reported with the first.
    if is_angle(prev, node.start_byte()) {
        return None;
    }
    if prev.is_some_and(|p| matches!(text(source, p), "-" | "^" | "_")) {
        return None;
    }
    let next = node.next_sibling();
    let end = if is_angle(next, node.end_byte()) {
        next.map_or(node.end_byte(), |n| n.end_byte())
    } else {
        node.end_byte()
    };
    let message = if end > node.end_byte() {
        "`>>` without a matching `<<`"
    } else {
        "`>` outside a chord"
    };
    Some(Diagnostic::spanning(
        source,
        node,
        end,
        Severity::Error,
        "syntax-error",
        message.to_string(),
    ))
}

/// What is left of a Scheme expression that lost an opening `(`: `#(foo 1 .
/// 2)` reads as `#foo` and leaves `1 . 2)` to LilyPond. Outside markup, a
/// dotted pair's `.` never follows a Scheme value; nor a parenthesis, right
/// after it or after one more word that is no note, or a fraction
/// (`#string->symbol name)`): a slur follows a note. Unless the value is a
/// command's argument, and the parenthesis maybe the command's post-event
/// (`-\tweak color #red (`, `\vshape #'(…) (`). Nor, at the top level, a
/// number, string or quote on the line of a Scheme expression that starts it
/// (`#set-global-staff-size 20)`).
fn after_scheme(source: &str, node: Node) -> Option<Diagnostic> {
    let mut next = node.next_sibling();
    while next.is_some_and(|n| n.kind() == "comment") {
        next = next.and_then(|n| n.next_sibling());
    }
    let next = next?;
    let (kind, t) = (next.kind(), text(source, next));
    let top = node
        .parent()
        .is_some_and(|p| p.kind() == "lilypond_program");
    let line_start = node.start_byte() - node.start_position().column;
    let starts_line = source
        .get(line_start..node.start_byte())
        .is_some_and(|s| s.trim().is_empty());
    let same_line = next.start_position().row == node.end_position().row;
    // Back over the other arguments to a command (not `\set`/`\override`,
    // whose value follows `=`), or to a post-event's direction mark.
    let post_event = || {
        for p in std::iter::successors(node.prev_sibling(), |p| p.prev_sibling()) {
            match (p.kind(), text(source, p)) {
                ("punctuation", "-" | "^" | "_") | ("escaped_word", _) => return true,
                ("embedded_scheme" | "symbol" | "string" | "comment", _) => {}
                _ => return false,
            }
        }
        false
    };
    let is_paren = |n: Node| n.kind() == "punctuation" && matches!(text(source, n), "(" | ")");
    let no_note = |n: Node| {
        let t = text(source, n);
        n.kind() == "fraction"
            || (n.kind() == "symbol"
                && !matches!(t, "r" | "R" | "s" | "q")
                && PitchLanguage::ALL
                    .iter()
                    .all(|&lang| parse_pitch_name(t, lang).is_none()))
    };
    // `#foo word )`: the parenthesis is the one reported.
    let (next, kind, t) = match next.next_sibling() {
        Some(after) if no_note(next) && is_paren(after) && !post_event() => {
            (after, after.kind(), text(source, after))
        }
        _ => (next, kind, t),
    };
    let stray = (kind == "punctuation" && t == ".")
        || (kind == "punctuation" && matches!(t, "(" | ")") && !post_event())
        || (top
            && starts_line
            && same_line
            && (matches!(
                kind,
                "unsigned_integer" | "decimal_number" | "fraction" | "string"
            ) || (kind == "punctuation" && t == "'")));
    (stray && !in_markup(source, next)).then(|| {
        Diagnostic::at(
            source,
            next,
            Severity::Error,
            "syntax-error",
            format!("unexpected `{t}` after a Scheme expression"),
        )
    })
}

/// `\override #'key` in markup: the command takes a pair (`#'(key . value)`),
/// and a quoted symbol or number never is one.
fn markup_override(source: &str, node: Node) -> Option<Diagnostic> {
    let command = previous(node).filter(|p| text(source, *p) == r"\override")?;
    let datum = text(source, node).strip_prefix('#')?;
    let quoted = datum
        .strip_prefix('\'')
        .or_else(|| datum.strip_prefix('`'))?;
    (!quoted.is_empty() && !quoted.starts_with('(')).then(|| {
        Diagnostic::at(
            source,
            node,
            Severity::Error,
            "syntax-error",
            format!(
                "`{}` takes a pair, like `#'(key . value)`, not `{}`",
                text(source, command),
                text(source, node)
            ),
        )
    })
}

/// Whether `node` stands in markup, where any word or mark is text: it, or a
/// block around it, follows `\markup` through markup commands and their
/// Scheme arguments only (`\markup \hspace #1 .`, `\markup { \line { … } }`).
fn in_markup(source: &str, node: Node) -> bool {
    let mut at = Some(node);
    while let Some(n) = at {
        let mut prev = n.prev_sibling();
        while let Some(p) = prev {
            match p.kind() {
                "escaped_word" if matches!(text(source, p), "\\markup" | "\\markuplist") => {
                    return true;
                }
                "escaped_word" | "embedded_scheme" | "comment" => prev = p.prev_sibling(),
                _ => break,
            }
        }
        at = n.parent();
    }
    false
}

/// The sibling before `node`, skipping comments.
fn previous(node: Node) -> Option<Node> {
    let mut prev = node.prev_sibling();
    while prev.is_some_and(|p| p.kind() == "comment") {
        prev = prev.and_then(|p| p.prev_sibling());
    }
    prev
}
