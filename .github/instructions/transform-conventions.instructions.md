---
description: "Use when writing, editing, or reviewing transform passes in lytk. Covers the dual OOP/functional API, idempotency, copy-on-write IR, and required test patterns for every transform."
applyTo: ["src/transforms/**", "src/lytk/transforms.py", "tests/transforms/**"]
---

# Transform Conventions

> **Scope**: these conventions apply to the **Rust project** under `src/`.

## Dual API (OOP + functional)

Every transform must be available in both styles — follow `torchaudio` conventions:

```python
# OOP
Transpose(semitones=2).apply(score) -> Score

# Functional
transpose(score, semitones=2) -> Score
```

The functional form is a thin wrapper that instantiates and calls the OOP form:

```python
def transpose(score: Score, semitones: int) -> Score:
    return Transpose(semitones=semitones).apply(score)
```

In Rust, expose both as free functions and `impl Transform for Foo`.

## No mutation — return new IR

Transforms **must not** mutate the input `Score` in place. Always return a new or copy-on-write tree. Callers depend on the original remaining unchanged.

```python
# BAD
def apply(self, score: Score) -> Score:
    score.parts[0].name = "..."   # mutates input
    return score

# GOOD
def apply(self, score: Score) -> Score:
    new_score = copy.deepcopy(score)
    ...
    return new_score
```

## Idempotency

Every transform must be idempotent: `T(T(x)) == T(x)`. Add a test for this.

```python
def test_transpose_idempotent():
    result = transpose(score, semitones=2)
    assert transpose(result, semitones=2) == result  # second application is a no-op
```

> Exception: transforms that are their own inverse (retrograde, inversion) satisfy `T(T(x)) == x` instead — document this explicitly and test it.

## Invertibility

When a logical inverse exists, implement it and test the round-trip:

```python
def test_transpose_roundtrip():
    shifted = transpose(score, semitones=5)
    restored = transpose(shifted, semitones=-5)
    assert restored == score
```

## Composability

Transforms are composable via sequential application. Do not add side effects or hidden state that would break ordering.

## Required tests for every new transform

| Test | What it checks |
|------|----------------|
| `test_<name>_basic` | Correct output for a known input |
| `test_<name>_idempotent` | `T(T(x)) == T(x)` |
| `test_<name>_roundtrip` | `inverse(T(x)) == x` (if invertible) |
| `test_<name>_no_mutation` | Input `Score` unchanged after `apply()` |
| `test_<name>_hypothesis` | Property-based test with `hypothesis` / `proptest` |

Use `tests/fixtures/` JSON snapshots for regression. Load them with `Score.from_json(...)`.

## Python class template

```python
from __future__ import annotations
from dataclasses import dataclass
from lytk.ir.score import Score


@dataclass(frozen=True)
class MyTransform:
    """One-line description.

    Args:
        param: What it controls.
    """
    param: int

    def apply(self, score: Score) -> Score:
        # Return a transformed copy — never mutate.
        ...


def my_transform(score: Score, param: int) -> Score:
    return MyTransform(param=param).apply(score)
```

Use `@dataclass(frozen=True)` so transform instances are hashable and safe to cache.

## Rust equivalent

```rust
pub struct MyTransform { pub param: i32 }

impl MyTransform {
    pub fn apply(&self, score: &Score) -> Score { ... }
}

pub fn my_transform(score: &Score, param: i32) -> Score {
    MyTransform { param }.apply(score)
}
```

Return owned `Score`; use `Arc::clone` for subtrees that are unchanged to keep allocations cheap.
