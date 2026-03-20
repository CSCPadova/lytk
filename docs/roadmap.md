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

## In Progress / Near-term

### Epic 5: Complete LilyPond Parser

| Task | Description | Status |
|------|-------------|--------|
| E5T1 | Add `\chordmode` support | Planned |
| E5T2 | Improve `\figuremode` robustness | Planned |
| E5T3 | Handle `\partial` in multi-movement contexts | Planned |

### Epic 6: Complete MusicXML Parser

| Task | Description | Status |
|------|-------------|--------|
| E6T1 | `<measure-style>` support (multi-rest, slash notation) | Planned |
| E6T2 | `<dashes>` / `<bracket>` spanner support | Planned |
| E6T3 | Non-traditional key signature support | Planned |

### Epic 7: LilyPond Export Completeness

| Task | Description | Status |
|------|-------------|--------|
| E7T1 | Emit lyrics in LilyPond output | Planned |
| E7T2 | Emit repeat structures | Planned |
| E7T3 | Emit `\chordmode` and `\figuremode` | Planned |

---

## Planned (lower priority)

### Epic 8: New Format Adapters

Priority order: ABC first (simplest, many folk datasets), then MEI, then Humdrum.

| Task | Description | Status |
|------|-------------|--------|
| E8T1 | ABC notation parser (`abc_to_ir.rs`) | Planned |
| E8T2 | ABC notation emitter (`ir_to_abc.rs`) | Planned |
| E8T3 | MEI parser (`mei_to_ir.rs`) | Planned |
| E8T4 | MEI emitter (`ir_to_mei.rs`) | Planned |
| E8T5 | Humdrum parser (`hum_to_ir.rs`) — import only | Planned |

### Epic 9: Python Bindings & Distribution

| Task | Description | Status |
|------|-------------|--------|
| E9T1 | Python type stubs (`.pyi`) | Planned |
| E9T2 | Python wrappers for new adapters | Planned |
| E9T3 | maturin GitHub Actions for wheel building | Planned |
| E9T4 | Update pyproject.toml for distribution | Planned |

### music21 Feature Parity (v2)

[music21](https://web.mit.edu/music21/) is the standard Python toolkit for Music
Information Retrieval (MIR) but is slow, poorly designed, and frequently buggy. lytk
aims to provide equivalent or superior analytical capabilities with a clean API and
Rust performance. Planned for v2 or a separate package.

Areas: Pitch & Interval Analysis, Score Analysis (key-finding, ambitus, histograms),
Rhythm & Meter, Harmony & Voice Leading, Melodic Analysis, Data Augmentation Transforms.

---

## Implementation Sequence

| Phase | Epics | Focus |
|-------|-------|-------|
| 1 | E0 ✅ | Stabilization: warnings, test fix, CI |
| 2 | E2 ✅ | Modularity: split all large files |
| 3 | E1 ✅ | Architecture: Forward/Backup removal, parser/emitter rewrite |
| 4 | E3 ✅ + E4 ✅ + E4b ✅ | Test coverage + MIDI as first-class + MIDI reference fidelity |
| 5 | E5 + E6 + E7 | Feature completeness: remaining parser/emitter gaps |
| 6 | E8 (ABC first) | New formats: ABC, then MEI, then Humdrum |
| 7 | E9 + E1T5 | Distribution: Python stubs, wheels, PyPI |

## Key Decisions

- **Split before rewrite:** Split ly_to_ir.rs (E2T1) first as a pure refactor, then rewrite each sub-module to emit Music tree (E1T2). Lower risk, easier to review.
- **Full Forward/Backup removal:** Remove from VoiceElement entirely (E1T1). Convert to spacer rests in mxml_to_ir, generate during serialization in ir_to_mxml. Clean break.
- **Execution order:** E0 → E2 → E1 (stabilize → split → architecture). Safest progression.
- **New formats priority:** ABC first (simplest, many folk datasets), then MEI, then Humdrum. All deferred until core is solid.

## Out of Scope (v1)

These MusicXML elements have no planned IR representation:
- `<harp-pedals>` / `<accordion-registration>` — hardware-specific notation
- `<scordatura>` — tuning override notation
- `<percussion>` pictogram elements — complex symbol table
- `<image>` — embedded raster images
- `<listen>` / `<listening>` — performance instructions (MusicXML 4.0 new)
- `<staff-divide>` arrow — orchestral condensed score notation
