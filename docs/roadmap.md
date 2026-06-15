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
| EBT1 | Emit **lyrics** in IR→LY (Music path `\addlyrics` + parse `\addlyrics` on import) | ✅ |
| EBT2 | **Repeat/volta** LY→IR→LY round-trip (`\repeat volta` + `\alternative`) | ✅ Music path: parser now flushes the repeat body before `\alternative` (was conflating body with alt 1); `lift.rs` reconstructs `Music::Repeat` from repeat barlines + volta endings. Single-staff; Score-path (ir_to_ly) emitter + multi-staff still TODO |
| EBT3 | `\chordmode` import → `Harmony` IR (text-based parser, language-aware; distributed to melody part; round-trips) | ✅ |
| EBT4 | `\figuremode` robustness (natural `!` + double accidentals `++`/`--`, round-trip via `figure_to_ly`) | ✅ |
| EBT5 | MIDI **velocity ↔ dynamics** mapping both directions (shared `dynamics_velocity` map) | ✅ |
| EBT6 | `\partial` in multi-movement contexts (reset per `\score` block — was leaking) | ✅ |
| EBT7 | **E2E cross-format fidelity audit** (`tests/test_e2e_conversion.py` + note-array signature): fixed ly→ly relative octave (single-staff), ly→abc empty output, midi→midi tie multiplication | 🟡 follow-ups below |

**EBT7 follow-up bugs (root-caused 2026-06-15, see changelog):**
- ⬜ ly→ly relative octave shift for **multi-staff / multi-voice** (`<<\\>>` relative flow needs per-voice prev-threading in `ir_to_ly`)
- ⬜ midi→ly **tuplet duration** loss (`ir_to_ly` ignores a bare note's tuplet ratio — emit `\tuplet`)
- ⬜ xml→ly **repeat-from-the-top** note loss (backward repeat with no forward desyncs `\repeat volta` tracking in `ir_to_ly/emit.rs`)
- ⬜ ly→midi **grace notes** steal metrical time (`ir_to_midi` grace-unaware)
- ⬜ ABC **multi-voice (`V:`)** support — would make →ABC lossless for polyphony (currently v1 single-line, format-inherent)

### Epic C: Semantic Round-Trip Test Bar (quality gate)

| Task | Description | Status |
|------|-------------|--------|
| ECT1 | Signature/comparator library (`tests/common/mod.rs`): pitches, durations, dynamics, articulations, ties/slurs, lyrics, harmonies, time sigs | ✅ |
| ECT2 | Per-fixture semantic round-trip suite (`tests/semantic_roundtrip.rs`): LY↔IR↔LY + XML↔IR↔XML, pitch-multiset & note-count invariants + dynamics | ✅ |
| ECT3 | Fidelity scoreboard (`tests/fidelity.rs`) gated on a committed baseline (non-decreasing); runs in CI via `cargo test`. Audit also gates on 0 panics | ✅ |

Scoreboard at completion: **XML→IR→XML 152/152** (note-count & pitch-multiset); **LY→IR→LY 33/35** (only example.ly/example2.ly drift, +9 notes — complex multi-voice). Building the scoreboard surfaced and fixed two real CLI LY→LY bugs: top-level `parallel_music`/`named_context` weren't parsed (re-parse yielded 0 notes), and the Music-path emitter emitted relative octave marks without a `\relative` wrapper (now emits absolute).

**Update 2026-06-12:** scoreboard now **LY→IR→LY 35/35** — the example.ly/example2.ly drift was `\addlyrics` inside `<< … >>` being parsed as music (lyric syllables became phantom notes); fixed along with the pedal.ly bar-58 PianoStaff time-signature unification and 12 further conversion bugs (see changelog).

**Update 2026-06-13:** the 5 confirmed-but-open bugs are now all fixed — Score-path `\repeat volta N` count (via `Barline.repeat_times`), two-note tremolo emission (`\repeat tremolo`), multi-staff lyrics referencing, cross-staff voice duplication, and the MIDI conductor track reading only part[0]. 797 Rust tests green. Only `\change Staff` cross-staff beaming remains as a known notation gap.

**Update 2026-06-14 (piano fidelity):** deep pass on the hardest piano fixtures.
- **pedal.ly** — the sustain pedal now renders *below* the left-hand staff in MuseScore: empty staff bars carry an invisible anchor rest, and `\sustainOn`/`Off` attach at the note onset (LY post-event semantics). See changelog.
- **chopin_n.ly** — was 121/181 bars wrong; the **whole main body (bars 1–69) is now bar-for-bar correct** and renders like the LilyPond reference. Fixed: tuplet chords scaling inner notes (+ nested-tuplet product), `q` chord-repetition, tie-stop resolution, per-voice slur numbering, `\partial` pickup preserved through the variable/time-change resplit, fingered chords inside grace blocks, and `\repeat unfold N` (was emitted once). **Update 2026-06-15 — end cadenza + Agitato now fixed (Epic H):** the whole fixture
now converts correctly. The post-Agitato RH/LH drift is gone (`disambiguate_colliding
_voice_numbers` + the `\tuplet 3/2 4 {…}` group-duration parse fix — the real cause of
the LH Agitato over-parse), and the free-time end cadenza now collapses to ONE
`senza_misura` bar holding both hands followed by the strict-time 4/4 coda, matching the
LilyPond reference (rendered & compared). See Epic H. Residual polish only: the cadenza's
internal free-time voice rhythm and a slightly over-long coda flourish bar are cosmetic,
not structural.

### Epic D: ML Representations (`src/representations/`, modeled on muspy)

| Task | Description | Status |
|------|-------------|--------|
| EDT1 | Note-array `(onset, duration, pitch, velocity)` ↔ IR | ✅ `src/representations/note_array.rs`: `NoteArray`/`NoteRow`, `to_note_array` (Music-tree walk: unfolds repeats, resolves simultaneity/grace/tuplets, dynamics→velocity), `from_note_array` (Simultaneous of `Skip·Note` branches). 11 tests incl. round-trip |
| EDT2 | Event sequence (note-on/off, time-shift, velocity-set) + documented vocabulary | ✅ `src/representations/event_sequence.rs`: `EventSequence` + `EventOptions` (Performance-RNN vocabulary — note-on 0–127, note-off 128–255, time-shift 256.., velocity-set; documented in the module + `event_name`/`vocab_size`). `to_event_sequence`/`from_event_sequence` go through `NoteArray`; FIFO note-off matching; decomposed time-shifts. 7 tests |
| EDT3 | Piano-roll dense `T×128` (configurable resolution) ↔ IR | ✅ `src/representations/piano_roll.rs`: `PianoRoll` (row-major `T×128`, velocity or binary), `to_piano_roll`/`from_piano_roll` (run-detection decode) through `NoteArray`. Adjacent same-pitch notes merge (documented). 8 tests |
| EDT4 | numpy interop via PyO3 (`numpy` crate) + `.pyi` stubs | ✅ `numpy` crate (0.22, abi3-compatible) wired into `lib.rs`: `to_/from_note_array` (`(N,4)` i32), `to_/from_event_sequence` (1-D i64), `to_/from_piano_roll` (`(T,128)` u8). `.pyi` stubs + `__init__` re-exports + `numpy` runtime dep. Verified via `maturin develop` + 12 pytest round-trips |
| EDT5 | Round-trip tests (note-array exact; event exact; piano-roll quantization-aware) | ✅ note-array + event-sequence exact (onset/duration/pitch; velocity banded); piano-roll exact for distinct-pitch / gap-separated material, lossy merge for adjacent same-pitch (tested + documented) |

### Epic E: ABC Adapter (new format)

| Task | Description | Status |
|------|-------------|--------|
| EET1 | `abc_to_ir.rs` parser (`ToMusicAdapter`) | ✅ Hand-written parser: `X/T/C/M/L/K/Q` headers, notes (explicit accidentals, octave marks, fractional durations), default-unit-length rule, key tonic+mode→fifths (incl. church modes), rests, bar lines + repeats, chords `[..]`, ties; graceful skip of chord symbols/decorations/grace/inline fields. Also `ToIrAdapter` via lower. 12 tests |
| EET2 | `ir_to_abc.rs` emitter (`FromMusicAdapter`) | ✅ Emits header + body at `L:1/8`; pitch/duration/key/meter/barline/chord/tie rendering. v1 limitation: pitches carry only explicit accidentals (no key-aware re-spelling) — self-consistent on round-trip. 5 tests |
| EET3 | CLI wiring + ABC fixtures + semantic round-trip test | ✅ `.abc` wired into `convert` (in + out, `-f abc`); 3 fixtures; `tests/abc_roundtrip.rs` (6 cases: parse→emit→parse pitch/duration identity, repeats, chords, pitch multiset, ABC→LY, ABC→XML) + 2 CLI tests |

### Epic F: Datasets & Metrics (ML pipeline)

| Task | Description | Status |
|------|-------------|--------|
| EFT1 | Dataset classes (`src/lytk/datasets/`): base `Dataset`, generic `FolderDataset`, one remote dataset (e.g. JSB Chorales); torch/tf adapters (lazy import) | ✅ `src/lytk/datasets/`: `Dataset` base (representation converters, metrics, splits, lazy `to_pytorch_dataset`/`to_tensorflow_dataset`), `FolderDataset` (lazy load of .ly/.xml/.mxl/.mid via `load_document` + new `Score.to_music_document` lift binding), `Subset`. Remote dataset (JSB Chorales downloader) deferred — needs network, not sandbox-testable |
| EFT2 | train/val/test split + on-disk caching of converted representations | ✅ `Dataset.split(ratios, seed)` (deterministic, disjoint, remainder-safe) + `FolderDataset` `.npy` cache keyed by file + representation + params |
| EFT3 | Objective metrics (`src/representations/metrics.rs` + Python): pitch-class histogram/entropy, n-PC rate, polyphony, empty-beat rate, scale & groove consistency | ✅ `src/representations/metrics.rs` (11 metrics + helpers): n_pitches/n_pitch_classes_used, pitch_range, pitch_class_histogram, pitch/pitch_class_entropy, polyphony, polyphony_rate, empty_beat_rate, pitch_in_scale_rate, scale_consistency, groove_consistency. PyO3 `compute_metrics` → dict + stub. 7 Rust + 3 pytest. Adversarially verified against muspy (6-group workflow, 0 discrepancies) |
| EFT4 | Tests: folder → dataset → batch tensor shapes; metrics on hand-built fixtures | ✅ `tests/test_datasets.py` (14 cases: load .ly/.xml/.mxl/.mid, folder discovery, representation conversion, metrics, splits, caching) + metric tests (Rust + pytest, muspy-verified) |

### Epic G: Python Distribution & Docs (release readiness) 🟡

| Task | Description | Status |
|------|-------------|--------|
| EGT1 | Complete `.pyi` stubs for `_core` incl. representations | ✅ `_core.pyi` covers both classes + every bound function incl. ABC (`from_abc`/`from_abc_string`/`to_abc`) and the numpy representation/metric functions |
| EGT2 | Python wrappers for ABC + representations | ✅ Bound ABC in PyO3 (`from_abc`, `from_abc_string`, `to_abc` — emit lifts Score→Music internally); re-exported in `__init__.py`; `.abc` added to the Python CLI (read/write/info, `-f abc`). Representations already exposed + re-exported. 5 ABC pytests |
| EGT3 | maturin GitHub Actions wheel matrix (Linux/macOS/Windows, abi3) | ✅ `.github/workflows/release.yml`: abi3 wheels (Linux x86_64+aarch64, macOS x86_64+arm64, Windows x64) + sdist via `PyO3/maturin-action`, publish-to-PyPI job (Trusted Publishing/OIDC) gated on a `v*` tag |
| EGT4 | `pyproject.toml` metadata, README quickstart, finalize `import-export.md` matrix | ✅ `pyproject.toml`: `license = "GPL-2.0-or-later"` (matches the repo LICENSE) + classifiers, keywords, URLs, `torch`/`tensorflow` extras (verified in the built wheel METADATA). README Python quickstart expanded (ABC, representations, MIDI, extras). `import-export.md`: top-level format matrix + real ABC section + `\cadenzaOn/Off` updated |
| EGT5 | Tag **v1.0.0**; update roadmap (Completed) + changelog | ⬜ Deferred — the version tag is intentionally NOT created yet (per request). Everything else for release is in place; bump `version` in `pyproject.toml`/`Cargo.toml` + push a `v*` tag to trigger the wheel build/publish when ready |

### Epic H: Multi-voice / multi-staff bar-splitting rework 🟢

**Status (2026-06-15):** the two error classes below are both resolved on `master`.
(1) RH/LH drift fixed by `disambiguate_colliding_voice_numbers` (multi-voice collapse)
+ the `\tuplet 3/2 4 {…}` group-duration parse fix (the LH-Agitato over-parse that was
the real "RH ends ~70 beats before LH" cause). (2) The free-time end cadenza now
collapses to one `senza_misura` bar holding both hands + a strict-time coda (EHT4 ✅).
The clean-room EHT1–EHT3 rewrite (a single authoritative bar-splitter) remains an
optional future refactor; it is no longer needed to fix chopin. The targeted, surgical
path below shipped instead.

**Why.** `chopin_n.ly` converts with the whole main body (bars 1–69) bar-for-bar
correct, but two classes of error remain, both rooted in *how and when measures
are split*: (1) **right-hand/left-hand drift** — a multi-voice bar puts both
branches into one voice (e.g. bar 72 voice 2 = 6/4, bar 156 voice 2 = 9/4), so
the RH runs 3 beats ahead of the LH from that point on (audible after the
Agitato); (2) **the RH ends ~70 beats before the LH** — the free-time cadenza
auto-splits into a different number of 4/4 bars per hand, so the staves fall out
of measure-index alignment. The current code patches these with index-based
merges and after-the-fact resplits that each handle only some cases.

**Root cause (verified by stage-by-stage instrumentation).**

1. **Staves are pre-parsed at the wrong meter.** `upperStaff`/`lowerStaff` are
   pre-parsed into `VarDef::Measures` ([`mod.rs:174`]) while the active time sig
   is the default 4/4 — `\time 6/8` lives in the separate `\global` variable,
   resolved only later. So `push_voice_element`'s auto-bar-split
   ([`state.rs:211`]) runs against 4/4 during pre-parse; correct bar boundaries
   survive only because explicit `|` checks happen to land right. Every downstream
   pass then has to "fix up" boundaries it should never have had to.
2. **Multi-voice merge aligns by measure index, not time.** `<< { a } \new
   Voice { b } >>` is parsed branch-by-branch and stitched by
   `merge_simultaneous_block` ([`walk.rs:888`]) → `merge_voice_measure_streams`
   ([`merge.rs:341`]), which zips streams **by index**. When the branches'
   per-branch measure *counts* differ (a multi-bar `\new Voice` body vs a
   one-bar sibling, or a 4/4-pre-parse mismatch), content collapses into one
   over-full voice instead of overlaying by position.
3. **Re-barring is a lossy fix-up, not the source of truth.**
   `resplit_measures_for_time_sig` ([`merge.rs:761`]) and
   `resplit_measures_with_time_changes` ([`merge.rs:1254`], driven by
   `unify_staff_time_signatures` [`merge.rs:1602`]) re-flow already-built
   measures. With per-staff drift, the unified timeline carries conflicting
   time-change positions and the catch-up boundary logic mis-bars multi-voice
   measures (verified: disabling unify changes *which* bars break, not *whether*).
4. **`\cadenzaOn`/`\cadenzaOff` is ignored** ([`music.rs:871`]) and is
   intrinsically **score-wide** — it must suppress barlines for *all* staves over
   the same span. A per-variable senza-misura flag desyncs single-hand cadenzas
   (it regressed `pedal.ly`).

**Architectural decision — split bars once, at score-assembly, by absolute time.**
Stop bar-splitting during per-variable pre-parse. Parse each voice into a flat,
meter-agnostic element stream carrying explicit `|` checks, `\partial`,
`\cadenzaOn/Off` span markers, and attribute events with their *time positions*.
Build measures in **one** authoritative pass after the full score timeline (every
staff's meter changes, partial, and cadenza spans) is known. This makes the four
mechanisms above disappear rather than be patched.

| Task | Description | Status |
|------|-------------|--------|
| EHT1 | **Defer bar-splitting.** Add a meter-agnostic `VarDef::Stream` (flat `Vec<VoiceEvent>` with positions + `|`/attribute/partial/cadenza markers) or make `VarDef::Measures` store the *active def-meter* and never auto-split (rely on `|`). Pre-parse stops calling the `state.rs:211` auto-flush; record `\time` as an event. | ⬜ |
| EHT2 | **Position-based voice overlay.** Replace `merge_simultaneous_block` index-zip with a merge that lays each branch's events onto a shared timeline by absolute onset, so `<< { } \\ { } >>` and `<< { } \new Voice { } >>` overlay correctly regardless of per-branch bar counts. Reuse the `walk_parallel_music_voices`/`_staves` split detection but feed the new merge. | ⬜ |
| EHT3 | **Single authoritative bar-splitter.** One function: given per-voice event streams + the unified timeline (meters, partial, cadenza spans) → measures. Subsumes `resplit_measures_for_time_sig`, `resplit_measures_with_time_changes`, the pickup/senza handling, and `synchronize_time_signatures`. Splits every voice at the same boundaries; voices in a bar align by position. | ⬜ |
| EHT4 | **Score-wide cadenza.** ✅ **DONE (2026-06-15).** The `\cadenzaOn/Off` end cadenza now collapses to ONE `senza_misura` bar holding both hands, then the strict-time coda — matching the LilyPond reference. Implemented *without* the planned span-union: per-measure `measure_has_cadenza` flag + `resolve_variable` senza-flagging keep the whole cadenza flagged; resplit/unify preserve `senza_misura`; `merge::collapse_cadenza_runs` collapses each ≥2 run to one bar (re-joining voices by number); and for a score-wide cadenza (every staff free) each staff is collapsed *before* the PianoStaff index-merge so the bass coda isn't folded in. pedal.ly (single-hand) stays aligned via the auto-split. Test: `chopin_cadenza_is_single_senza_bar_with_both_hands`. | ✅ |
| EHT5 | **Regression bar.** Golden per-staff bar-fill + RH/LH total-duration equality for `chopin_n.ly`, `pedal.ly`, `repeats.ly`; the existing main-body-bar-perfect property must not regress; full `cargo test` + render diff vs the LilyPond reference. | ⬜ |

**Sequence & guardrails.** EHT1→EHT2→EHT3 are the spine (do together, behind the
existing tests); EHT4 builds on EHT3's span model; EHT5 gates the whole thing.
This touches the hottest path in `ly_to_ir` — land it on a branch with the full
fixture suite green at every step, and treat *"bars 1–69 of chopin stay
bar-perfect"* and *"`pedal.ly` staves stay aligned"* as non-negotiable
invariants. Prototyped primitives already on master that this epic consumes:
`Measure.senza_misura`, `#(skip-of-length)`, `\repeat unfold N`, the
`\partial`-pickup preservation.

**Progress (2026-06-15, branch `epic-h-bar-splitting`).** A surgical, atomic,
non-regressive subset of the rework landed (all 872 tests green at each step):
- **Multi-voice collapse FIXED (the post-Agitato desync).** Root cause was not
  the index-zip but `resplit_*` flattening by `v.number`, folding two
  *simultaneous same-numbered* `Voice`s (e.g. `<< { } \new Voice { \voiceTwo …}>>`
  leaves both numbered 2) into one over-full voice. `disambiguate_colliding_voice
  _numbers` (merge.rs) renumbers the colliding voice before each by-number flatten
  — a no-op for already-distinct voices, so byte-identical elsewhere. Fixes
  chopin bar 72 and **re-syncs the whole post-Agitato section** (the +3-beat
  offset is gone). Tests: `chopin_bar72_multivoice_not_collapsed`,
  `chopin_no_overfull_voice_main_body` (bars 1–155), `chopin_bars_1_69_bar_perfect`.
- **`Measure.senza_misura` + `<senza-misura/>` exporter** (inert plumbing).
- **`\cadenzaOn/Off` parsed**, flagging cadenza measures senza (without
  suppressing auto-split, so single-hand cadenzas stay aligned; pedal.ly gains a
  correct `<senza-misura/>`).
- **`\tuplet` ratio/group-duration form fixed — this was the real RH/LH desync.**
  The "RH ends ~70 beats before the LH" symptom turned out **not** to be a cadenza
  re-barring problem at all. A bisection (`upperStaff` 545 q correct vs `lowerStaff`
  618 q, +85 q over expected; the gap is wholly in the LH **Agitato**, 250 q vs
  168 q ≈ **3/2×**) pinned it on the tuplet parser. `\tuplet 3/2 4 { … }` — the `4`
  is LilyPond's optional *group-duration* argument (per-group beaming span, ratio
  unchanged) — was unrecognized: the parser expected the music block immediately
  after the fraction, so the block fell through and its notes parsed at **full
  duration** (3/2× too long) with no `<time-modification>`. chopin's LH Agitato
  wraps ~40 bars in one such tuplet. Fix ([`music.rs`]): skip an optional
  `unsigned_integer` (+dots) between the fraction and the block. Result: the chopin
  **RH/LH gap collapses 76.5 q → 6 q**, output **186 → 167 measures**, the ~19
  phantom LH bars gone. No-op for the plain `\tuplet a/b { … }` form. Test:
  `test_parse_tuplet_with_group_duration_arg`. (Supersedes the earlier
  "resplit per-voice positions" / "cadenza pre-split" hypotheses — those were
  downstream symptoms of the over-long LH stream being packed by the index-merge.)
- **Cadenza collapse (EHT4) — ✅ DONE.** The end cadenza now collapses to ONE
  `senza_misura` bar holding both hands, followed by the strict-time 4/4 coda with
  both hands — matching the LilyPond reference (rendered & compared). Mechanism: keep
  the whole `\cadenzaOn…\cadenzaOff` span flagged senza through the pipeline
  (per-measure `measure_has_cadenza` for the lazy-flush tail; `resolve_variable` flags
  sub-variable splices like cadenzaA/cadenzaB; resplit/unify preserve the flag), then
  `merge::collapse_cadenza_runs` folds each ≥2 senza run into one bar, re-joining
  voices by number. For a score-wide cadenza (every staff free) each staff is collapsed
  *before* the PianoStaff index-merge, so a longer treble cadenza can't fold the bass
  coda into its span. A single-hand cadenza (pedal.ly) is a run of 1 → left to the
  auto-split, staying staff-aligned. The earlier "6 q residual" (treble 31 q vs bass
  24 q) is simply the two hands' notated free-time lengths; both now share the one
  cadenza bar and the coda aligns by bar. Test:
  `chopin_cadenza_is_single_senza_bar_with_both_hands`.

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
| 7 | D ✅ | ML representations (note-array, event, piano-roll, numpy) |
| 8 | E ✅ + F ✅ | ABC adapter + datasets & metrics (E done; F done bar the optional remote dataset) |
| **9** | **G** | **Distribution: stubs, wheels, docs → tag v1.0.0** |
| **10** | **H** | **Multi-voice/multi-staff bar-splitting rework (deferred bar-splitting; fixes chopin RH/LH drift + cadenza)** |

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
