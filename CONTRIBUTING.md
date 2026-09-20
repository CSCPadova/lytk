# Contributing to lytk

Thanks for your interest. Bug reports with a minimal input file are especially
welcome — most issues in this codebase are format edge cases.

## Setup

```bash
cargo build                  # Rust library + CLI
uv sync                      # Python dev environment
maturin develop              # build the extension module into the venv
```

## Before opening a pull request

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
pytest tests/
```

`pre-commit install` wires up the formatting and clippy checks locally.

## Conventions

- **Tests first.** Every behaviour change needs a test that fails before the fix.
  Round-trip tests assert *semantic* equivalence (pitches, durations, structure),
  never string equality — emitters are free to reformat.
- **Fixtures** live in `tests/fixtures/`. Add the smallest file that reproduces
  the problem.
- **New formats** go through the IR: implement `ToIrAdapter`/`FromIrAdapter` (or
  the Layer-1 `ToMusicAdapter`/`FromMusicAdapter`) rather than converting
  format-to-format directly. See `docs/design.md`.
- **New transforms** implement both `Transform` and `MusicTransform`, and must be
  idempotent and composable.
- The PyO3 bindings live in `src/python.rs` and `src/navigation.rs`; the rest of
  the crate stays pyo3-free so `src/lib.rs` remains a plain Rust root.
  Python-visible changes need a matching entry in `src/lytk/_core.pyi`.
- **Optional dependencies stay optional.** torch and TensorFlow are imported
  inside the method that needs them, never at module scope, so `import lytk`
  works without either. Their tests use `pytest.importorskip`.
- A new input format needs adding to **both** `lytk.cli._SUPPORTED_EXTS` and
  `lytk.datasets.SUPPORTED_EXTENSIONS` — `test_extensions_match_the_cli` enforces
  that they agree.
- `src/tree-sitter/` is vendored generated code — never edit it by hand.

## Releasing

lytk is distributed on **PyPI only**; the Rust crate is the implementation, not a
published artifact.

Maintainers: bump the version in both `Cargo.toml` and `pyproject.toml`, update
`docs/changelog.md`, then push a `v*` tag. `.github/workflows/release.yml` builds
the abi3 wheels and the sdist, gates on the test suite, and publishes to PyPI via
Trusted Publishing.

## Licence

Contributions are accepted under the GPL-2.0-only licence of this project.
