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

Build with the optional MIDI feature:

```bash
cargo build --features midi
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

The installed package lives in `src/lytk/`. After running `maturin develop` you can import it:

```python
import lytk
print(lytk.hello_from_bin())   # stub, prints the Rust greeting
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
```

Run tests for a specific module:

```bash
cargo test --lib ir::
cargo test --lib adapters::
cargo test --lib transforms::
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

Run a specific test file:

```bash
pytest tests/adapters/test_mxml.py -v
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

MusicXML fixture files are in `tests/fixtures/xml/` (143 files from the MusicXML Test Suite). LilyPond fixtures are in `tests/fixtures/ly/` (30 files).

The test `test_parse_all_fixtures` in `src/adapters/mxml_to_ir.rs` runs the MusicXML adapter against every file in `tests/fixtures/xml/` and asserts that each parses without error and produces at least one part.

---

## CLI

> **Note:** The CLI is currently a stub. Running `lytk` prints "lytk CLI not yet implemented" and exits with code 1. The interface below describes the planned commands — see [roadmap.md](roadmap.md) for implementation status.

### Planned interface

Convert a single file:

```bash
lytk convert input.xml -o output.ly          # MusicXML → LilyPond
lytk convert input.ly  -o output.xml         # LilyPond → MusicXML
lytk convert input.mxl -o output.ly          # compressed MXL → LilyPond
```

Batch convert a directory (parallel, 8 threads):

```bash
lytk convert input_dir/ -o output_dir/ -j 8
```

Force output format (overrides extension detection):

```bash
lytk convert input.xml -o output.txt --format ly
```

Apply a transform and convert:

```bash
lytk transpose input.xml --semitones 3 -o transposed.xml
lytk transpose input_dir/ --semitones 3 -o output_dir/ -j 8
```

Print score metadata:

```bash
lytk info input.xml
```

### Building the CLI

```bash
cargo build --release
```

Add the binary to your PATH:

```bash
export PATH="$PATH:$(pwd)/target/release"
lytk --help
```

---

## Cargo Features

| Feature | Default | Description |
|---|---|---|
| `midi` | off | Enable MIDI adapter (adds `midly` dependency) |

Enable a feature at build time:

```bash
cargo build --features midi
cargo test --features midi
```
