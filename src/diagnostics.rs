//! Diagnostics: what a reader found wrong with its input, or could not read.
//!
//! The LilyPond reader reports syntax errors from the tree, and what its walk
//! drops or cannot represent. An error is input LilyPond itself rejects; a
//! warning is input lytk does not read (an unknown command, an include it does
//! not follow) or cannot represent, and reads around.

use std::fmt;

use tree_sitter::Node;

/// How serious a [`Diagnostic`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// LilyPond rejects the input.
    Error,
    /// lytk does not read, or cannot represent, this part of the input.
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One finding about the input, located in the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable identifier, e.g. `missing-token` or `unknown-command`.
    pub code: &'static str,
    pub message: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column, counted in characters.
    pub column: usize,
    /// Byte range in the source.
    pub start: usize,
    pub end: usize,
}

impl Diagnostic {
    /// A diagnostic covering `node` of `source`.
    pub fn at(
        source: &str,
        node: Node,
        severity: Severity,
        code: &'static str,
        message: String,
    ) -> Self {
        Self::spanning(source, node, node.end_byte(), severity, code, message)
    }

    /// A diagnostic from the start of `node` to byte `end`.
    pub fn spanning(
        source: &str,
        node: Node,
        end: usize,
        severity: Severity,
        code: &'static str,
        message: String,
    ) -> Self {
        let mut columns = Columns::default();
        Self::counted(&mut columns, source, node, end, severity, code, message)
    }

    /// [`spanning`](Self::spanning), counting its column with `columns`.
    pub(crate) fn counted(
        columns: &mut Columns,
        source: &str,
        node: Node,
        end: usize,
        severity: Severity,
        code: &'static str,
        message: String,
    ) -> Self {
        let start = node.start_byte();
        let point = node.start_position();
        let column = columns.column(source, start - point.column, start);
        Self {
            severity,
            code,
            message,
            line: point.row + 1,
            column,
            start,
            end,
        }
    }

    /// A diagnostic about the input as a whole, at its start.
    pub fn whole(severity: Severity, code: &'static str, message: String) -> Self {
        Self {
            severity,
            code,
            message,
            line: 1,
            column: 1,
            start: 0,
            end: 0,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Character columns of byte offsets, counted on from the last one on the
/// same line: diagnostics come mostly in source order, and counting each from
/// its line's start would be quadratic on a long line full of them.
#[derive(Debug, Default)]
pub(crate) struct Columns {
    line_start: usize,
    byte: usize,
    chars: usize,
}

impl Columns {
    /// The 1-based character column of byte `start` on the line starting at
    /// byte `line_start`.
    pub(crate) fn column(&mut self, source: &str, line_start: usize, start: usize) -> usize {
        if line_start != self.line_start || start < self.byte {
            (self.line_start, self.byte, self.chars) = (line_start, line_start, 0);
        }
        self.chars += source
            .get(self.byte..start)
            .map_or(start - self.byte, |s| s.chars().count());
        self.byte = start;
        self.chars + 1
    }
}

impl fmt::Display for Diagnostic {
    /// `3:12: error: missing '}' [missing-token]`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: {} [{}]",
            self.line,
            self.column,
            self.severity.as_str(),
            self.message,
            self.code
        )
    }
}

/// Sort diagnostics by position and drop repeats (the reader walks some music
/// twice, e.g. a variable used inside `\relative`).
pub(crate) fn tidy(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        (a.start, a.end, a.severity, a.code).cmp(&(b.start, b.end, b.severity, b.code))
    });
    diagnostics.dedup_by(|a, b| (a.start, a.end, a.code) == (b.start, b.end, b.code));
}
