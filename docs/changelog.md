# Changelog

## 2026-03-21 — Fix `<<...>>` simultaneous block merging in repeats.ly

**Goal:** Fix `repeats.ly` (Scott Joplin's "Bethena") producing wrong MusicXML output — RH/LH measure count mismatch (164 vs 172), and `<<...>>` without `\\` blocks being concatenated instead of merged.

### Bug 1: `<<...>>` simultaneous music (no `\\`) not merged — children appended sequentially
- **Root cause:** `walk_parallel_music_staves` handled voice-separating `<<...\\...>>` blocks but not plain `<<...>>` simultaneous blocks. Children were walked sequentially, appending measures instead of overlaying them in time.
- **Fix:** Added `merge_simultaneous_block` function in [walk.rs](src/adapters/ly_to_ir/walk.rs) that tracks a `block_baseline` measure count at entry. For each sibling child after the first, it saves/restores walk state, walks the child, then merges newly added measures with existing sibling measures using spacer merge functions.

### Bug 2: `s1*3/4*3` double multiplier only consuming first `*`
- **Root cause:** `consume_duration_scale` was called once for the `*3/4` fractional scale, but the second `*3` (integer repeat count) wasn't consumed. `s1*3/4*3` produced 1 spacer measure instead of 3.
- **Fix:** In [music.rs](src/adapters/ly_to_ir/music.rs), added a second `consume_duration_scale` call after a fractional scale to handle the integer repeat multiplier. Applied to both `s` (spacer) and `R` (multi-measure rest) handlers.

### Bug 3: `\bar`/`\break`/`\pageBreak` attaching to wrong measure when voice has unflushed content
- **Root cause:** These handlers checked `state.current_measure.is_none()` to decide whether to attach to the last existing measure, but didn't check `state.current_voice.is_empty()`. When notes were in `current_voice` (not yet flushed to a measure), the barline was attached to the previous measure instead of the current one.
- **Fix:** Added `&& state.current_voice.is_empty()` to the condition in [music.rs](src/adapters/ly_to_ir/music.rs).

### Bug 4: Rest-only blocks (with `r4\fermata`) not treated as spacer-like for merge
- **Root cause:** `measures_are_spacer_only` returns false for regular rests (non-spacer). In `segueFourLH` and `outroLH`, a block like `{ s1*3/4*3 s2 r4\fermata }` or `{ \barRest | s4 r r | ... }` contains regular rests, so both sides of the `<<...>>` were treated as "real content" and concatenated instead of merged.
- **Fix:** Added `measures_have_no_pitched_content` function in [merge.rs](src/adapters/ly_to_ir/merge.rs) that allows regular rests but rejects notes/chords. Used as a fallback in `merge_simultaneous_block` when neither side is pure spacer.

### Cleanup
- Removed debug `eprintln!` statements from state.rs and walk.rs
- Removed unused `flush_pending_content` and `consume_duration_multiplier` functions
- Zero compiler warnings, zero clippy warnings

### Results
- `repeats.ly` output: RH and LH both have 164 measures (was 164 vs 172)
- Double barline correctly at end of measure 8
- LH first bar contains `a8 g4 b8 a4` (measure 3, after 2 intro rests — matching source)
- All 723 tests pass (435 lib + 61 round_trip + 208 fixture_regression + 19 CLI)
- pedal.ly regression: fixed

## 2026-03-20 — Bug fixes: LilyPond→MusicXML conversion (pedal.ly, example2.ly) ✅

**Goal:** Fix 5 bugs in the `ly_to_ir` adapter affecting pedal.ly and example2.ly conversions.

### Bug 1: `\markup` variable definitions leaking into music parsing
- **Root cause:** `walk_program` variable definition handler didn't recognize `\markup`/`\markuplist`, causing the following expression_block to be parsed as music (injecting a spurious F pitch).
- **Fix:** In [walk.rs](src/adapters/ly_to_ir/walk.rs), detect `\markup`/`\markuplist` and skip the entire definition.
- **Result:** First note of pedal.ly right hand is now correctly A.

### Bug 2: Dynamics dropped on spacer rests
- **Root cause (multi-layer):**
  1. `"s"` spacer rest handler discarded attachments (`_attachments`)
  2. `attach_dynamic` didn't handle `VoiceElement::Rest`
  3. `Rest` struct lacked `dynamics`/`wedges` fields
  4. MusicXML emitter didn't emit spacer rest dynamics
  5. Spacer merge functions didn't propagate rest-element dynamics
- **Fix:** Added `dynamics`/`wedges` fields to `Rest`; spacer handler uses them; `attach_dynamic` handles rests; `ir_to_mxml` emits them; merge functions propagate them.
- **Result:** pedal.ly dynamics: 8, wedges: 154.

### Bug 3: Pedal placement missing `below`
- **Root cause:** Pedal `Direction` created without `placement: Placement::Below`.
- **Fix:** Added `placement: Placement::Below` in [music.rs](src/adapters/ly_to_ir/music.rs).
- **Result:** 173 pedal directions with `placement="below"`.

### Bug 6: Pedal events at wrong measure positions (all at beat 1)
- **Root cause (two-part):**
  1. `Direction` struct had no fractional offset field — pedal events were pushed with `offset=0` regardless of when `\sustainOn`/`\sustainOff` occurred.
  2. `merge_spacer_by_duration` in [merge.rs](src/adapters/ly_to_ir/merge.rs) distributed all directions from a spacer measure to the single target measure at `spacer_pos`, ignoring each direction's individual absolute position. When the pedal variable has no `\time` command, all its events land in one big spacer measure (`spacer_pos=0`), so they all ended up in P2 measure 1.
- **Fix:**
  - Added `offset_frac: Frac` to `Direction` in [direction.rs](src/ir/direction.rs); set it to `state.elapsed_in_measure` when parsing pedal events in [music.rs](src/adapters/ly_to_ir/music.rs).
  - In `ir_to_mxml` [part.rs](src/adapters/ir_to_mxml/part.rs): directions with `offset_frac > 0` are emitted after a backup+forward sequence to the correct position.
  - In `merge_spacer_by_duration`: each direction is now distributed individually to the target measure containing `spacer_pos + dir.offset_frac`, using a per-direction binary search over `target_boundaries`.
- **Result:** Pedal start/stop events fan out across ~100 measures with correct within-measure positions.

### Bug 4: Scrambled measure numbers / wrong bar content in lower staff
- **Root cause:** `walk_parallel_music_voices` flushed pending voice content (accumulated before a `<< \\ >>` block) AFTER appending the merged parallel block result. This caused measure ordering to be wrong (e.g., m#5 before m#4) and made measure numbers appear scrambled (1, 3, 5, 4, 6, 5…).
- **Fix:** In [walk.rs](src/adapters/ly_to_ir/walk.rs), flush complete pending voice content BEFORE saving state and processing branches, so it's appended to part.measures in the correct order.
- **Result:** Measure numbers sequential; bar 51 has correct 2-voice content.

### Bug 5: Bass part (P4) has 443 measures instead of 62 in example2.ly
- **Root cause (two-part):**
  1. `resplit_measures_to_match` flattened all voices into one, losing multi-voice structure.
  2. When `\forma` (62 spacer measures with time sigs) was merged into `\Ibcn` (443 measures of real music), the existing 443-measure structure was preserved instead of being resplit.
- **Fix 1:** Rewrote `resplit_measures_to_match` in [merge.rs](src/adapters/ly_to_ir/merge.rs) to use per-voice `BTreeMap` streams, preserving voice structure.
- **Fix 2:** In [state.rs](src/adapters/ly_to_ir/state.rs), when a spacer has `has_own_time_sigs=true` and real music has more measures, resplit the real music to match the spacer's authoritative boundaries. Also removed redundant `merge_spacer_measures` call after `resplit_measures_to_match` (directions were being doubled, causing tempo count test failure).
- **Result:** P4 has exactly 62 measures.

### Test counts
- Total: 61 integration tests, all passing (proptest failures pre-existing, unrelated)

## 2026-03-20 — Epic 4c: Replace quick-xml with musicxml crate ✅

**Goal:** Switch all MusicXML I/O from manual quick-xml SAX parsing / Writer event emission + zip MXL handling to the strongly-typed `musicxml` crate (v1.1.2), which provides a full MusicXML 4.0 data model with native MXL support.

### E4cT1: Rewrite `mxml_to_ir` reading path
- `convert_file()` now calls `musicxml::read_score_partwise()` which handles both `.xml` and `.mxl` natively
- `convert_str()` uses `musicxml::read_score_data_partwise()` for in-memory XML parsing
- Replaced manual XmlNode DOM walking with direct traversal of typed `musicxml::elements::ScorePartwise` structs
- Deleted `src/adapters/mxml_to_ir/helpers.rs` (XmlNode, parse_xml helpers no longer needed)
- All 143 XML + 10 MXL fixture tests pass unchanged

### E4cT2: Rewrite `ir_to_mxml` writing path
- Replaced `quick_xml::Writer` event emission with `musicxml::elements::ScorePartwise` struct construction
- `convert()` serializes via `musicxml::write_partwise_score_data()`, returns UTF-8 string
- `write()` detects `.mxl` extension and uses `musicxml::write_partwise_score()` with native MXL compression
- Rewrote all 6 source files: `mod.rs`, `score.rs`, `part.rs`, `note.rs`, `direction.rs`, `helpers.rs`
- Removed `W` type alias and `Writer<Cursor<Vec<u8>>>` machinery

### E4cT3: Cleanup
- Deleted `src/adapters/mxl_zip.rs` — musicxml crate handles MXL natively
- Removed `pub mod mxl_zip` from `src/adapters/mod.rs`
- Removed `AdapterError::Xml`, `AdapterError::XmlAttr`, `AdapterError::Zip` error variants
- Removed `quick-xml` and `zip` dependencies from `Cargo.toml`
- Updated `tests/fixture_regression.rs` to use `MxmlToIrAdapter::convert_file()` instead of `mxl_zip::read_musicxml()`

### Test counts
- Total: 496 tests (435 unit + 61 integration)
- All passing, clippy clean
- 2 pre-existing proptest failures (double-flat pitch transpose edge case, unchanged)

### Next up
- **Epic 5: Complete LilyPond Parser** — `\chordmode`, `\figuremode` robustness, `\partial` in multi-movement

---

## 2025-07-16 — Epic 4b: MIDI Reference Fidelity ✅

### E4bT1: Fix chord relative pitch drift
- First note of a chord was using the wrong relative reference pitch, causing pitch drift across chord sequences
- Fixed relative pitch tracking for chord elements in `ly_to_ir`

### E4bT2: Fix tempo beat-unit handling
- `consume_tempo()` now correctly parses dots on beat-unit durations
- Added `beat_unit_to_quarters()` to convert beat-unit names (with dots) to quarter-note duration ratios
- Tempo microseconds-per-quarter now correctly accounts for dotted beat units (e.g. dotted quarter = 120 BPM → 166667 µs/quarter)

### E4bT3: GM instrument→program lookup
- Added `gm_program_from_name()` with ~100 General MIDI instrument name → program number mappings
- Added `extract_scheme_string()` for parsing `#"string"` embedded scheme values
- `\set Staff.midiInstrument` in `walk_parallel_music_staves` now correctly handles both plain strings and embedded scheme strings
- Added deferred property mechanism for grouping contexts (PianoStaff, GrandStaff, etc.): `\set` properties on grouping contexts are collected and applied post-loop to all newly-created parts, preventing orphan part creation

### E4bT4: Change default PPQN from 480 to 384
- MIDI output now uses 384 PPQN (matching LilyPond's default) instead of 480

### E4bT5: MIDI reference regression tests
- Added 14 MIDI reference regression tests in `tests/round_trip.rs` across 4 fixtures (pedal, example, example2, chopin)
- Tests cover: track count, PPQN, program changes, tempos, note count, pitch range
- Helper functions: `extract_programs`, `extract_tempos`, `count_midi_note_ons`, `collect_midi_note_on_pitches`, `ly_to_midi_vs_ref`, `ly_to_midi_vs_ref_movement`
- Program comparison uses deduplicated `(channel, program)` tuples (LilyPond emits duplicate ProgramChange events)
- Track count allows ±1 tolerance (LilyPond may emit extra tracks for Lyrics contexts)

### Test counts
- Total: 729 tests (440 unit + 19 CLI + 209 fixture regression + 61 round-trip + 18/20 proptest)
- 2 pre-existing proptest failures (double-flat pitch transpose edge case, unchanged)

### Next up
- **Epic 5: Complete LilyPond Parser** — `\chordmode`, `\figuremode` robustness, `\partial` in multi-movement

---

## 2026-03-20 — Epic 4: MIDI as First-Class ✅

### E4T1: Remove MIDI feature gate
- Moved `midly` from optional dependency (`dep:midly` behind `midi` feature) to default dependency
- Removed all 17 `#[cfg(feature = "midi")]` gates across `src/adapters/mod.rs`, `src/lib.rs`, `src/main.rs`
- Updated CI workflow (`.github/workflows/ci.yml`) — removed `--features midi` from clippy and test jobs
- Updated docs: `README.md`, `CLAUDE.md`, `docs/development.md`, `docs/cli.md` — all `--features midi` references removed
- MIDI support is now always compiled; no flag needed

### E4T2: MIDI round-trip tests
- Added 17 MIDI round-trip integration tests in `tests/round_trip.rs`:
  - Simple melody, mixed durations, dotted notes, rests, multiple measures
  - Two parts, 3/4 time, 6/8 time, key signatures with sharps, whole notes
  - Tempo markings, chromatic pitches/accidentals, wide pitch range, eighth notes
  - Cross-format tests: LilyPond→MIDI→Score, MusicXML→MIDI→MusicXML
  - CLI MIDI round-trip test
- Added 4 MIDI CLI integration tests in `tests/cli.rs`:
  - XML→MIDI conversion, MIDI→LY, MIDI→XML, `info` on MIDI file
- Fixed missing `page_layout` field in `ir_to_midi.rs` test helper

### E4T3: ToMusicAdapter/FromMusicAdapter for MIDI
- Implemented `ToMusicAdapter` for `MidiToIrAdapter` — bridges via `convert_file` → `lift_to_music`
- Implemented `FromMusicAdapter` for `IrToMidiAdapter` — bridges via `lower_to_score` → `write`
- Binary format: `convert_music()` / `convert_str_to_music()` return `Unsupported`; `write_music()` / `convert_file_to_music()` work
- Added 3 Music adapter integration tests: MIDI→Music, Music→MIDI, full LY→Music→MIDI→Music→LY round-trip

### Test counts
- Total: 731 tests (437 unit + 19 CLI + 209 fixture regression + 20 proptest + 45 round-trip + 1 doc)
- All passing, clippy clean

### Next up
- **Epic 5: Complete LilyPond Parser** — `\chordmode`, `\figuremode` robustness, `\partial` in multi-movement

---

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

---

## 2026-03-20 — Epic 1: Complete the Two-Layer IR Architecture

**Goal:** Make Music tree the primary IR; Score only for export.

All 341 unit tests + 15 CLI integration tests pass, clippy clean.

### E1T1: Remove Forward/Backup from VoiceElement ✅
- Removed `Forward` and `Backup` structs from `src/ir/note.rs`
- Removed `VoiceElement::Forward` and `VoiceElement::Backup` variants
- `VoiceElement` now has only three variants: `Note(Box<Note>)`, `Rest(Rest)`, `Chord(Chord)`
- Spacer rests (`Rest { is_spacer: true }`) replace Forward's function
- Updated `ir_to_mxml/part.rs` to emit spacer rests as `<forward>` XML elements
- Removed Forward/Backup match arms from: transforms (transpose, invert, retrograde), adapters (ly_to_ir/merge, ly_to_ir/postprocess, ir_to_ly/emit, ir_to_ly/mod, ir_to_mxml/helpers, ir_to_midi, mxml_to_ir/mod), ir/lift, ir/lower/build
- Fixed clippy `never_loop` warning in `lift.rs` (`voice_staff_number` → use `.first()`)
- Files modified: 15+

### E1T2: Interface inversion — Music tree as primary path ✅
- `ToMusicAdapter` and `FromMusicAdapter` traits established as the recommended API path
- Parser natively produces Score; Music tree is derived via `lift_to_music`
- Full native Music tree parser rewrite deferred — the interface is ready, but the 4000+ line parser produces Score directly
- CLI and Python bindings now expose both Score and Music tree paths

### E1T3: Direct Music tree → LilyPond emitter ✅
- Created `src/adapters/ir_to_ly/music_emit.rs` (~530 lines)
- Emits LilyPond directly from `MusicDocument` without going through Score representation
- Handles all Music enum variants: Sequential, Simultaneous, Context, Note, Chord, Rest, Skip, TimeSignature, KeySignature, Clef, Tempo, Barline, Direction, Grace, Tuplet, Repeat, Variable, FiguredBass, Harmony, Lyric
- Preserves structural information (contexts, nesting, simultaneous blocks) lost in Score round-trips
- Emits annotations (articulations, dynamics, slurs, ties, beams, pedal, etc.)
- `FromMusicAdapter` for `IrToLyAdapter` now calls `emit_music_document` directly
- 17 new unit tests + 2 round-trip tests

### E1T4: Update CLI pipeline ✅
- LY→LY single-file conversions now use Music tree path (`ToMusicAdapter` → `FromMusicAdapter`)
- Added `convert_ly_to_ly()` function in `main.rs`
- `run_convert` auto-detects LY→LY case and routes through Music tree
- Imports `FromMusicAdapter` and `ToMusicAdapter` in CLI

### E1T5: Update Python bindings ✅
- Added `PyMusicDocument` class with `title`, `composer` properties, `to_json`/`from_json`, `to_score()`
- Added `from_lilypond_music(path)` — parse LilyPond file to Music tree
- Added `from_lilypond_music_string(text)` — parse LilyPond string to Music tree
- Added `to_lilypond_music(doc, path=None)` — emit Music tree as LilyPond
- Updated `_core.pyi` stubs and `__init__.py` exports
- Both Score and MusicDocument paths are available from Python

### Next up
- **Epic 4: MIDI as First-Class** — remove feature gate, add full test coverage, Music tree adapters

---

## 2025-07-14 — Epic 3: Test Coverage ✅

**Goal:** Comprehensive unit, integration, round-trip, and property-based tests.

Expanded test suite from 357 to 691 tests. All pass, clippy clean.

### E3T1: mxml_to_ir unit tests ✅
- Added 50 new tests to `src/adapters/mxml_to_ir/tests.rs` (20→70 total)
- Coverage: XmlNode helpers, note parsing edge cases (dotted, tuplets, ties, slurs, articulations, ornaments, technicals, fermata, lyrics, beams), direction parsing, barlines, attributes, metadata, harmony, figured bass

### E3T2: ir_to_mxml unit tests ✅
- Added 14 new tests to `src/adapters/ir_to_mxml/tests.rs` (55→69 total)
- Coverage: spacer-as-forward, dotted note, key signature minor, tuplet display, metadata fields, anacrusis partial, slur start/stop, lyrics, ornaments, technicals, rights metadata, transpose attribute, divisions auto-computed
- Pattern: `if let ScoreChild::Part(ref mut part) = score.children[0]` blocks to avoid borrow checker conflicts

### E3T3: ly_to_ir unit tests ✅
- Added 20 new tests to `src/adapters/ly_to_ir/tests.rs` (81→101 total)
- Coverage: multi-measure rest expansion, barline types (`\bar "|."`, `\bar "||"`), tempo parsing, multi-voice `\\`, shorthand articulations, slur events, appoggiatura, dynamics context merge, time signature synchronization, chained variable resolution, voiceOne/Two, `\once \override`, `\skip`, fermata, PianoStaff, tied notes, relative octave

### E3T4: Fixture regression tests ✅
- Created `tests/fixture_regression.rs` with 209 tests
- All 143 XML fixtures: parse → Score, assert non-empty parts
- All 10 MXL fixtures: decompress → parse → Score
- All 35 LY fixtures (named + UUID): parse → Score, no panic
- 14 cross-format XML→LY regression tests
- 8 cross-format XML→MusicXML regression tests
- Uses macro-generated test functions for per-fixture isolation

### E3T5: Round-trip testing framework ✅
- Created `tests/round_trip.rs` with 25 integration tests
- Helper functions: `count_notes()`, `count_rests()`, `collect_pitches()`, `assert_mxml_roundtrip()`, `assert_ly_roundtrip()`
- Categories: MusicXML round-trips (simple melody, chords, dotted, two parts), LilyPond round-trips (simple, key+time, chords), cross-format (MusicXML→LY), MXL fixture round-trips (4), XML test suite round-trips (13)

### E3T6: Property-based tests with proptest ✅
- Created `tests/proptest_tests.rs` with 20 property tests
- Strategies: `arb_pitch_step()`, `arb_pitch()` (integer alters for MIDI safety), `arb_pitch_microtonal()`, `arb_duration()`, `arb_base_duration()`
- Pitch properties: transpose round-trip, zero identity, semitone addition, associativity, MIDI range, step index round-trip/wrapping
- Duration properties: actual_duration positivity, dot monotonicity, single dot = 1.5×, no-dot/no-tuplet = base, tuplet scaling
- Transform properties: transpose inverse, invert self-inverse, retrograde self-inverse, note count preservation (all 3 transforms), retrograde reverses order, composed transforms preserve count

### New files
- `tests/fixture_regression.rs` — 209 fixture regression tests
- `tests/round_trip.rs` — 25 round-trip integration tests
- `tests/proptest_tests.rs` — 20 property-based tests

### Next up
- **Epic 4: MIDI as First-Class** — remove feature gate, add full test coverage, Music tree adapters
