---
name: profile-python-baseline
description: "Profile the Python lytk converters to establish a time/memory baseline before Rust rewrite."
argument-hint: "Which converter to profile? (mxml2ly, ly2mxml, or both)"
agent: agent
tools: [execute, read/terminalSelection, read/terminalLastCommand, edit, the0807.uv-toolkit/uv-init, the0807.uv-toolkit/uv-sync, the0807.uv-toolkit/uv-add, the0807.uv-toolkit/uv-add-dev, the0807.uv-toolkit/uv-upgrade, the0807.uv-toolkit/uv-clean, the0807.uv-toolkit/uv-lock, the0807.uv-toolkit/uv-venv, the0807.uv-toolkit/uv-run, the0807.uv-toolkit/uv-script-dep, the0807.uv-toolkit/uv-python-install, the0807.uv-toolkit/uv-python-pin, the0807.uv-toolkit/uv-tool-install, the0807.uv-toolkit/uvx-run, the0807.uv-toolkit/uv-activate-venv, the0807.uv-toolkit/uv-pep723, the0807.uv-toolkit/uv-install]
---

Profile the **Python prototype** (`lytk-py/` at repo root) to establish a reproducible performance baseline.
This baseline sets the speed and memory targets for the Rust port.

> **Important**: `lytk-py/` is the read-only Python prototype. `src/lytk/` is the Rust project's Python package. This prompt profiles the **prototype** (`lytk-py/`), not the Rust project.

## Inputs

- Argument `$args` selects which pipeline(s) to profile: `mxml2ly`, `ly2mxml`, or `both` (default: `both`).
- Corpus: the MusicXML files in `musicxmlTestSuite/xmlFiles/` — use all `.xml` files found there.
- Prototype entry points: `lytk_py.converters.mxml_to_ir`, `lytk_py.converters.ir_to_ly` (from `lytk-py/` package).

## Steps

### 1 — Environment check
Verify that `cProfile`, `pstats`, and `memray` (or fall back to `memory_profiler`) are importable.
Install missing packages with `uv pip install memray` or `uv pip install memory-profiler` if needed.

### 2 — Time profiling (cProfile)
Write and run a script `benchmarks/run_profile.py` that:
1. Collects all `.xml` files from `musicxmlTestSuite/xmlFiles/` (skip `.mxl` zips for now).
2. For each selected pipeline (`mxml2ly` and/or `ly2mxml`):
   - Wraps the call in `cProfile.Profile()`.
   - Runs the full corpus through the converter.
   - Dumps `pstats` sorted by `cumtime`; captures the **top 30 functions**.
3. Records **total wall time** and **per-file median/p95 time** using `time.perf_counter`.

### 3 — Memory profiling
Run the same script under `memray run --output benchmarks/memray_out.bin benchmarks/run_profile.py`
(or `@profile` decorator with `memory_profiler` as fallback).
Extract **peak RSS** and **peak heap allocation**.

### 4 — Generate the Markdown report
Save results to `benchmarks/python_baseline.md` with this structure:

```markdown
# Python Baseline — lytk-py (Python prototype)

_Generated: <date>_
_Corpus: <N> files from musicxmlTestSuite/xmlFiles/_
_Python: <version>_

## mxml2ly

| Metric | Value |
|--------|-------|
| Total wall time | Xs |
| Median per-file | Xms |
| p95 per-file | Xms |
| Peak RSS | XMB |

### Top hotspots (cumtime)

| Rank | Function | File | Calls | cumtime |
|------|----------|------|-------|---------|
| 1 | … | … | … | … |

## ly2mxml

… (same table structure)

## Notes

- Slowest individual file: <filename> (<time>ms)
- Any parsing errors or skipped files
```

### 5 — Commit the artefacts
Stage `benchmarks/run_profile.py` and `benchmarks/python_baseline.md` and print a reminder:

```
Baseline recorded. Gate future Rust PRs on measurable improvement over these numbers.
```

## Output contract
- `benchmarks/run_profile.py` — reproducible profiling script (idempotent, re-runnable).
- `benchmarks/python_baseline.md` — filled-in Markdown report committed to the repo.
- Console output: a short summary table (total time + peak memory for each pipeline).
