---
name: add-adapter
description: "Scaffold a complete new format adapter in lytk end-to-end: Cargo feature flag, Rust struct, PyO3 binding, .pyi stub, and round-trip tests."
argument-hint: "Format name, e.g. 'midi', 'abc', 'mxl', 'ly'"
agent: agent
tools: [run_in_terminal, read_file, create_file, replace_string_in_file]
---

Scaffold a complete new **format adapter** for lytk following all conventions in
[adapter-conventions.instructions.md](../instructions/adapter-conventions.instructions.md).

The format to add is: **`$args`**
If `$args` is empty, ask the user which format before proceeding.

---

## Step 1 — Read before writing

Read these files first to understand what already exists and avoid duplication:

- [Cargo.toml](../../Cargo.toml) — check existing `[features]` and `[dependencies]`
- [src/lib.rs](../../src/lib.rs) — check existing PyO3 module registrations
- [src/lytk/_core.pyi](../../src/lytk/_core.pyi) — check existing stubs
- `src/adapters/` — check existing adapter implementations for patterns

---

## Step 2 — Cargo feature flag

Add an optional feature to `Cargo.toml`:

```toml
[features]
$args = ["dep:<primary-crate>"]

[dependencies]
<primary-crate> = { version = "<latest>", optional = true }
```

Rules:
- One feature per adapter: `mxl`, `midi`, `abc`, `ly`, etc.
- Only list crates actually needed by this adapter.
- Do not add to `default = []` — adapters must be opt-in.

---

## Step 3 — Rust adapter struct

Create `src/adapters/$args.rs` (and add `pub mod $args;` to `src/adapters/mod.rs`, creating that file if absent).

The struct must implement both traits where applicable:

```rust
#[cfg(feature = "$args")]
pub struct ${Fmt}ToIrAdapter { /* options */ }

#[cfg(feature = "$args")]
impl ToIrAdapter for ${Fmt}ToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score> { ... }
    fn convert_str(&self, text: &str) -> Result<Score> { ... }
}

#[cfg(feature = "$args")]
pub struct IrTo${Fmt}Adapter { /* options */ }

#[cfg(feature = "$args")]
impl FromIrAdapter for IrTo${Fmt}Adapter {
    fn convert(&self, score: &Score) -> Result<String> { ... }
    fn write(&self, score: &Score, path: &Path) -> Result<()> { ... }
}
```

Special rules:
- **MXL/MusicXML only**: unzip `.mxl` files before parsing (see adapter-conventions for the zip pattern).
- **LilyPond**: track pitch language and `\relative`/`\absolute` mode in `Score.metadata` — do not infer at parse time.
- Return typed `AdapterError` (not `anyhow::Error` directly) so Python sees a clean `ValueError`.

---

## Step 4 — PyO3 bindings

Add public Python-facing functions in `src/lib.rs` inside `#[cfg(feature = "$args")]` guards:

```rust
#[cfg(feature = "$args")]
#[pyfunction]
fn ${args}_to_ir(path: &str) -> PyResult<PyScore> { ... }

#[cfg(feature = "$args")]
#[pyfunction]
fn ir_to_${args}(score: &PyScore) -> PyResult<String> { ... }
```

Register them in the `#[pymodule]` initializer:

```rust
#[cfg(feature = "$args")]
m.add_function(wrap_pyfunction!(${args}_to_ir, m)?)?;
#[cfg(feature = "$args")]
m.add_function(wrap_pyfunction!(ir_to_${args}, m)?)?;
```

---

## Step 5 — Python stub

Append to `src/lytk/_core.pyi`:

```python
# $args adapter
def ${args}_to_ir(path: str) -> Score: ...
def ir_to_${args}(score: Score) -> str: ...
```

---

## Step 6 — Tests

Create `tests/adapters/test_${args}_adapter.py` with the full required test set:

| Test | What it checks |
|------|----------------|
| `test_${args}_basic_parse` | A known-good file produces a non-empty `Score` |
| `test_${args}_roundtrip` | `from_ir(to_ir(x))` is semantically equivalent to `x` |
| `test_${args}_mxl_zip` | Compressed `.mxl` input handled *(MusicXML only)* |
| `test_${args}_empty_score` | Empty/minimal score is handled gracefully |
| `test_${args}_parse_error` | Invalid input raises `ValueError`, not a panic |

For MusicXML: parametrize `test_${args}_roundtrip` over `musicxmlTestSuite/xmlFiles/*.xml`.
For other formats: use fixtures from `tests/fixtures/`.

Also add a Rust unit test inside `src/adapters/$args.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_${args}() { ... }
}
```

---

## Step 7 — Verify

Run:

```bash
cargo test --features $args
pytest tests/adapters/test_${args}_adapter.py -v
```

Fix any errors before declaring done. Do **not** run `maturin develop` unless the user asks — building takes time.

---

## Output contract

By the end of this prompt the following files must exist or be updated:

- `Cargo.toml` — new feature + dependency
- `src/adapters/$args.rs` — adapter implementation
- `src/lib.rs` — PyO3 bindings registered
- `src/lytk/_core.pyi` — stub updated
- `tests/adapters/test_${args}_adapter.py` — full test file
