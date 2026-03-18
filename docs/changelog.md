# Changelog

## 2026-03-18 — Epic 0: Housekeeping & Stabilization ✅

### E0T1: Fix compiler warnings
- Fixed all 76 clippy warnings: assign_op_pattern, unused imports/variables, clone on Copy types, boxed large enum variant (Direction), dead code, ptr_arg, redundant closures, needless lifetimes
- Files: `src/ir/lift.rs`, `src/ir/lower.rs`, `src/ir/music.rs`, `src/adapters/ly_to_ir.rs`, `src/adapters/ir_to_ly.rs`, `src/adapters/ir_to_mxml.rs`

### E0T2: Remove debug `eprintln!` statements
- Removed 12 debug `eprintln!` calls from `ly_to_ir.rs` (RESOLVE, PRE-MERGE, POST-MERGE, SCALE, TIME, RESPLIT) and `lower.rs` (boundaries)
- Kept legitimate `eprintln!` in `main.rs` (CLI error output) and `ly_flatten.rs` (deduplication warnings)

### E0T3: Fix `test_figuremode_distribution_across_measures`
- Root cause: `distribute_figured_bass` placed figures into a leading attribute-only measure (created by `\clef bass` flush before variable resolution). This empty measure was later drained by `merge_leading_attribute_measures`, but figured bass entries were not carried over.
- Fix: `distribute_figured_bass` now skips leading measures with no voice content; `merge_leading_attribute_measures` now merges figured bass from drained measures into the target measure
- All 324 tests pass, 0 failures

### E0T4: Clean up untracked root files
- Added `.gitignore` patterns for root-level scratch/test output files: `/out.xml`, `/example.xml`, `/pedal.xml`, `/rep.xml`, `/test*.ly`, `/test*.midi`, `/test*.pdf`

### E0T5: Add GitHub Actions CI workflow
- Created `.github/workflows/ci.yml` with 4 jobs: fmt (rustfmt check), clippy (with midi feature), test (Linux/macOS/Windows matrix), Python tests (maturin develop + pytest)

### Also in this commit
- Two-layer IR: Music tree types (`music.rs`, `annotation.rs`, `moment.rs`), lift/lower passes (`lift.rs`, `lower.rs`), `MusicTransform` trait with implementations for Transpose, Invert, Retrograde, ChangeLanguage
- Adapter bridging via `ToMusicAdapter`/`FromMusicAdapter` traits
- Updated `docs/roadmap.md` with full epic/task development plan
- Updated `CLAUDE.md` and `.github/copilot-instructions.md` to reference roadmap and changelog
- Added test fixtures: `tests/fixtures/ly/` (chopin_n, pedal, repeats), `tests/fixtures/mxl/` (10 MXL files)

### Next up
- **Epic 2: Break Up Monolithic Files** — starting with E2T1 (split `ly_to_ir.rs` ~8000 lines into module directory). Execution order: E0 → E2 → E1.
