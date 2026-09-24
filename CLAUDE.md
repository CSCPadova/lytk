# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Latest changes: look at the file docs/changelog.md to know about latest activity on the codebase.

**Development plan and roadmap:** See [`docs/roadmap.md`](docs/roadmap.md) for the full epic/task breakdown, implementation sequence, and key decisions. After an epic/task has been completed update the roadmap file and write the latest changes to the [changelog file](docs/changelog.md), documenting what has been done and what to do next.

## Project

`lytk` — a Rust library (with Python bindings via PyO3/maturin) for music notation conversion and augmentation. Converts between LilyPond, MusicXML, MXL (compressed MusicXML), MIDI, ABC, and Humdrum (`**kern`) through a shared Internal Representation (IR).

## Build & Test Commands

```bash
cargo build                          # build library + CLI
cargo test                           # all tests (~1050 Rust; plus ~140 Python via pytest)
cargo test <test_name>               # run a single test by name
cargo test --test cli                # CLI integration tests only
cargo test -- --nocapture             # see stdout/eprintln during tests
cargo fmt && cargo clippy --all-targets -- -D warnings  # lint

# Python
maturin develop                      # rebuild Rust extension for Python
uv sync                              # install Python deps

# CLI usage (the Python `lytk` command; after `maturin develop`)
lytk convert input.ly -o output.xml
lytk convert input.mxl -o output.ly
lytk info input.xml
lytk transpose input.ly -o output.ly -s 3
lytk flatten input.ly -o output.ly
pytest tests/test_cli.py             # CLI tests
```

MIDI support is included by default.

## Architecture

Two-layer IR:
- **Layer 1 (Music tree):** Format-agnostic recursive tree (`Music` enum — Sequential, Simultaneous, Context, Note, Chord, Rest, Skip, Grace, Tuplet, Repeat, Variable). No measures. Parsers produce this, transforms operate on it.
- **Layer 2 (Score):** Measure-based tree (`Score → Part → Measure → Voice → Note/Rest/Chord/Forward/Backup`). Used for MusicXML/MIDI export.
- **Bridging:** `lift_to_music()` converts Score → Music tree; `lower_to_score()` converts Music tree → Score.

Six layers:

1. **Parser** (`src/parser.rs`, `src/tree-sitter/`) — tree-sitter-lilypond grammar, C sources committed in-repo. Never modify `tree-sitter-lilypond/` (read-only reference).

2. **IR** (`src/ir/`) — central data model:
   - Layer 1: `music.rs` (Music enum, MusicDocument), `annotation.rs`, `moment.rs`
   - Layer 2: `score.rs`, `note.rs`, `measure.rs`, `voice.rs`
   - Shared: `pitch.rs`, `duration.rs`, `direction.rs`, `harmony.rs`, `articulation.rs`, `language.rs`
   - Bridging: `lift.rs` (Score → Music), `lower.rs` (Music → Score)
   - Durations use `Frac` = `num::Ratio<i64>` for precise fractional arithmetic
   - `TimeSignature.beats` is a `String` (supports compound like "3+2"), use `.beats_fraction()` for the `Frac` value

3. **Adapters** (`src/adapters/`) — format converters, all go through IR:
   - `ly_to_ir/` — LilyPond parser using tree-sitter AST walk. Handles `\relative`, variables, `<< \\ >>` multi-voice, `\include`, figured bass, lyrics. The walk writes a positioned `Timeline` per part (`ly_to_ir/timeline.rs`); measures are made once, at score assembly
   - `mxml_to_ir/` — MusicXML reader using `musicxml` crate (typed struct traversal). Handles `.xml` and `.mxl` natively.
   - `ir_to_ly/` (~3400 lines) — IR to LilyPond emitter. Handles multi-staff piano scores, voice filtering, relative pitch mode
   - `ir_to_mxml/` — IR to MusicXML writer using `musicxml` crate (struct construction + serialization). Native MXL support.
   - `ly_flatten.rs` — `\include` expansion
   - `midi_to_ir.rs` / `ir_to_midi.rs` — MIDI I/O via `midly`
   - `abc_to_ir.rs` / `ir_to_abc.rs` — ABC notation (Layer-1 Music tree)
   - `humdrum_to_ir.rs` / `ir_to_humdrum.rs` — Humdrum `**kern` (Layer-1 Music tree; spine rearrangement `*^`/`*v` unsupported → clear error)
   - Traits: `ToIrAdapter` (parse → Score), `FromIrAdapter` (Score → emit), `ToMusicAdapter` (parse → MusicDocument), `FromMusicAdapter` (MusicDocument → emit)

4. **Transforms** (`src/transforms/`) — idempotent, composable passes:
   - `transpose`, `invert`, `retrograde`, `language` (pitch language translation)
   - Each implements `Transform` trait (`apply(&self, &Score) -> Score`) and `MusicTransform` trait (`apply_music(&self, &MusicDocument) -> MusicDocument`)

5. **ML representations** (`src/representations/`) — muspy-style encodings over the Layer-1 Music tree: `note_array` (onset/duration/pitch/velocity rows), `event_sequence` (Performance-RNN-style event codes), `piano_roll` (T × 128 matrix), `metrics`. All exposed to Python (`to_note_array`, `to_piano_roll`, `to_event_sequence` + inverses). Structured note navigation lives in `src/navigation.rs`.

6. **CLI** (`src/lytk/cli.py`) — the one `lytk` command, a Typer app over the Python bindings: `convert`, `transpose` (`-s`/`--interval`/`--to-key`), `invert`, `retrograde`, `change-language`, `abs2rel`, `rel2abs`, `info`, `positions`, `bundle`, `batch`, `diff`, `flatten`. Folder conversion and `batch` run in worker processes (`ProcessPoolExecutor`). There is no Rust binary: the crate is a library only. The Python package `src/lytk/` also ships datasets (`lytk.datasets`) and eval metrics (`lytk.metrics`).

## Key Design Patterns

- **Positioned reading** in `ly_to_ir`: the walk never builds measures. Notes go into voice *lanes* at their absolute onset; `\time`, `\key`, `\clef`, directions, barlines, harmonies, figures, `\partial` and `\cadenzaOn/Off` are events at positions (`timeline::Event`). A run that would overlap music already in its lane moves to the next free lane (`Timeline::place_run`).
- **One bar-splitter**: `assemble_score` builds a score-wide `Grid` (meter grid anchored at the start, `\partial` and each `\time`; explicit barlines add a boundary; a cadenza span is one free bar) and `timeline::split` cuts every part on it. Bar checks `|` only check. Never re-bar measures after the fact.
- **Variable resolution**: a music variable is pre-parsed from position 0 into `VarDef::Music { tl, len, … }` and spliced at the current position; a `\new Staff` variable is `VarDef::Parts`.
- **Multi-voice** `<< { } \\ { } >>` and `<< {…} {…} >>`: every branch starts at the block's start; branch *k* of a `\\` block writes lane *k*. Simultaneous music never needs merging.
- **Multi-staff parts** (piano): `Part.staves` > 1, voices carry `staff` numbers. `ir_to_ly` filters voices by staff using `voice_matches_staff` + `voice_has_content`. `ir_to_mxml` threads `part_staves` to conditionally emit `<staff>` elements.
- **Assembly** (`ly_to_ir::assemble_score`): spacer-only lanes and `\new Dynamics` parts fold into directions; PianoStaff staves are unioned with staff numbers (`merge_piano_staff_parts`); lyrics attach after splitting; then `post_process_beams_and_stems`, ties, slurs, clefs.

### General
- **TDD** — every feature must have tests before implementation.
- **DRY / modularity / composability** — no ad-hoc one-offs; prefer extending the transform/adapter framework.
- **Performance baseline first** — profile `python-ly/` and record the baseline (time, memory). The Rust target must beat it.

### Rust specifics
- Avoid unnecessary `clone()` on large trees (known hotspots: variable splicing in `ly_to_ir/state.rs`, `Timeline::splice`).
- Cross-language ABI: expose C-compatible types where needed; use `abi3` for Python.
- Define `benches/` with `criterion` benchmarks for all hot paths.

### Python specifics
- The CLI is Python (Typer). A new command-line feature needs whatever it calls exposed in `src/python.rs` first; don't add a Rust binary.
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
- `lilypond/input/regression/musicxml/` — external MusicXML acid-test corpus (read-only)

## Reference Projects (read-only, do not modify)

| Folder | Use for |
|---|---|
| `MuseScore/src/importexport/` | C++ MusicXML, MEI, mscz, MIDI parsing/emitting reference |
| `music21/` | IR design, MIR features, format parsing patterns |
| `python-ly/`, `quickly/` | LilyPond tokenizer/parser reference, Python baseline |
| `abjad/` | Score-building API, visitor patterns |
| `tree-sitter-lilypond/` | Upstream grammar reference — copy into `src/tree-sitter/` to update |
| `symusic/` | Fast MIDI IR reference |
| `muspy/` | Symbolic-music-for-ML reference: IR (`Music`/`Track`/`Note`), ML representations (note-array, event sequence, piano-roll), dataset loaders, objective metrics |
| `PDMX/` | Dataset IR for MusicXML + MSCZ; internal representation reference |
| `lilypond/` | The original lilypond compiler, reference to tokenize and handle ly files |

## Common Pitfalls

- `tree-sitter-lilypond/` is read-only reference; build reads only `src/tree-sitter/`
- MXL files are ZIP archives — the `musicxml` crate handles them natively via `read_score_partwise()` / `write_partwise_score()`
- LilyPond `\relative` changes pitch semantics — tracked via `in_relative` / `relative_ref` state
- `TimeSignature.beats` is a String, not a number — always use `.beats_fraction()` for arithmetic
- Round-trip fidelity: test semantic equivalence (pitch, duration, structure), not string equality
