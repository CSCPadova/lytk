# Vendored tree-sitter LilyPond grammar

Generated parser sources for [tree-sitter-lilypond][upstream] by Nathan
Whetsell, vendored here so the crate builds without a `tree-sitter` CLI or a
network fetch. `bindings/rust/build.rs` compiles `src/parser.c` and
`lilypond-scheme/src/parser.c`.

MIT licensed — see [`LICENSE`](LICENSE). Do not edit these files by hand; to
update, regenerate them upstream and copy the result in.

Vendored from upstream commit
[`b3b38a6`](https://github.com/nwhetsell/tree-sitter-lilypond/commit/b3b38a6645255115048d6a1069b070811b477249)
(2025-12-12, parsers generated with Tree-sitter 0.26.2): `lilypond/src` →
`src/`, `lilypond-scheme/src` → `lilypond-scheme/src/`. Upstream's grammar has
not changed since (checked 2026-09-27 against `3d999a1`, a regeneration with
Tree-sitter 0.27.0).

[upstream]: https://github.com/nwhetsell/tree-sitter-lilypond
