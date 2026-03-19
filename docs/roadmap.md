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

### MIDI Adapter (optional)
`midly`-based `MidiToIr` and `IrToMidi` behind `--features midi`.

### CLI
`convert`, `transpose`, `info`, `flatten` subcommands. Batch mode with rayon (`-j`).
Format auto-detection. Multi-movement output (`_01`, `_02`, …).

### Python Bindings
Full PyO3 API: `from_musicxml`, `from_lilypond`, `to_musicxml`, `to_lilypond`,
`from_midi`/`to_midi` (feature-gated), `transpose`, `change_language`, `invert`,
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

---

## In Progress / Near-term


### Epic 2: Break Up Monolithic Files ✅

**Goal:** Split large files into focused, testable modules. Pure refactor — no functional changes.

| Task | Description | Status |
|------|-------------|--------|
| E2T1 | Split `ly_to_ir.rs` (~8000 lines) into module directory | ✅ |
| E2T2 | Split `ir_to_ly.rs` (~3400 lines) into module directory | ✅ |
| E2T3 | Split `ir_to_mxml.rs` (~2800 lines) into module directory | ✅ |
| E2T4 | Split `mxml_to_ir.rs` (~2600 lines) into module directory | ✅ |
| E2T5 | Split `lower.rs` (~1400 lines) into sub-modules | ✅ |

### Epic 1: Complete the Two-Layer IR Architecture ✅

**Goal:** Finish the architectural vision — Music tree as primary IR, Score only for export.

| Task | Description | Status |
|------|-------------|--------|
| E1T1 | Remove Forward/Backup from VoiceElement — spacer rests replace Forward, ir_to_mxml emits `<forward>` for spacers | ✅ |
| E1T2 | Interface inversion — Music tree as recommended path, Score path preserved for performance; full native parser deferred | ✅ |
| E1T3 | Direct Music tree → LilyPond emitter (`music_emit.rs`, 530 lines, 17 tests) | ✅ |
| E1T4 | CLI LY→LY conversion routed through Music tree path | ✅ |
| E1T5 | Python bindings: `MusicDocument` class, `from_lilypond_music`, `to_lilypond_music` | ✅ |

### Epic 3: Test Coverage

**Goal:** Comprehensive unit, integration, round-trip, and property-based tests.

| Task | Description | Status |
|------|-------------|--------|
| E3T1 | Unit tests for `mxml_to_ir` parsing functions | Planned |
| E3T2 | Unit tests for `ir_to_mxml` emission functions | Planned |
| E3T3 | Unit tests for `ly_to_ir` sub-parsers | Planned |
| E3T4 | Expand fixture-based regression tests | Planned |
| E3T5 | Round-trip testing framework (MusicXML↔Score, LilyPond↔Score) | Planned |
| E3T6 | Property-based tests with proptest | Planned |

### Epic 4: MIDI as First-Class

**Goal:** Remove feature gate, add full test coverage.

| Task | Description | Status |
|------|-------------|--------|
| E4T1 | Move midly to default dependency, remove feature gates | Planned |
| E4T2 | MIDI round-trip tests | Planned |
| E4T3 | ToMusicAdapter/FromMusicAdapter for MIDI | Planned |

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
| 3 | E1 + E4T1 | Architecture: Forward/Backup removal, parser/emitter rewrite, MIDI ungating |
| 4 | E3 + E4T2-3 | Test coverage: unit, round-trip, proptest, MIDI tests |
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
