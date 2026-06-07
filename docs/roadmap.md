# Roadmap

Items are grouped by status. Completed items are kept for reference.

---

## Completed ✅

### IR Layer
Complete `Score → Part → Voice → Measure → Note/Rest/Chord` tree with `Pitch`, `Duration`,
`Articulation`, `Direction`, `Barline`, `Clef`, `KeySignature`, `TimeSignature`, `Lyric`,
`PartGroup`, `PageLayout`, `Harmony`, `FiguredBass`. Pitch language data for all 11 LilyPond
languages. Full serde serialisation.

### Two-Layer IR Architecture (Layer 1)
Music tree types (`Music` enum: Sequential, Simultaneous, Context, Note, Chord, Rest, Skip,
Grace, Tuplet, Repeat, Variable, etc.), `MusicDocument`, lift/lower passes
(`lift_to_music` / `lower_to_score`), `MusicTransform` trait with implementations for
Transpose, Invert, Retrograde, ChangeLanguage. Adapter bridging via lift/lower.

### MusicXML → IR
Parses all 143 MusicXML Test Suite fixtures. Supports MusicXML 4.0 including harmonies,
figured bass, page layout, coda/segno, da capo/dal segno, glissando, arpeggio.

### LilyPond → IR
Full tree-sitter-based parser: notes, rests, chords, tuplets (including sextuplets), grace notes,
acciaccatura, appoggiatura, after-grace, ties, slurs, beams, articulations, dynamics, wedges,
ornaments, tremolo, glissando, arpeggio, lyrics (`\lyricsto`, `\lyricmode`, melisma `_`),
clef/key/time signatures, repeats with voltas, variables, multi-staff, multi-voice,
`\autoBeamOff`/`\autoBeamOn`, `\markup` text, shorthand symbols, figured bass,
clef-aware auto-stem, tuplet-aware auto-beam grouping.

### IR → MusicXML
Full MusicXML 4.0 emitter. Common/cut time symbols. All articulations, technicals, ornaments,
dynamics, wedges, slurs, ties, tuplets, grace notes, lyrics, harmonies, figured bass,
page layout, coda/segno, da capo/dal segno.

### IR → LilyPond
Emitter with relative pitch, all notation elements, multi-staff, lyrics, voltas.

### LilyPond Flatten (`lytk flatten`)
Recursive `\include` expander:
- Circular dependency detection (hard error with ancestor chain)
- Extension fallback (`.ly` → `.ily`)
- Extra search paths (`-I`)
- `\version`/`\language` deduplication with warning (last occurrence wins)
- Multiple `\header` blocks → hard error
- `% === BEGIN/END INCLUDE ===` comment markers (suppressible with `--no-markers`)

### Transforms
`Transpose`, `ChangeLanguage`, `Invert`, `Retrograde`. Composable via `apply_all`.
Dual OOP + functional API. Both `Transform` (Score) and `MusicTransform` (MusicDocument) traits.

### MIDI Adapter
`midly`-based `MidiToIr` and `IrToMidi`, included by default. `ToMusicAdapter`/`FromMusicAdapter`
bridging via lift/lower passes.

### CLI
`convert`, `transpose`, `info`, `flatten` subcommands. Batch mode with rayon (`-j`).
Format auto-detection. Multi-movement output (`_01`, `_02`, …).

### Python Bindings
Full PyO3 API: `from_musicxml`, `from_lilypond`, `to_musicxml`, `to_lilypond`,
`from_midi`/`to_midi`, `transpose`, `change_language`, `invert`,
`retrograde`, `Score.to_json`/`from_json`/`to_dict`/`from_dict`.

### Benchmarks
25 Criterion benchmarks; 16 pytest-benchmark tests. ~52× faster than python-ly for
transpose, ~40× for language change.

### Epic 0: Housekeeping & Stabilization ✅
- **E0T1:** Fixed all 76 clippy warnings (assign_op_pattern, unused imports/vars, clone on Copy, boxed Direction variant, etc.)
- **E0T2:** Removed 12 debug `eprintln!` calls from `ly_to_ir.rs` and `lower.rs`
- **E0T3:** Fixed `test_figuremode_distribution_across_measures` — root cause: `distribute_figured_bass` placed figures into leading attribute-only measures that later get drained by `merge_leading_attribute_measures`. Fix: skip leading empty measures in distribution; carry over figured bass during merge.
- **E0T4:** Added gitignore patterns for scratch/test output files at root level
- **E0T5:** Created `.github/workflows/ci.yml`: fmt, clippy (with midi feature), test (Linux/macOS/Windows), Python tests

### Epic 1: Complete the Two-Layer IR Architecture ✅
Music tree types (`Music` enum), `MusicDocument`, lift/lower passes, `MusicTransform` trait
(Transpose, Invert, Retrograde, ChangeLanguage), direct Music tree → LilyPond emitter,
CLI LY→LY via Music tree path, Python `MusicDocument` bindings.

### Epic 2: Break Up Monolithic Files ✅
Split `ly_to_ir.rs` (~8000→module dir), `ir_to_ly.rs` (~3400→module dir),
`ir_to_mxml.rs` (~2800→module dir), `mxml_to_ir.rs` (~2600→module dir),
`lower.rs` (~1400→sub-modules). Pure refactor, no functional changes.

### Epic 3: Test Coverage ✅
Expanded test suite from 357 to 691 tests:
- **E3T1:** mxml_to_ir unit tests — 50 new tests (20→70 total)
- **E3T2:** ir_to_mxml unit tests — 14 new tests (55→69 total)
- **E3T3:** ly_to_ir unit tests — 20 new tests (81→101 total)
- **E3T4:** Fixture regression tests — 209 tests covering all 143 XML, 10 MXL, 35 LY fixtures + cross-format (XML→LY, XML→MusicXML)
- **E3T5:** Round-trip testing framework — 25 integration tests (MusicXML↔Score, LilyPond↔Score, cross-format, fixture-based)
- **E3T6:** Property-based tests with proptest — 20 tests for Pitch/Duration/Transform algebraic properties (transpose round-trip, invert self-inverse, retrograde self-inverse, duration monotonicity)

---

## Completed ✅

### Epic 4: MIDI as First-Class ✅

**Goal:** Remove feature gate, add full test coverage.

| Task | Description | Status |
|------|-------------|--------|
| E4T1 | Move midly to default dependency, remove feature gates | ✅ Done |
| E4T2 | MIDI round-trip tests | ✅ Done |
| E4T3 | ToMusicAdapter/FromMusicAdapter for MIDI | ✅ Done |

### Epic 4b: MIDI Reference Fidelity ✅

**Goal:** Fix systematic differences between our MIDI output and LilyPond reference MIDIs.

| Task | Description | Status |
|------|-------------|--------|
| E4bT1 | Fix chord relative pitch drift (first note of chord inherits wrong relative ref) | ✅ Done |
| E4bT2 | Fix tempo beat-unit handling (dotted quarter tempos, beat-unit→quarter conversion) | ✅ Done |
| E4bT3 | GM instrument→program lookup (`gm_program_from_name`, scheme string `\set` handling) | ✅ Done |
| E4bT4 | Change default PPQN from 480 to 384 to match LilyPond | ✅ Done |
| E4bT5 | MIDI reference regression tests (14 tests across 4 fixtures: pedal, example, example2, chopin) | ✅ Done |

### Epic 4c: Replace quick-xml with musicxml crate ✅

**Goal:** Switch MusicXML I/O from manual quick-xml SAX/Writer + zip to the typed `musicxml` crate (v1.1.2).

| Task | Description | Status |
|------|-------------|--------|
| E4cT1 | Rewrite `mxml_to_ir` reading path — use `musicxml::read_score_partwise` | ✅ Done |
| E4cT2 | Rewrite `ir_to_mxml` writing path — build `ScorePartwise` structs, serialize via `musicxml::write_partwise_score_data` | ✅ Done |
| E4cT3 | Cleanup — delete `mxl_zip.rs`, remove `quick-xml`/`zip` deps, remove `Xml`/`XmlAttr`/`Zip` error variants | ✅ Done |

---

# v1.0.0 Milestone — "LilyPond as a first-class symbolic-music-for-ML format"

**Theme:** ship lytk as a credible alternative to music21 + muspy for symbolic-music ML:
faithful LilyPond ↔ MusicXML ↔ MIDI conversion, a new ABC adapter, muspy-style ML
representations (note-array / event / piano-roll) with numpy interop, dataset loaders,
objective metrics, and a semantic round-trip test bar that proves fidelity rather than
just "it parses". Reference: `muspy/` (representations, datasets, metrics).

Status legend: ⬜ not started · 🟡 in progress · ✅ done

### Epic A: Stabilization & Baseline

| Task | Description | Status |
|------|-------------|--------|
| EAT1 | Resolve merge conflicts in `ly_to_ir/consume.rs` (keep bow/harmonic articulations) + `merge.rs` | ✅ |
| EAT2 | Build + capture test baseline (435 lib / 19 CLI / 208 fixture / 18-of-20 proptest; 2 known double-flat transpose failures) | ✅ |
| EAT3 | Add `muspy` to `CLAUDE.md` reference table | ✅ |
| EAT4 | Fix stale "feature-gated behind `midi`" wording in `docs/import-export.md` | ✅ |
| EAT5 | Fixture error-tracking harness (`tests/fixture_audit.rs`) — full convert matrix over all 195 fixtures, report errors/panics per stage; baseline: 0 errors, 0 panics (semantic loss not yet caught — see Epic C) | ✅ |

### Epic B: Conversion Fidelity (TDD — failing semantic test first)

| Task | Description | Status |
|------|-------------|--------|
| EBT1 | Emit **lyrics** in IR→LY (currently dropped, `import-export.md` L221) | ⬜ |
| EBT2 | **Repeat/volta** LY→IR→LY round-trip (`\repeat volta` + `\alternative`) | ⬜ |
| EBT3 | `\chordmode` import → `Harmony` IR (emitter side already exists) | ⬜ |
| EBT4 | `\figuremode` robustness | ⬜ |
| EBT5 | MIDI **velocity ↔ dynamics** mapping both directions (replace fixed velocity 80) | ⬜ |
| EBT6 | `\partial` in multi-movement contexts | ⬜ |

### Epic C: Semantic Round-Trip Test Bar (quality gate)

| Task | Description | Status |
|------|-------------|--------|
| ECT1 | Comparators for durations, dynamics, articulations, ties/slurs, lyrics, repeat/volta, time/key sig (extend `tests/round_trip.rs` helpers) | ⬜ |
| ECT2 | Per-fixture semantic round-trip tests (LY↔IR↔LY, XML↔IR↔XML, XML→LY) | ⬜ |
| ECT3 | Wire fixture-audit report into CI as a tracked, non-increasing metric | ⬜ |

### Epic D: ML Representations (`src/representations/`, modeled on muspy)

| Task | Description | Status |
|------|-------------|--------|
| EDT1 | Note-array `(onset, duration, pitch, velocity)` ↔ IR | ⬜ |
| EDT2 | Event sequence (note-on/off, time-shift, velocity-set) + documented vocabulary | ⬜ |
| EDT3 | Piano-roll dense `T×128` (configurable resolution) ↔ IR | ⬜ |
| EDT4 | numpy interop via PyO3 (`numpy` crate) + `.pyi` stubs | ⬜ |
| EDT5 | Round-trip tests (note-array exact; event exact; piano-roll quantization-aware) | ⬜ |

### Epic E: ABC Adapter (new format)

| Task | Description | Status |
|------|-------------|--------|
| EET1 | `abc_to_ir.rs` parser (`ToMusicAdapter`) | ⬜ |
| EET2 | `ir_to_abc.rs` emitter (`FromMusicAdapter`) | ⬜ |
| EET3 | CLI wiring + ABC fixtures + semantic round-trip test | ⬜ |

### Epic F: Datasets & Metrics (ML pipeline)

| Task | Description | Status |
|------|-------------|--------|
| EFT1 | Dataset classes (`src/lytk/datasets/`): base `Dataset`, generic `FolderDataset`, one remote dataset (e.g. JSB Chorales); torch/tf adapters (lazy import) | ⬜ |
| EFT2 | train/val/test split + on-disk caching of converted representations | ⬜ |
| EFT3 | Objective metrics (`src/representations/metrics.rs` + Python): pitch-class histogram/entropy, n-PC rate, polyphony, empty-beat rate, scale & groove consistency | ⬜ |
| EFT4 | Tests: folder → dataset → batch tensor shapes; metrics on hand-built fixtures | ⬜ |

### Epic G: Python Distribution & Docs (release readiness)

| Task | Description | Status |
|------|-------------|--------|
| EGT1 | Complete `.pyi` stubs for `_core` incl. representations | ⬜ |
| EGT2 | Python wrappers for ABC + representations | ⬜ |
| EGT3 | maturin GitHub Actions wheel matrix (Linux/macOS/Windows, abi3) | ⬜ |
| EGT4 | `pyproject.toml` metadata, README quickstart, finalize `import-export.md` matrix | ⬜ |
| EGT5 | Tag **v1.0.0**; update roadmap (Completed) + changelog | ⬜ |

---

## Implementation Sequence

| Phase | Epics | Focus |
|-------|-------|-------|
| 1 | E0 ✅ | Stabilization: warnings, test fix, CI |
| 2 | E2 ✅ | Modularity: split all large files |
| 3 | E1 ✅ | Architecture: Forward/Backup removal, parser/emitter rewrite |
| 4 | E3 ✅ + E4 ✅ + E4b ✅ + E4c ✅ | Test coverage + MIDI first-class + MIDI fidelity + musicxml crate |
| **5** | **A** | **Unblock build, baseline, audit harness, doc hygiene** |
| **6** | **B + C** | **Conversion fidelity + semantic round-trip bar (interleaved, test-first)** |
| **7** | **D** | **ML representations (note-array, event, piano-roll, numpy)** |
| **8** | **E + F** | **ABC adapter + datasets & metrics (parallel, both build on D)** |
| **9** | **G** | **Distribution: stubs, wheels, docs → tag v1.0.0** |

## Key Decisions

- **Split before rewrite:** Split ly_to_ir.rs (E2T1) first as a pure refactor, then rewrite each sub-module to emit Music tree (E1T2). Lower risk, easier to review.
- **Full Forward/Backup removal:** Remove from VoiceElement entirely (E1T1). Convert to spacer rests in mxml_to_ir, generate during serialization in ir_to_mxml. Clean break.
- **Fidelity before features:** v1.0.0 prioritizes making the existing three formats round-trip *semantically* (Epics B/C) before adding ML surface area. A real test bar prevents "parses-but-wrong" regressions.
- **ML representations are in-scope for v1.0.0:** the stated goal is symbolic-music generation/understanding, so note-array/event/piano-roll + datasets + metrics ship in v1.0.0 (modeled on muspy), not deferred.
- **Representations go through the Music tree**, not Score — format-agnostic, reuses `Frac` durations and `moment.rs` offsets.
- **New formats priority:** ABC first (simplest, many folk datasets). MEI and Humdrum stay deferred past v1.0.0.

## Deferred past v1.0.0

### Epic 8 (remainder): MEI & Humdrum adapters
MEI parser/emitter (`mei_to_ir.rs` / `ir_to_mei.rs`), Humdrum import (`hum_to_ir.rs`).
Reference material in `MEILER/`, `hum2ly/`.

### Audio rendering / synthesis (v1.1)
Listen-back via a synthesizer (reference muspy/symusic synth).

### music21 Feature Parity (v2)

[music21](https://web.mit.edu/music21/) is the standard Python toolkit for Music
Information Retrieval (MIR) but is slow, poorly designed, and frequently buggy. lytk
aims to provide equivalent or superior analytical capabilities with a clean API and
Rust performance. Planned for v2 or a separate package.

Areas: Pitch & Interval Analysis, Score Analysis (key-finding, ambitus, histograms),
Rhythm & Meter, Harmony & Voice Leading, Melodic Analysis, Data Augmentation Transforms.

## Out of Scope (v1)

These MusicXML elements have no planned IR representation:
- `<harp-pedals>` / `<accordion-registration>` — hardware-specific notation
- `<scordatura>` — tuning override notation
- `<percussion>` pictogram elements — complex symbol table
- `<image>` — embedded raster images
- `<listen>` / `<listening>` — performance instructions (MusicXML 4.0 new)
- `<staff-divide>` arrow — orchestral condensed score notation
