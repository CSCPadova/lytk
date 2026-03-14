//! LilyPond parser using tree-sitter.
//!
//! Wraps the tree-sitter LilyPond grammar to produce a concrete syntax tree (CST)
//! from LilyPond source text. The CST can then be converted to the lytk IR.

use tree_sitter::{Language, Parser, Tree};

/// Errors that can occur during parsing.
#[derive(Debug)]
pub enum ParseError {
    /// tree-sitter failed to set the language grammar.
    LanguageError(String),
    /// tree-sitter returned `None` from `parse()`.
    ParseFailed,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LanguageError(msg) => write!(f, "language error: {msg}"),
            Self::ParseFailed => write!(f, "tree-sitter parse returned None"),
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
    pub fn parse(&mut self, source: &str) -> Result<Tree, ParseError> {
        self.parser.parse(source, None).ok_or(ParseError::ParseFailed)
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
    pub const LANGUAGE_LILYPOND: LanguageFn =
        unsafe { LanguageFn::from_raw(tree_sitter_lilypond) };

    /// The tree-sitter [`LanguageFn`] for LilyPond Scheme.
    pub const LANGUAGE_LILYPOND_SCHEME: LanguageFn =
        unsafe { LanguageFn::from_raw(tree_sitter_lilypond_scheme) };

    pub const LILYPOND_NODE_TYPES: &str =
        include_str!("tree-sitter/src/node-types.json");

    pub const HIGHLIGHTS_QUERY: &str =
        include_str!("tree-sitter/queries/highlights.scm");

    pub const INJECTIONS_QUERY: &str =
        include_str!("tree-sitter/queries/injections.scm");
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
