# Vendored tree-sitter LilyPond grammar

Generated parser sources for [tree-sitter-lilypond][upstream] by Nathan
Whetsell, vendored here so the crate builds without a `tree-sitter` CLI or a
network fetch. `bindings/rust/build.rs` compiles `src/parser.c` and
`lilypond-scheme/src/parser.c`.

MIT licensed — see [`LICENSE`](LICENSE). Do not edit these files by hand; to
update, regenerate them upstream and copy the result in.

[upstream]: https://github.com/nwhetsell/tree-sitter-lilypond
