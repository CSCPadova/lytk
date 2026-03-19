# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Latest changes: look at the file docs/changelog.md to know about latest activity on the codebase.

**Development plan and roadmap:** See [`docs/roadmap.md`](docs/roadmap.md) for the full epic/task breakdown, implementation sequence, and key decisions. After an epic/task has been completed update the roadmap file and write the latest changes to the [changelog file](docs/changelog.md), documenting what has been done and what to do next.

## Project

`lytk` — a Rust library (with Python bindings via PyO3/maturin) for music notation conversion and augmentation. Converts between LilyPond, MusicXML, MXL (compressed MusicXML), and MIDI through a shared Internal Representation (IR).

## Build & Test Commands

```bash
cargo build                          # build library + CLI
cargo test                           # all unit tests (324) + CLI integration tests (15)
cargo test <test_name>               # run a single test by name
cargo test --test cli_tests          # CLI integration tests only
cargo test -- --nocapture             # see stdout/eprintln during tests
cargo fmt && cargo clippy -- -D warnings  # lint

# Python (not yet fully wired)
maturin develop                      # rebuild Rust extension for Python
uv sync                              # install Python deps

# CLI usage
cargo run -- convert input.ly -o output.xml
cargo run -- convert input.mxl -o output.ly
cargo run -- info input.xml
cargo run -- transpose input.ly -o output.ly -s 3
cargo run -- flatten input.ly -o output.ly
```

MIDI support requires `cargo build --features midi`.

## Architecture

Two-layer IR:
- **Layer 1 (Music tree):** Format-agnostic recursive tree (`Music` enum — Sequential, Simultaneous, Context, Note, Chord, Rest, Skip, Grace, Tuplet, Repeat, Variable). No measures. Parsers produce this, transforms operate on it.
- **Layer 2 (Score):** Measure-based tree (`Score → Part → Measure → Voice → Note/Rest/Chord/Forward/Backup`). Used for MusicXML/MIDI export.
- **Bridging:** `lift_to_music()` converts Score → Music tree; `lower_to_score()` converts Music tree → Score.

Five layers:

1. **Parser** (`src/parser.rs`, `src/tree-sitter/`) — tree-sitter-lilypond grammar, C sources committed in-repo. Never modify `tree-sitter-lilypond/` (read-only reference).

2. **IR** (`src/ir/`) — central data model:
   - Layer 1: `music.rs` (Music enum, MusicDocument), `annotation.rs`, `moment.rs`
   - Layer 2: `score.rs`, `note.rs`, `measure.rs`, `voice.rs`
   - Shared: `pitch.rs`, `duration.rs`, `direction.rs`, `harmony.rs`, `articulation.rs`, `language.rs`
   - Bridging: `lift.rs` (Score → Music), `lower.rs` (Music → Score)
   - Durations use `Frac` = `num::Ratio<i64>` for precise fractional arithmetic
   - `TimeSignature.beats` is a `String` (supports compound like "3+2"), use `.beats_fraction()` for the `Frac` value

3. **Adapters** (`src/adapters/`) — format converters, all go through IR:
   - `ly_to_ir.rs` (~8000 lines) — LilyPond parser using tree-sitter AST walk. Handles `\relative`, variables, `<< \\ >>` multi-voice, `\include`, figured bass, lyrics
   - `mxml_to_ir.rs` — MusicXML reader (quick-xml SAX-style)
   - `ir_to_ly.rs` (~3400 lines) — IR to LilyPond emitter. Handles multi-staff piano scores, voice filtering, relative pitch mode
   - `ir_to_mxml.rs` (~2800 lines) — IR to MusicXML writer
   - `mxl_zip.rs` — MXL (ZIP-compressed MusicXML) handling
   - `ly_flatten.rs` — `\include` expansion
   - `midi_to_ir.rs` / `ir_to_midi.rs` — behind `midi` feature flag
   - Traits: `ToIrAdapter` (parse → Score), `FromIrAdapter` (Score → emit), `ToMusicAdapter` (parse → MusicDocument), `FromMusicAdapter` (MusicDocument → emit)

4. **Transforms** (`src/transforms/`) — idempotent, composable passes:
   - `transpose`, `invert`, `retrograde`, `language` (pitch language translation)
   - Each implements `Transform` trait (`apply(&self, &Score) -> Score`) and `MusicTransform` trait (`apply_music(&self, &MusicDocument) -> MusicDocument`)

5. **CLI** (`src/main.rs`) — `clap` subcommands: `convert`, `transpose`, `info`, `flatten`. Batch mode uses `rayon` for parallelism.

## Key Design Patterns

- **Variable resolution** in `ly_to_ir`: variables are pre-parsed into `VarDef::Measures(Vec<Measure>, Frac)` storing the time signature at definition time. At resolution, if the current time sig differs, measures are re-split via `resplit_measures_for_time_sig`.
- **Multi-voice** `<< { } \\ { } >>`: detected by `parallel_music_separator` nodes. Each voice branch is walked independently from saved state, then merged into unified measures.
- **Multi-staff parts** (piano): `Part.staves` > 1, voices carry `staff` numbers. `ir_to_ly` filters voices by staff using `voice_matches_staff` + `voice_has_content`. `ir_to_mxml` threads `part_staves` to conditionally emit `<staff>` elements.
- **Post-processing** in `ly_to_ir`: `merge_leading_attribute_measures` (merges key/time-only measures), `merge_dynamics_parts` (folds Dynamics-only parts), `post_process_beams_and_stems`.

### General
- **TDD** — every feature must have tests before implementation.
- **DRY / modularity / composability** — no ad-hoc one-offs; prefer extending the transform/adapter framework.
- **Performance baseline first** — profile `python-ly/` and record the baseline (time, memory). The Rust target must beat it.

### Rust specifics
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

## Test Fixtures

- `tests/fixtures/ly/` — LilyPond source files (pedal, chopin, repeats)
- `tests/fixtures/mxl/` — compressed MusicXML files (10 files covering single voice, chords, multi-part piano, fingering)
- `tests/fixtures/musicxml/` — uncompressed MusicXML
- `tests/fixtures/xml/` — additional XML fixtures
- `musicxmlTestSuite/` — external MusicXML test corpus (read-only)

## Reference Projects (read-only, do not modify)

| Folder | Use for |
|---|---|
| `MuseScore/src/importexport/` | C++ MusicXML, MEI, mscz, MIDI parsing/emitting reference |
| `music21/` | IR design, MIR features, format parsing patterns |
| `python-ly/`, `quickly/` | LilyPond tokenizer/parser reference, Python baseline |
| `abjad/` | Score-building API, visitor patterns |
| `tree-sitter-lilypond/` | Upstream grammar reference — copy into `src/tree-sitter/` to update |
| `symusic/` | Fast MIDI IR reference |
| `PDMX/` | Dataset IR for MusicXML + MSCZ; internal representation reference |
| `lilypond/` | The original lilypond compiler, reference to tokenize and handle ly files |

## Common Pitfalls

- `tree-sitter-lilypond/` is read-only reference; build reads only `src/tree-sitter/`
- MXL files are ZIP archives — `mxl_zip.rs` handles extraction before XML parsing
- LilyPond `\relative` changes pitch semantics — tracked via `in_relative` / `relative_ref` state
- `TimeSignature.beats` is a String, not a number — always use `.beats_fraction()` for arithmetic
- Round-trip fidelity: test semantic equivalence (pitch, duration, structure), not string equality
