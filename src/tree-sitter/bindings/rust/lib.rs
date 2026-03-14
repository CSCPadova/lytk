//! Tree-sitter LilyPond grammar bindings.
//!
//! Adapted from tree-sitter-lilypond/bindings/rust/lib.rs.
//! Provides the compiled grammar for LilyPond and LilyPond Scheme.

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

/// The content of the [`node-types.json`] file for LilyPond.
pub const LILYPOND_NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// The content of the [`node-types.json`] file for LilyPond Scheme.
pub const LILYPOND_SCHEME_NODE_TYPES: &str =
    include_str!("../../lilypond-scheme/src/node-types.json");

pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn test_can_load_lilypond_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE_LILYPOND.into())
            .expect("Error loading LilyPond parser");
    }

    #[test]
    fn test_can_load_lilypond_scheme_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE_LILYPOND_SCHEME.into())
            .expect("Error loading LilyPond Scheme parser");
    }
}
