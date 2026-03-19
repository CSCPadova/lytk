# Development Guide

## Prerequisites

| Tool | Version | Purpose |
|---|---|---|
| Rust + Cargo | ≥ 1.78 | Compile the Rust library and CLI |
| Python | ≥ 3.10 | Python bindings and tooling |
| uv | latest | Python dependency management |
| maturin | ≥ 1.0, < 2.0 | Build the PyO3 extension |

Install the Rust toolchain:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Install `uv` and `maturin`:

```bash
pip install uv
uv tool install maturin
```

---

## Compiling

### Rust — library and CLI

Debug build (fast compile, no optimisations):

```bash
cargo build
```

Release build (optimised, slower compile):

```bash
cargo build --release
```

The compiled CLI binary is at:

```
target/debug/lytk          # debug
target/release/lytk        # release
```

Check for errors without producing a binary (fastest feedback loop):

```bash
cargo check
```

### Python extension

The Python package (`import lytk`) requires building the Rust `_core` extension with maturin.

Development install (rebuilds the `.so` on every `cargo` change):

```bash
uv sync              # install Python deps into the venv
maturin develop      # compile Rust and install into the active venv
```

Or in one step via uv:

```bash
uv run maturin develop
```

Build a wheel (for distribution):

```bash
maturin build --release
```

---

## Running Tests

### Rust unit tests

Run all tests across all modules:

```bash
cargo test
```

Run a specific test by name (substring match):

```bash
cargo test test_parse_minimal_score
cargo test test_parse_all_fixtures
cargo test test_sextuplet
```

Run tests for a specific module:

```bash
cargo test --lib ir::
cargo test --lib adapters::
cargo test --lib transforms::
```

Run flatten-specific tests:

```bash
cargo test --lib adapters::ly_flatten
```

Show output from passing tests (useful for debugging):

```bash
cargo test -- --nocapture
```

Run tests in a single thread (useful when tests write to the filesystem):

```bash
cargo test -- --test-threads=1
```

### Rust doc tests

Doc tests are run as part of `cargo test`. To run only doc tests:

```bash
cargo test --doc
```

### Python tests

```bash
pytest tests/ -v
```

Run with coverage:

```bash
pytest tests/ --cov=lytk --cov-report=term-missing
```

### Lint and format

```bash
# Rust
cargo fmt                           # format
cargo clippy -- -D warnings         # lint (treat warnings as errors)

# Python
ruff check .                        # lint
ruff format .                       # format
```

---

## Test Fixtures

- **MusicXML fixtures** — `tests/fixtures/xml/` (143 files from the MusicXML Test Suite)
- **LilyPond fixtures** — `tests/fixtures/ly/` (test scores including multi-movement, multi-staff, lyrics, figured bass)

The test `test_parse_all_fixtures` in `src/adapters/mxml_to_ir.rs` runs the MusicXML adapter against every file in `tests/fixtures/xml/` and asserts that each parses without error and produces at least one part.

---

## CLI

The CLI is fully implemented. See [docs/cli.md](cli.md) for the complete reference.

### Quick reference

```bash
# Convert
lytk convert input.xml -o output.ly
lytk convert input.ly  -o output.xml
lytk convert input_dir/ -o output_dir/ -j 8

# Flatten (expand \include directives)
lytk flatten score.ly -o flat.ly
lytk flatten score.ly -I ./lib -o flat.ly

# Transpose
lytk transpose input.ly --semitones 3 -o out.ly

# Metadata
lytk info input.xml
```

---

## Cargo Features

MIDI support is included by default (the `midly` crate is a standard dependency).

---

## Benchmarks

Benchmarks are in `benches/benchmarks.rs` (Criterion) and `benches/bench_python.py`
(pytest-benchmark, comparing lytk vs python-ly).

```bash
cargo bench
pytest benches/bench_python.py --benchmark-compare
```

Key measured results (release build, ~5k-note score):

| Operation | lytk (Rust) | python-ly | Speedup |
|---|---|---|---|
| Transpose | ~0.8 ms | ~42 ms | ~52× |
| Language change | ~0.5 ms | ~20 ms | ~40× |
