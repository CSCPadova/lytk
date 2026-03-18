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

---

## 2026-03-19 — Epic 2: Break Up Monolithic Files ✅

Pure refactor — no functional changes. All 324 tests pass, clippy clean.

### E2T1: Split `ly_to_ir.rs` (8418 lines) into module directory
- Converted `src/adapters/ly_to_ir.rs` → `src/adapters/ly_to_ir/` with 12 sub-modules:
  - `mod.rs` (334) — adapter struct, trait impls, top-level orchestration
  - `state.rs` (462) — WalkState struct + impl
  - `walk.rs` (1095) — tree-sitter walking functions
  - `modifiers.rs` (406) — handle_symbol, handle_escaped_word, handle_punctuation
  - `music.rs` (767) — music expression parsing (chords, grace, tuplets, repeats)
  - `consume.rs` (815) — consume_duration, consume_attachments
  - `apply.rs` (491) — apply articulations, dynamics, ornaments
  - `postprocess.rs` (415) — beams, stems, auto-beam grouping
  - `lyrics.rs` (236) — lyric parsing, attachment
  - `figured_bass.rs` (278) — figuremode parsing, distribution
  - `merge.rs` (1085) — merge/synchronize passes, variable resolution
  - `tests.rs` (1976) — all tests

### E2T2: Split `ir_to_ly.rs` (3345 lines) into module directory
- Converted `src/adapters/ir_to_ly.rs` → `src/adapters/ir_to_ly/` with 7 sub-modules:
  - `mod.rs` (383), `maps.rs` (343), `helpers.rs` (67), `parts.rs` (244), `emit.rs` (672), `lyrics.rs` (229), `tests.rs` (1451)

### E2T3: Split `ir_to_mxml.rs` (2832 lines) into module directory
- Converted `src/adapters/ir_to_mxml.rs` → `src/adapters/ir_to_mxml/` with 7 sub-modules:
  - `mod.rs` (98), `score.rs` (301), `part.rs` (368), `note.rs` (417), `direction.rs` (282), `helpers.rs` (121), `tests.rs` (1272)

### E2T4: Split `mxml_to_ir.rs` (2578 lines) into module directory
- Converted `src/adapters/mxml_to_ir.rs` → `src/adapters/mxml_to_ir/` with 6 sub-modules:
  - `mod.rs` (458), `part.rs` (415), `note.rs` (464), `direction.rs` (273), `helpers.rs` (136), `tests.rs` (879)

### E2T5: Split `lower.rs` (1356 lines) into sub-modules
- Converted `src/ir/lower.rs` → `src/ir/lower/` with 5 sub-modules:
  - `mod.rs` (39), `state.rs` (116), `walk.rs` (284), `build.rs` (625), `tests.rs` (329)

### Next up
- **Epic 1: Complete the Two-Layer IR Architecture** — starting with E1T1 (remove Forward/Backup from VoiceElement). Execution order: E0 ✅ → E2 ✅ → E1.
