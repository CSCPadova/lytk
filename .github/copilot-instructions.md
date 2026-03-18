# lytk — Copilot Workspace Instructions

## Project Overview

`lytk` is a **Rust library with Python bindings** for large-scale music notation data augmentation. It processes LilyPond, MusicXML, MIDI, and other symbolic music formats through a shared Internal Representation (IR), applying transformations for dataset creation and augmentation.

**Build toolchain:** `maturin` + `pyo3` for the Rust/Python bridge; `uv` for Python dependency management.

Latest changes: look at the file docs/changelog.md to know about latest activity on the codebase.

**Development plan and roadmap:** See [`docs/roadmap.md`](docs/roadmap.md) for the full epic/task breakdown, implementation sequence, and key decisions. After an epic/task has been completed update the roadmap file and write the latest changes to the [changelog file](docs/changelog.md), documenting what has been done and what to do next.

---

## Directory Layout

| Path | What it is |
|---|---|
| `src/lib.rs`, `src/` | **Rust project** — the actual codebase being built. |
| `src/lytk/` | **Python package** installed by maturin as `import lytk`. Contains `__init__.py`, `_core.pyi`, and any pure-Python glue code for the Rust extension. |
| `src/tree-sitter/` | **Versioned tree-sitter files** — generated C sources, queries, and Rust bindings. These are the canonical build-time copies; see Parser layer below. |

---

## Architecture (5 Layers)

```
1. Parser        tree-sitter-lilypond grammar → parse tree
2. IR            Score → Part(Group) → Voice → Measure → Note/Rest/…
3. Transforms    Idempotent, composable passes on the IR
4. Adapters      from_ir / to_ir per format (ly, mxl, midi, abc, …)
5. CLI + lib     rayon-parallel batch CLI; clean public Rust crate + PyO3 bindings
```

### 1 — Parser layer (`src/tree-sitter/`)
- **Core tree-sitter files are versioned inside the project** under `src/tree-sitter/` — do **not** depend on `tree-sitter-lilypond/` at build or test time.
- The grammar source (`grammar.js`) lives only in `tree-sitter-lilypond/lilypond/` and is **not** copied into `src/tree-sitter/`. It is needed only to regenerate the C parser; since the grammar is not modified, editing it is out of scope.
- Layout:
  - `src/tree-sitter/src/` — generated C sources (`parser.c`, `scanner.c`, …) — **committed to the repo**
  - `src/tree-sitter/queries/` — `.scm` highlight / injection queries — **committed to the repo**
  - `src/tree-sitter/bindings/rust/` — Rust bindings used by `Cargo.toml`
- To seed these files, generate C from `tree-sitter-lilypond/lilypond/grammar.js` then copy the output into `src/tree-sitter/` and commit.
- Precompile tree-sitter queries (`.scm` files) — cache compiled `Query` objects, never recompile per-file.
- `tree-sitter-lilypond/` remains in the workspace as a read-only reference only; it is never imported or linked at build time.

### 2 — IR (`src/ir/` + `src/lytk/ir/`)
- Rust IR in `src/ir/`; Python glue in `src/lytk/ir/`.
- `IRNode` is the base tree node with parent↔child links.
- Structural nodes (`Score`, `PartGroup`, `Part`, `Voice`, `Measure`) subclass `IRNode`.
- Leaf value objects (`Pitch`, `Duration`, `Articulation`, …) are **frozen dataclasses** — not nodes.
- In Rust: use `Arc<Node>` (or arena-backed `&'arena Node`) for cheap clone; arena/bump allocation reduces per-node allocation cost.
- IR must be **JSON-serializable** to support caching and round-trip test fixtures.
- Visitor pattern (`visitors.py`) for read-only tree walks; explicit mutating passes return new trees for immutability.

### 3 — Transform passes (`src/transforms/` + `src/lytk/transforms.py`)
- Each transform is **idempotent** and **composable**.
- API dual style (follow `torchaudio` conventions):
  - OOP: `Transpose(semitones=2).apply(score) -> Score`
  - Functional: `transpose(score, semitones=2) -> Score`
- Planned transforms: pitch/key transposition, inversion, retrograde, language change, rel↔abs pitch mode, add/remove barlines, indent, reformat, inline variables, combine `\include` files.
- Transforms must not mutate in-place; return new or copy-on-write IR.

### 4 — Adapters (`src/adapters/` + `src/lytk/converters/`)
- Rust adapters in `src/adapters/`; Python-facing ABCs in `src/lytk/converters/`.
- Each adapter implements `ToIRConverter` (`convert(path) -> Score`, `convert_string(text) -> Score`) and `FromIRConverter` (`convert(score) -> str`, `write(score, path)`).
- Adapter modules are **optional Cargo features** to keep the binary lean.
- Round-trip tests are required for every adapter (MusicXML test suite in `musicxmlTestSuite/`).

### 5 — CLI + library (`src/lib.rs`, `src/main.rs`)
- Rust CLI via `clap` in `src/main.rs` (stub — not yet implemented).
- CLI will support streaming and multi-threaded batch processing via `rayon`.
- Public Rust crate exposes a clean API that PyO3 binds; use `abi3-py39` stable ABI.
- Python bindings are not yet implemented beyond a stub `hello_from_bin()` function.

---

## Build & Dev Commands

```bash
# Python dev install (rebuilds Rust on change)
uv sync
maturin develop                     # or: uv run maturin develop

# Run Python tests
pytest tests/ -v
pytest tests/ --cov=lytk

# Run Rust tests
cargo test

# Lint / format
cargo fmt && cargo clippy -- -D warnings
ruff check . && ruff format .       # Python side

# Seed / refresh tree-sitter generated files from the reference repo
# (run once, or when pulling upstream grammar changes)
cd tree-sitter-lilypond/lilypond
tree-sitter generate grammar.js --abi 14     # regenerate C in-place
cd -
cp -r tree-sitter-lilypond/lilypond/src/       src/tree-sitter/src/
cp -r tree-sitter-lilypond/lilypond/queries/   src/tree-sitter/queries/
cp -r tree-sitter-lilypond/bindings/rust/      src/tree-sitter/bindings/rust/
# commit src/tree-sitter/ — these are the only files the Rust build needs
```

---

## Key Design Decisions & Conventions

### General
- **TDD** — every feature must have tests before or alongside implementation.
- **DRY / modularity / composability** — no ad-hoc one-offs; prefer extending the transform/adapter framework.
- **Performance baseline first** — profile `python-ly/` and record the baseline (time, memory). The Rust target must beat it.

### Rust specifics
- `feature` flags in `Cargo.toml` for heavy adapters: `midi`, `mxl`, `abc`, etc.
- Prefer arena/bump allocation (`bumpalo`) for AST nodes to reduce allocator pressure.
- Use `Arc<Node>` for cheap shared ownership; avoid unnecessary `clone()` on large trees.
- `rayon` for data-parallel batch CLI operations.
- Cross-language ABI: expose C-compatible types where needed; use `abi3` for Python.
- Define `benches/` with `criterion` benchmarks for all hot paths.

### Python specifics
- `pyproject.toml` uses `maturin` build backend; `uv` for env management.
- `lytk._core` is the compiled Rust extension; the installable Python package lives in **`src/lytk/`** (`python-source = "src"` in `pyproject.toml`).
- `.pyi` stub files alongside every `_core` sub-module.
- `__slots__` on all `IRNode` subclasses.
- `from __future__ import annotations` in every module.

### IR conventions
- Immutable leaf value objects (`Pitch`, `Duration`) → frozen dataclasses.
- `IRNode.find(cls)` for typed tree queries.
- JSON fixture files in `tests/fixtures/` for regression and round-trip testing.

---

## Reference Projects (read-only, for inspiration)

| Folder | What to take from it |
|---|---|
| `python-ly/` | Pitch language translation, indent, reformat, bar lines, rel↔abs, inversion, retrograde — **primary Python baseline** for benchmarks |
| `quickly/` | Successor to python-ly; improved tokenizer API |
| `src/tree-sitter/` | **Versioned-in-project** grammar JS, generated C, queries, Rust bindings — the build-time source of truth |
| `tree-sitter-lilypond/` | Reference copy of the upstream grammar — read-only; copy from here into `src/tree-sitter/` when updating |
| `lilypond-midi-input/` | Real-time MIDI→LilyPond in Rust; `clap` CLI patterns |
| `lilypond-rs/` | Rust types for LilyPond objects (inspired by abjad) |
| `abjad/` | Python score-building API; visitor and IR patterns |
| `lilybert/` | Tokenizer, data augmentation, Hydra config patterns |
| `MEILER/` | MEI→LilyPond XSLT; useful edge-case scores |
| `PDMX/` | Dataset IR for MusicXML + MSCZ; internal representation reference |
| `MuseScore/src/importexport/` | C++ MusicXML, MEI, mscz, midi parsing and emitting reference (complex real-world codebase) |
| `symusic/` | fast MIDI parsing in C++ with IR; useful for MIDI adapter reference and benchmarks |
| `music21/` | Python musicology toolkit; useful for various format parsing, IR design patterns and advanced MIR features |

---

## Common Pitfalls

- **Do not** modify files inside any reference subdirectory (`python-ly/`, `abjad/`, `lilypond/`, `tree-sitter-lilypond/`, etc.) — treat them all as read-only references. The canonical tree-sitter files that are actually built live in `src/tree-sitter/`.
- **tree-sitter source of truth**: the Rust build reads only `src/tree-sitter/` (committed generated C + queries + bindings). `grammar.js` is not copied there — it stays in `tree-sitter-lilypond/` (read-only reference). To refresh after an upstream grammar change: regenerate C inside `tree-sitter-lilypond/`, copy to `src/tree-sitter/`, commit.
- **ZipFile/MXL**: compressed `.mxl` files are ZIP archives; unzip before XML parsing.
- **Relative vs absolute pitch mode**: LilyPond `\relative` changes note-name semantics; track mode explicitly through the IR.
- **Pitch language**: LilyPond supports `nederlands`, `english`, `italiano`, `deutsch`, etc. — store the active language in `Score.metadata` and translate at emit time, not during parsing.
- **Accidental vs key-signature pitch**: distinguish written pitch (as in score) from sounding pitch (concert pitch) in the IR.
- **tree-sitter ABI**: regenerate parser C sources whenever `grammar.js` changes; mismatched ABI versions cause silent misparsing.
- **PyO3 / maturin**: always run `maturin develop` after editing Rust code; stale `.so` will silently use the old binary.
- **Round-trip fidelity**: MusicXML → IR → LilyPond → IR → MusicXML will not be byte-identical; test semantic equivalence (pitch, duration, structure), not string equality.

---

## Testing Strategy

```
tests/
  fixtures/           # JSON IR snapshots + .ly/.xml pairs
  adapters/           # round-trip tests per format
  transforms/         # property-based tests (idempotency, inverse pairs)
  benchmarks/         # criterion (Rust) / pytest-benchmark (Python)
```

- **Adapters**: use `musicxmlTestSuite/xmlFiles/` as the corpus.
- **Transforms**: test idempotency (`T(T(x)) == T(x)`) and inverses (`inverse_T(T(x)) == x`).
- **Property tests**: use `hypothesis` (Python) / `proptest` (Rust).
- **Benchmarks**: profile `python-ly/` as the Python baseline; gate Rust optimisation PRs on measured speedup.

---

## File Map (notable files)

### Rust project (build targets — edit these)

| Path | Purpose |
|---|---|
| `Cargo.toml` | Workspace root; add adapter feature flags here |
| `pyproject.toml` | maturin build config; Python metadata |
| `src/lib.rs` | PyO3 module root — exposes Rust API to Python |
| `src/lytk/__init__.py` | Python public API (`mxml2ly`, `ly2mxml`, `convert`) |
| `src/lytk/_core.pyi` | PyO3 stub — keep in sync with `lib.rs` |
| `src/tree-sitter/` | Versioned grammar JS, generated C, `.scm` queries, and Rust bindings — edit and commit here |
| `src/tree-sitter/queries/` | Precompile-able `.scm` highlight/injection queries |
