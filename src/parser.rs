//! LilyPond parser using tree-sitter.
//!
//! Wraps the tree-sitter LilyPond grammar to produce a concrete syntax tree (CST)
//! from LilyPond source text. The CST can then be converted to the lytk IR.

use tree_sitter::{Language, Parser, Tree};

/// Maximum tree-sitter syntax-tree depth we will walk. The IR walk that consumes
/// the tree is deeply recursive (one frame per `{ }` / `<< >>` nesting level), so
/// pathologically nested input — easy to generate, e.g. `{{{ … }}}` thousands
/// deep — would overflow the stack and **abort the process** (a stack overflow is
/// not a catchable panic). We reject such input up front with a normal error
/// instead. Real scores nest only a few dozen levels; this bound (tree depth, a
/// small multiple of brace-nesting) is astronomically above any genuine score
/// yet well below the recursion ceiling.
pub const MAX_NESTING_DEPTH: usize = 2000;

/// Errors that can occur during parsing.
#[derive(Debug)]
pub enum ParseError {
    /// tree-sitter failed to set the language grammar.
    LanguageError(String),
    /// tree-sitter returned `None` from `parse()`.
    ParseFailed,
    /// The syntax tree is nested deeper than [`MAX_NESTING_DEPTH`]; walking it
    /// would risk a stack-overflow abort, so we refuse it.
    TooDeeplyNested(usize),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LanguageError(msg) => write!(f, "language error: {msg}"),
            Self::ParseFailed => write!(f, "tree-sitter parse returned None"),
            Self::TooDeeplyNested(limit) => {
                write!(f, "input nested deeper than the limit of {limit} levels")
            }
        }
    }
}

/// Iteratively (no recursion) test whether `tree`'s maximum node depth exceeds
/// `limit`. Returns `true` as soon as the bound is crossed, so it is cheap on
/// adversarial input.
fn tree_depth_exceeds(tree: &Tree, limit: usize) -> bool {
    let mut cursor = tree.walk();
    let mut depth: usize = 0;
    loop {
        if cursor.goto_first_child() {
            depth += 1;
            if depth > limit {
                return true;
            }
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return false; // returned to the root: whole tree within bound
            }
            depth -= 1;
        }
    }
}

impl std::error::Error for ParseError {}

/// A thin wrapper around tree-sitter's [`Parser`] pre-loaded with the LilyPond grammar.
///
/// # Example
/// ```
/// use _core::parser::LilyPondParser;
///
/// let mut parser = LilyPondParser::new().unwrap();
/// let tree = parser.parse(r#"{ c'4 d' e' f' }"#).unwrap();
/// let root = tree.root_node();
/// assert_eq!(root.kind(), "lilypond_program");
/// ```
pub struct LilyPondParser {
    parser: Parser,
}

impl LilyPondParser {
    /// Create a new parser loaded with the LilyPond grammar.
    pub fn new() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_lilypond::LANGUAGE_LILYPOND.into();
        parser
            .set_language(&language)
            .map_err(|e| ParseError::LanguageError(e.to_string()))?;
        Ok(Self { parser })
    }

    /// Parse LilyPond source text into a tree-sitter [`Tree`].
    ///
    /// Rejects input nested deeper than [`MAX_NESTING_DEPTH`] (which would risk a
    /// stack-overflow abort during the IR walk) with [`ParseError::TooDeeplyNested`].
    pub fn parse(&mut self, source: &str) -> Result<Tree, ParseError> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or(ParseError::ParseFailed)?;
        if tree_depth_exceeds(&tree, MAX_NESTING_DEPTH) {
            return Err(ParseError::TooDeeplyNested(MAX_NESTING_DEPTH));
        }
        Ok(tree)
    }

    /// Parse with an existing tree for incremental re-parsing.
    pub fn parse_with_old_tree(
        &mut self,
        source: &str,
        old_tree: &Tree,
    ) -> Result<Tree, ParseError> {
        self.parser
            .parse(source, Some(old_tree))
            .ok_or(ParseError::ParseFailed)
    }
}

/// Re-export the grammar bindings so other modules can access node types.
pub mod grammar {
    pub use super::tree_sitter_lilypond::*;
}

/// Module for the tree-sitter grammar bindings (path-adapted for this project).
mod tree_sitter_lilypond {
    use tree_sitter_language::LanguageFn;

    extern "C" {
        fn tree_sitter_lilypond() -> *const ();
        fn tree_sitter_lilypond_scheme() -> *const ();
    }

    /// The tree-sitter [`LanguageFn`] for LilyPond.
    pub const LANGUAGE_LILYPOND: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_lilypond) };

    /// The tree-sitter [`LanguageFn`] for LilyPond Scheme.
    pub const LANGUAGE_LILYPOND_SCHEME: LanguageFn =
        unsafe { LanguageFn::from_raw(tree_sitter_lilypond_scheme) };

    pub const LILYPOND_NODE_TYPES: &str = include_str!("tree-sitter/src/node-types.json");

    pub const HIGHLIGHTS_QUERY: &str = include_str!("tree-sitter/queries/highlights.scm");

    pub const INJECTIONS_QUERY: &str = include_str!("tree-sitter/queries/injections.scm");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_melody() {
        let mut parser = LilyPondParser::new().unwrap();
        let source = r#"{ c'4 d' e' f' }"#;
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), "lilypond_program");
        assert!(!root.has_error());
    }

    #[test]
    fn test_deeply_nested_input_is_rejected_not_overflowed() {
        // Pathologically nested braces would overflow the IR walk's stack and
        // abort the process; the depth guard must turn it into a clean error.
        let mut parser = LilyPondParser::new().unwrap();
        let deep = format!("{}{}", "{ ".repeat(8000), " }".repeat(8000));
        assert!(matches!(
            parser.parse(&deep),
            Err(ParseError::TooDeeplyNested(_))
        ));
        // A normally-nested score is unaffected.
        let shallow = "{ << { c'4 d' } \\\\ { e'4 f' } >> }";
        assert!(parser.parse(shallow).is_ok());
    }

    #[test]
    fn test_parse_empty() {
        let mut parser = LilyPondParser::new().unwrap();
        let tree = parser.parse("").unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), "lilypond_program");
    }

    #[test]
    fn test_parse_with_version() {
        let mut parser = LilyPondParser::new().unwrap();
        let source = r#"\version "2.24.0"
{ c'4 d'4 e'4 f'4 }"#;
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), "lilypond_program");
        assert!(!root.has_error());
    }

    #[test]
    fn test_grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        let language: Language = tree_sitter_lilypond::LANGUAGE_LILYPOND.into();
        parser.set_language(&language).unwrap();
    }
}
