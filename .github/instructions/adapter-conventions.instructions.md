---
description: "Use when writing, editing, or reviewing adapters (format converters) in lytk. Covers the ToIRConverter/FromIRConverter ABCs, Cargo feature flags, MXL zip handling, round-trip test requirements, and PyO3 binding patterns."
applyTo: ["src/adapters/**", "src/lytk/converters/**", "tests/adapters/**"]
---

# Adapter Conventions

> **Scope**: these conventions apply to the **Rust project** under `src/`. The `lytk-py/converters/` directory is the read-only Python prototype — use it as a reference spec, never as a build target.

---

## Two required ABCs

Every adapter must implement one or both of these interfaces (mirror the Python reference in `lytk-py/converters/base.py`):

```python
# src/lytk/converters/base.py  (Python-facing ABC)
class ToIRConverter(ABC):
    def convert(self, source: str | Path) -> Score: ...       # file path or string input
    def convert_string(self, text: str) -> Score: ...         # in-memory string input

class FromIRConverter(ABC):
    def convert(self, score: Score) -> str: ...               # returns format as string
    def write(self, score: Score, path: str | Path) -> None: ...  # writes to file
```

In Rust, define equivalent traits:

```rust
pub trait ToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score>;
    fn convert_str(&self, text: &str) -> Result<Score>;
}

pub trait FromIrAdapter {
    fn convert(&self, score: &Score) -> Result<String>;
    fn write(&self, score: &Score, path: &Path) -> Result<()>;
}
```

---

## Cargo feature flags

Heavy adapters must be gated behind optional features so they are not compiled unless needed:

```toml
# Cargo.toml
[features]
default = []
mxl  = ["dep:quick-xml", "dep:zip"]
midi = ["dep:midly"]
abc  = ["dep:abc-parser"]

[dependencies]
quick-xml = { version = "0.31", optional = true }
zip       = { version = "2",    optional = true }
midly     = { version = "0.5",  optional = true }
```

Gate the adapter struct behind the feature:

```rust
#[cfg(feature = "mxl")]
pub mod mxl;
```

---

## MXL / compressed MusicXML

`.mxl` files are ZIP archives. Always unzip before XML parsing — never pass the raw bytes to the XML parser.

```rust
#[cfg(feature = "mxl")]
fn open_musicxml(path: &Path) -> Result<String> {
    if path.extension().and_then(|e| e.to_str()) == Some("mxl") {
        let file = std::fs::File::open(path)?;
        let mut zip = zip::ZipArchive::new(file)?;
        // rootfile is declared in META-INF/container.xml
        let rootfile = find_rootfile(&mut zip)?;
        let mut entry = zip.by_name(&rootfile)?;
        let mut buf = String::new();
        entry.read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Ok(std::fs::read_to_string(path)?)
    }
}
```

---

## Round-trip test requirement

Every adapter must have a round-trip test. Use `musicxmlTestSuite/xmlFiles/` as the corpus for MusicXML adapters.

**Do not test byte equality** — test *semantic equivalence* (pitches, durations, structure):

```python
# tests/adapters/test_mxl_roundtrip.py
import pytest
from pathlib import Path

CORPUS = list(Path("musicxmlTestSuite/xmlFiles").glob("*.xml"))

@pytest.mark.parametrize("xml_file", CORPUS, ids=lambda p: p.name)
def test_mxl_roundtrip(xml_file):
    from lytk import mxml2ly, ly2mxml
    ly_text  = mxml2ly(xml_file)
    mxml_out = ly2mxml(ly_text)
    # compare pitch/duration/structure, not raw XML bytes
    assert_scores_equivalent(xml_file, mxml_out)
```

Equivalent Rust test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn roundtrip_mxl_to_ir_to_mxl() {
        let path = Path::new("musicxmlTestSuite/xmlFiles/01a-Pitches-Pitches.xml");
        let adapter = MxlToIrAdapter::new();
        let score   = adapter.convert_file(path).unwrap();
        let out     = IrToMxlAdapter::new().convert(&score).unwrap();
        // parse out back and compare IR fields, not strings
        let score2  = adapter.convert_str(&out).unwrap();
        assert_eq!(score.parts().len(), score2.parts().len());
    }
}
```

---

## Required tests for every new adapter

| Test | What it checks |
|------|----------------|
| `test_<fmt>_basic_parse` | A known-good file produces a non-empty `Score` |
| `test_<fmt>_roundtrip` | `from_ir(to_ir(x))` is semantically equivalent to `x` |
| `test_<fmt>_mxl_zip` | Compressed `.mxl` input is handled (MusicXML only) |
| `test_<fmt>_empty_score` | Graceful handling of an empty or minimal score |
| `test_<fmt>_parse_error` | Invalid input raises a typed `ParseError`, not a panic |

---

## PyO3 binding pattern

Expose each adapter to Python through `src/lib.rs`:

```rust
#[cfg(feature = "mxl")]
#[pyfunction]
fn mxml_to_ir(path: &str) -> PyResult<PyScore> {
    use crate::adapters::mxl::MxlToIrAdapter;
    let score = MxlToIrAdapter::new()
        .convert_file(Path::new(path))
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
    Ok(PyScore::from(score))
}
```

Add a corresponding stub in `src/lytk/_core.pyi`:

```python
def mxml_to_ir(path: str) -> Score: ...
```

---

## Pitch language and relative mode

- Store the active pitch language (`nederlands`, `english`, `italiano`, `deutsch`, …) in `Score.metadata`, not in the parser state.
- Translate language at **emit time** (in `FromIrAdapter`), not during parsing.
- Track `\relative` vs `\absolute` mode explicitly in the IR (`Score.metadata.pitch_mode`); do not infer it during round-trip.
