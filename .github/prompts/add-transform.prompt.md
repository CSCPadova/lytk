---
name: add-transform
description: "Scaffold a complete new transform pass in lytk end-to-end: Rust struct, dual OOP/functional API, PyO3 binding, .pyi stub, and all required tests."
argument-hint: "Transform name in snake_case, e.g. 'transpose', 'invert', 'retrograde', 'change_language'"
agent: agent
tools: [run_in_terminal, read_file, create_file, replace_string_in_file]
---

Scaffold a complete new **transform pass** for lytk following all conventions in
[transform-conventions.instructions.md](../instructions/transform-conventions.instructions.md).

The transform to add is: **`$args`**  
If `$args` is empty, ask the user for the transform name (snake_case) and its parameters before proceeding.

---

## Step 1 — Read before writing

Read these files first to understand what already exists and avoid duplication:

- [src/lib.rs](../../src/lib.rs) — check existing PyO3 registrations
- [src/lytk/_core.pyi](../../src/lytk/_core.pyi) — check existing stubs
- `src/transforms/` — check if a module file already exists for this transform
- `lytk-py/transforms.py` — read-only Python prototype; use as the spec for behaviour

---

## Step 2 — Rust transform struct

Create `src/transforms/$args.rs` (add `pub mod $args;` to `src/transforms/mod.rs`, creating that file if absent).

```rust
use crate::ir::Score;

pub struct ${Name} {
    pub param: <type>,   // replace with real parameters
}

impl ${Name} {
    pub fn new(param: <type>) -> Self {
        Self { param }
    }

    pub fn apply(&self, score: &Score) -> Score {
        // Build and return a new Score — never mutate the input.
        todo!()
    }
}

/// Functional shorthand.
pub fn $args(score: &Score, param: <type>) -> Score {
    ${Name}::new(param).apply(score)
}
```

Rules:
- **Never mutate** the input `Score`. Return a new owned value.
- Use `Arc::clone` for subtrees that are unchanged to keep allocations cheap.
- If this transform is **self-inverse** (e.g. retrograde, inversion), document it with a `// Self-inverse: apply twice returns original` comment and test `T(T(x)) == x` instead of idempotency.

---

## Step 3 — PyO3 bindings

Add Python-facing functions in `src/lib.rs`:

```rust
#[pyfunction]
fn $args(score: &PyScore, param: <type>) -> PyResult<PyScore> {
    Ok(PyScore::from(crate::transforms::$args::$args(&score.inner, param)))
}
```

Register in the `#[pymodule]` initializer:

```rust
m.add_function(wrap_pyfunction!($args, m)?)?;
```

---

## Step 4 — Python stub

Append to `src/lytk/_core.pyi`:

```python
# $args transform
def $args(score: Score, param: <type>) -> Score: ...
```

---

## Step 5 — Python glue (OOP wrapper)

Add to `src/lytk/transforms.py` (create the file if it does not exist):

```python
from __future__ import annotations
from dataclasses import dataclass
from lytk._core import $args as _$args
from lytk._core import Score


@dataclass(frozen=True)
class ${Name}:
    """<One-line description>.

    Args:
        param: <What it controls>.
    """
    param: <type>

    def apply(self, score: Score) -> Score:
        return _$args(score, self.param)


def $args(score: Score, param: <type>) -> Score:
    """Functional shorthand for ${Name}."""
    return ${Name}(param=param).apply(score)
```

---

## Step 6 — Tests

Create `tests/transforms/test_$args.py` with the full required test set:

```python
from __future__ import annotations
import copy
import pytest
from lytk.transforms import ${Name}, $args
from tests.helpers import load_fixture   # JSON fixture loader


@pytest.fixture
def score():
    return load_fixture("simple_score.json")


def test_${args}_basic(score):
    """Transform produces expected output for a known input."""
    result = $args(score, param=<value>)
    assert <expected condition>


def test_${args}_no_mutation(score):
    """Input Score is unchanged after apply()."""
    original = copy.deepcopy(score)
    $args(score, param=<value>)
    assert score == original


def test_${args}_idempotent(score):
    """T(T(x)) == T(x)."""
    # Skip this test and use test_${args}_self_inverse instead
    # if this transform is self-inverse (e.g. retrograde, inversion).
    once  = $args(score, param=<value>)
    twice = $args(once,  param=<value>)
    assert twice == once


def test_${args}_roundtrip(score):
    """inverse(T(x)) == x — skip if no logical inverse exists."""
    shifted   = $args(score, param=<forward_value>)
    restored  = $args(shifted, param=<inverse_value>)
    assert restored == score


def test_${args}_hypothesis(score):
    """Property-based test with hypothesis."""
    from hypothesis import given, strategies as st

    @given(param=st.<strategy>())
    def inner(param):
        result = $args(score, param=param)
        assert result is not score           # always returns new object
        assert isinstance(result, type(score))

    inner()
```

Also add a Rust unit test inside `src/transforms/$args.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ${args}_basic() {
        let score = Score::fixture_simple();
        let result = $args(&score, <value>);
        assert!(!result.parts().is_empty());
    }

    #[test]
    fn ${args}_no_mutation() {
        let score = Score::fixture_simple();
        let before = score.clone();
        let _ = $args(&score, <value>);
        assert_eq!(score, before);
    }

    #[test]
    fn ${args}_idempotent() {
        let score  = Score::fixture_simple();
        let once   = $args(&score, <value>);
        let twice  = $args(&once,  <value>);
        assert_eq!(once, twice);
    }
}
```

---

## Step 7 — Verify

Run:

```bash
cargo test transforms::$args
pytest tests/transforms/test_$args.py -v
```

Fix all failures before declaring done.

---

## Output contract

By the end of this prompt the following files must exist or be updated:

- `src/transforms/$args.rs` — Rust implementation
- `src/transforms/mod.rs` — `pub mod $args;` added
- `src/lib.rs` — PyO3 function registered
- `src/lytk/_core.pyi` — stub updated
- `src/lytk/transforms.py` — OOP class + functional wrapper added
- `tests/transforms/test_$args.py` — full test file with all 5 tests
