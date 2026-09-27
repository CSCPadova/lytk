# Roadmap

Items are grouped by status. Completed items are kept for reference.

## Latest status (2026-09-27, Epic J: J4 and J5)

**LilyPond text reads as LilyPond reads it.** Strings are decoded in every
context (a string used to stop at its first escape), header values given as
`\markup` or `#"…"` become text, each `\score`'s header stays with its
movement, and `Score.header` / `lytk.header_fields` expose every field (the
latter with its span, to cut it). `\include "english.ly"` and the other
language files set the pitch language, and `\language` works inside
`\score` and music. The writers write every header field and the text at
notes. Test counts: 1,133 Rust + 228 Python. Details in the devlog.

---

## Previous status (2026-09-27, Epic J: J2 and J3)

**LilyPond reading reports what is wrong.** Readers raise `lytk.ParseError`
(a `ValueError`) on input they cannot read. `lytk.check_lilypond` and
`lytk check` report `Diagnostic`s: syntax errors from the tree alone (0.08 ms
median per file), and with `semantic=True` what the walk rejects or does not
read. `strict=True` readers raise `LilyPondSyntaxError`, and every LilyPond
`Score`/`MusicDocument` carries its diagnostics. On LilyPond 2.26's 2,626
files one has an error (a grammar gap); of 502 broken fixtures, 499 have one.
Markup is no longer read as music. Test counts: 1,125 Rust + 224 Python.
Details in the devlog.

---

## Previous status (2026-09-27, Epic J: J0 and J1)

**No panic reaches Python; LilyPond input cannot hang lytk or exhaust its
memory.** Every binding runs inside a panic firewall (`lytk.InternalError`),
the LilyPond reader has bounds (elements, length, bars, multipliers, nesting,
octaves) and walks on its own 64 MB stack, and a multi-agent hunt's findings
are fixed and pinned in `tests/fuzz_inputs.rs` together with LilyPond-shaped
fuzzing. `tests/ly_corpus.rs` measures the grammar and the reader against
LilyPond's own 2,626 regression tests and snippets (CI job `lilypond-corpus`).
Test counts: 1,114 Rust + 214 Python. Details in the devlog.

---

## Previous status (2026-09-24, third pass)

**One CLI, in Python.** The Rust binary (`src/main.rs`, clap) is gone; the
`lytk` command is a Typer app (`src/lytk/cli.py`) with all 13 subcommands of
the Rust one, stdin/stdout streaming, multi-movement output and parallel
folder/batch conversion (worker processes). Three bindings were added for it
(`from_lilypond_movements`, `to_lilypond(relative=…)`, `to_mxl_bytes`) plus
`Score.lyricist` and `Part.midi_instrument`. The 48 Rust CLI tests became
Python tests (`tests/test_cli.py`, 66 cases). `clap`/`anyhow` dropped from the
crate. Test counts: 998 Rust + 184 Python.

---

## Previous status (2026-09-24, second pass)

**Pre-release work for 0.1.0** (details in `docs/devlog.md`):

- **Epic H done (EHT1–EHT3, EHT5).** The LilyPond reader no longer builds
  measures while walking. Music is placed at absolute positions in voice lanes
  (`ly_to_ir/timeline.rs`) and bars are cut once, at score assembly, on a
  score-wide grid (meter anchored at the start/`\partial`/`\time`, explicit
  barlines as extra boundaries, one free bar per cadenza). `merge.rs` went from
  1,796 to ~250 lines: every index-based merge, resplit and sync pass is gone.
  Reviewing the before/after output of 319 inputs turned up 20+ reading bugs it
  fixes (off-by-one keys/clefs/tempos/breaks, misplaced repeats, lost or
  phantom staves and notes, lyric blocks parsed as music). EHT5 regression bar:
  `piano_fixtures_staves_stay_in_step`.
- **Multi-staff repeats (EBT2 remainder) done.** The Music-tree lift for piano
  parts reused none of the single-staff logic: it dropped `\repeat`/`\alternative`
  and played a staff's voices one after the other. It now lifts each staff like
  a single-staff part; the LilyPond writer separates voices with `\\`.
- **`français`** added, and every pitch language checked against LilyPond's
  `scm/define-note-names.scm`: German, Finnish, Swedish and Norwegian output
  used spellings LilyPond rejects (`ees`, `aes`, `eess`, `heses`); fixed, and all
  910 LilyPond spellings now read back.
- **Relicensed to MIT** (owner decision). Test fixtures keep their own terms
  (`tests/fixtures/README.md`) and are excluded from the sdist.
- README rewritten as a project presentation.

Test counts: 1046 Rust + 141 Python green; clippy clean on 1.90 and 1.98.

---

## Previous status (2026-09-24)

**0.1.0 preview release prep** (details in `docs/devlog.md`). The first public
release ships as **0.1.0**, before the P-epics below; `v1.0.0` stays gated on them.

- **CI was red on master since 2026-06-18** (5 pushes), from three independent
  causes: a new clippy 1.98 `question_mark` lint; two Windows-only CLI tests that
  built batch-job JSON with `format!` + `Path::display()` (backslashes became
  invalid JSON escapes); and `scipy.linalg.sqrtm(disp=…)` removed by SciPy 1.18,
  which broke `frechet_music_distance` **for users**, not just in tests. All fixed.
- **Release workflow had never run**, and its x86_64 macOS job targeted the retired
  `macos-13` image → `macos-15-intel`.
- Version `1.0.0` → `0.1.0`, classifier `Production/Stable` → `Beta`; README carries
  a preview note and the full not-yet-implemented list.
- **Remaining to tag** (manual): create the `pypi` environment + PyPI Trusted
  Publisher, dry-run `release.yml` via `workflow_dispatch`, make the repo public,
  push `v0.1.0`.
- **After 0.1.0**: P5 first (the only API-breaking epic, cheapest while on 0.x),
  then P3 → P4 → P6 → P8 → P9.

Test counts: 1025 Rust + 141 Python green; clippy clean on 1.90 and 1.98.

---

## Previous status (2026-09-20)

**Conversion/augmentation audit + DLPack** (details in `docs/devlog.md`).
Exercised all 36 format pairs and every transform end to end.

- **Three silent conversion bugs fixed**: ABC dropped tuplets on read *and*
  write (wrong durations, no error); ABC emitted almost no bar lines (a
  28-measure score came out as one ABC measure); Humdrum overwrote every grace
  note because its per-measure map was keyed by onset alone. ABC grace notes
  (`{…}`) implemented on both sides.
- **The fidelity gate was vacuous for ABC and absent for Humdrum.** Added
  `XML → ABC → XML` (124/152) and `XML → KRN → XML` (131/152) boards over the
  full MusicXML corpus, committed as non-decreasing baselines, plus
  `tests/humdrum_roundtrip.rs`.
- **DLPack**: already works — the representations are NumPy arrays and NumPy
  implements the protocol, so torch/JAX/CuPy consume them zero-copy. Bumped the
  `numpy` floor to 1.23 (the `np.from_dlpack` floor), added tests, documented it.
- **Verified correct, unchanged**: all 36 conversions run; the transforms are
  semantically right (exact pitch shift, true mirror, exact reversal, duration
  multiset preserved); all three ML representations are exact inverses.
- **Open**: ~~`français` is missing from the 11 pitch languages~~ (✅ 2026-09-24); remaining
  cross-format drift is un-notatable durations and inner polyphony (one stream
  per staff in both the ABC and kern writers).

Test counts: 1025 Rust + 141 Python green; clippy clean.

---

## Previous status (2026-09-20)

**PyPI release preparation + DL data loaders** (details in `docs/devlog.md`).
lytk is distributed as a **Python package only** — the Rust crate is the
implementation, not a published artifact, so the layout keeps the standard maturin
naming (`[lib] name = "_core"` → the `lytk._core` module).

- **Data loaders for torch and TensorFlow.** `to_pytorch_dataset()` was as far as
  the pipeline went: representations are ragged along their first axis, so
  `DataLoader(ds, batch_size=8)` — the obvious next line — raised *"stack expects
  each tensor to be equal size"*. Added `to_pytorch_dataloader()`,
  `to_tensorflow_dataloader()` and an exported `pad_collate`, returning
  `(padded, lengths)`; lengths are explicit because `0` is a valid event
  code/pitch/velocity, so padding is not self-identifying. Both loaders verified
  to produce byte-identical batches.
- **`FolderDataset` was blind to ABC and `**kern`.** The CLI read both; the
  dataset loader's extension set did not, so those corpora — the music21 kern
  corpus included — came back as `len(ds) == 0` with no error. Fixed, with a test
  asserting the CLI and dataset format lists agree.
- **A TensorFlow-only dtype bug** in the new padding path (`0.0` pad value vs
  integer representations) existed because CI installed torch but never
  TensorFlow, so every tf test silently skipped. CI now installs `tensorflow-cpu`.
- **Release hygiene**: the vendored tree-sitter grammar was shipping in the wheel
  with no MIT notice (now included, as the licence requires); the crate-level
  `allow(clippy::useless_conversion)` pyo3 needs had been hiding 4 real lint hits
  in `ly_to_ir` (fixed, allow now scoped to `mod python`); the release workflow's
  test gate would have failed on a missing `pip` upgrade; added
  `rust-version = 1.85` + MSRV CI, `CONTRIBUTING.md`, `SECURITY.md`, extras with
  version floors plus `lytk[all]`, and corrected a stale README.

Test counts: 1014 Rust + 150 Python green (1 skipped: FMD needs a LilyBERT
checkpoint); clippy clean. **Not tagged** — see Epic G EGT5.

---

## Previous status (2026-06-18)

Four landings + a reconciliation (details in `docs/devlog.md`):
- **MIDI instrument preservation** — instrument identity (GM name ↔ 0-indexed
  program) now survives ly ↔ musicxml ↔ midi via a shared GM table
  (`src/adapters/gm.rs`). Fixes ly→musicxml dropping the program and the
  midi→musicxml off-by-one. (ABC has no standard instrument field — gap noted.)
- **ABC multi-voice (`V:`)** — parser + emitter per ABC 2.1 §4.1; →ABC is now
  lossless for polyphony. (Closes the last EBT7 follow-up.)
- **Structured note navigation** — typed read-only `Part/Measure/Voice/Note/
  Rest/Chord/Pitch` Python objects via `score.iter_parts()` (`src/navigation.rs`).
  Was a known gap (only flat `notes()` existed); now done.
- **MIDI round-trip carried-meter fix** — non-4/4 pieces no longer drift on
  export; fidelity **2/2/2 → 3/3/2**. `example2_1` now stable on note-count +
  pitch. `pedal` (budget cascade) and `chopin_n` (**inherent** — source MIDI's
  meter changes are not bar-aligned) remain gated, documented limitations.
- **EBT7 reconciliation** — four "open" bugs were already fixed on the hardening
  branch (Phase 3a–3d); marked done below.

Test counts: 936 Rust + 136 Python green; clippy (lib+bin) clean.

---

## Completed ✅

### IR Layer
Complete `Score → Part → Measure → Voice → Note/Rest/Chord` tree with `Pitch`, `Duration`,
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
| EBT2 | **Repeat/volta** LY→IR→LY round-trip (`\repeat volta` + `\alternative`) | ✅ Music path: parser now flushes the repeat body before `\alternative` (was conflating body with alt 1); `lift.rs` reconstructs `Music::Repeat` from repeat barlines + volta endings. Score path done in R1; **multi-staff done 2026-09-24** (piano parts lift each staff like a single-staff part) |
| EBT3 | `\chordmode` import → `Harmony` IR (text-based parser, language-aware; distributed to melody part; round-trips) | ✅ |
| EBT4 | `\figuremode` robustness (natural `!` + double accidentals `++`/`--`, round-trip via `figure_to_ly`) | ✅ |
| EBT5 | MIDI **velocity ↔ dynamics** mapping both directions (shared `dynamics_velocity` map) | ✅ |
| EBT6 | `\partial` in multi-movement contexts (reset per `\score` block — was leaking) | ✅ |
| EBT7 | **E2E cross-format fidelity audit** (`tests/test_e2e_conversion.py` + note-array signature): fixed ly→ly relative octave (single-staff), ly→abc empty output, midi→midi tie multiplication | 🟡 follow-ups below |

**EBT7 follow-up bugs — ALL RESOLVED (2026-06-18 reconciliation; the first four
landed on the hardening branch, the fifth is the ABC multi-voice feature):**
- ✅ ly→ly relative octave shift for **multi-staff / multi-voice** — **Phase 3a**: the Score-path emitter emits absolute octaves whenever relative threading is unreliable (`ir_to_ly/mod.rs` `relative_is_reliable`)
- ✅ midi→ly **tuplet duration** loss — **Phase 3c**: `ir_to_ly/emit.rs` wraps duration-ratio tuplets (no `TupletDisplay`) in `\tuplet a/b { … }`
- ✅ xml→ly **repeat-from-the-top** note loss — **Phase 3b**: `ir_to_ly/emit.rs` repeat-brace depth tracking (backward-only / forward-only repeats balanced)
- ✅ ly→midi **grace notes** steal metrical time — **Phase 3d**: `ir_to_midi` emits a short grace at the current tick without advancing the voice clock
- ✅ ABC **multi-voice (`V:`)** — **2026-06-18**: `V:` voices parsed/emitted per ABC 2.1 §4.1; →ABC is now lossless for polyphony (each voice a Staff/Part). See the devlog.

### Epic C: Semantic Round-Trip Test Bar (quality gate)

| Task | Description | Status |
|------|-------------|--------|
| ECT1 | Signature/comparator library (`tests/common/mod.rs`): pitches, durations, dynamics, articulations, ties/slurs, lyrics, harmonies, time sigs | ✅ |
| ECT2 | Per-fixture semantic round-trip suite (`tests/semantic_roundtrip.rs`): LY↔IR↔LY + XML↔IR↔XML, pitch-multiset & note-count invariants + dynamics | ✅ |
| ECT3 | Fidelity scoreboard (`tests/fidelity.rs`) gated on a committed baseline (non-decreasing); runs in CI via `cargo test`. Audit also gates on 0 panics | ✅ |

Scoreboard at completion: **XML→IR→XML 152/152** (note-count & pitch-multiset); **LY→IR→LY 33/35** (only example.ly/example2.ly drift, +9 notes — complex multi-voice). Building the scoreboard surfaced and fixed two real CLI LY→LY bugs: top-level `parallel_music`/`named_context` weren't parsed (re-parse yielded 0 notes), and the Music-path emitter emitted relative octave marks without a `\relative` wrapper (now emits absolute).

**Update 2026-06-12:** scoreboard now **LY→IR→LY 35/35** — the example.ly/example2.ly drift was `\addlyrics` inside `<< … >>` being parsed as music (lyric syllables became phantom notes); fixed along with the pedal.ly bar-58 PianoStaff time-signature unification and 12 further conversion bugs (see the devlog).

**Update 2026-06-13:** the 5 confirmed-but-open bugs are now all fixed — Score-path `\repeat volta N` count (via `Barline.repeat_times`), two-note tremolo emission (`\repeat tremolo`), multi-staff lyrics referencing, cross-staff voice duplication, and the MIDI conductor track reading only part[0]. 797 Rust tests green. Only `\change Staff` cross-staff beaming remains as a known notation gap.

**Update 2026-06-14 (piano fidelity):** deep pass on the hardest piano fixtures.
- **pedal.ly** — the sustain pedal now renders *below* the left-hand staff in MuseScore: empty staff bars carry an invisible anchor rest, and `\sustainOn`/`Off` attach at the note onset (LY post-event semantics). See the devlog.
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
| EET2 | `ir_to_abc.rs` emitter (`FromMusicAdapter`) | ✅ Emits header + body at `L:1/8`; pitch/duration/key/meter/barline/chord/tie rendering. **Multi-voice (`V:`) added 2026-06-18** — ≥2 top-level voices/staves emit `V:n name="…"` blocks (ABC 2.1 §4.1), so multi-part XML/MIDI → ABC is lossless for polyphony. v1 limitation remains: pitches carry only explicit accidentals (no key-aware re-spelling) — self-consistent on round-trip. 7 tests |
| EET3 | CLI wiring + ABC fixtures + semantic round-trip test | ✅ `.abc` wired into `convert` (in + out, `-f abc`); 3 fixtures; `tests/abc_roundtrip.rs` (6 cases: parse→emit→parse pitch/duration identity, repeats, chords, pitch multiset, ABC→LY, ABC→XML) + 2 CLI tests |

### Epic F: Datasets & Metrics (ML pipeline)

| Task | Description | Status |
|------|-------------|--------|
| EFT1 | Dataset classes (`src/lytk/datasets/`): base `Dataset`, generic `FolderDataset`, one remote dataset (e.g. JSB Chorales); torch/tf adapters (lazy import) | ✅ `src/lytk/datasets/`: `Dataset` base (representation converters, metrics, splits, lazy `to_pytorch_dataset`/`to_tensorflow_dataset`), `FolderDataset` (lazy load of .ly/.xml/.mxl/.mid via `load_document` + new `Score.to_music_document` lift binding), `Subset`. Remote dataset (JSB Chorales downloader) deferred — needs network, not sandbox-testable |
| EFT2 | train/val/test split + on-disk caching of converted representations | ✅ `Dataset.split(ratios, seed)` (deterministic, disjoint, remainder-safe) + `FolderDataset` `.npy` cache keyed by file + representation + params |
| EFT3 | Objective metrics (`src/representations/metrics.rs` + Python): pitch-class histogram/entropy, n-PC rate, polyphony, empty-beat rate, scale & groove consistency | ✅ `src/representations/metrics.rs` (11 metrics + helpers): n_pitches/n_pitch_classes_used, pitch_range, pitch_class_histogram, pitch/pitch_class_entropy, polyphony, polyphony_rate, empty_beat_rate, pitch_in_scale_rate, scale_consistency, groove_consistency. PyO3 `compute_metrics` → dict + stub. 7 Rust + 3 pytest. Adversarially verified against muspy (6-group workflow, 0 discrepancies) |
| EFT4 | Tests: folder → dataset → batch tensor shapes; metrics on hand-built fixtures | ✅ `tests/test_datasets.py` (14 cases: load .ly/.xml/.mxl/.mid, folder discovery, representation conversion, metrics, splits, caching) + metric tests (Rust + pytest, muspy-verified) |

### Epic G: Python Distribution & Docs (release readiness) 🟡 *(all but the tag)*

| Task | Description | Status |
|------|-------------|--------|
| EGT1 | Complete `.pyi` stubs for `_core` incl. representations | ✅ `_core.pyi` covers both classes + every bound function incl. ABC (`from_abc`/`from_abc_string`/`to_abc`) and the numpy representation/metric functions |
| EGT2 | Python wrappers for ABC + representations | ✅ Bound ABC in PyO3 (`from_abc`, `from_abc_string`, `to_abc` — emit lifts Score→Music internally); re-exported in `__init__.py`; `.abc` added to the Python CLI (read/write/info, `-f abc`). Representations already exposed + re-exported. 5 ABC pytests |
| EGT3 | maturin GitHub Actions wheel matrix (Linux/macOS/Windows, abi3) | ✅ `.github/workflows/release.yml`: abi3 wheels (Linux x86_64+aarch64, macOS x86_64+arm64, Windows x64) + sdist via `PyO3/maturin-action`, publish-to-PyPI job (Trusted Publishing/OIDC) gated on a `v*` tag |
| EGT4 | `pyproject.toml` metadata, README quickstart, finalize `import-export.md` matrix | ✅ `pyproject.toml`: `license = "GPL-2.0-or-later"` (matches the repo LICENSE) + classifiers, keywords, URLs, `torch`/`tensorflow` extras (verified in the built wheel METADATA). README Python quickstart expanded (ABC, representations, MIDI, extras). `import-export.md`: top-level format matrix + real ABC section + `\cadenzaOn/Off` updated |
| EGT5 | Tag a release; update roadmap (Completed) + changelog | ✅ `v0.1.0` tagged 2026-09-25, the first public release (preview). `v1.0.0` waits for the P-epics |
| EGT6 | Standard maturin layout retained | ✅ Distribution is PyPI-only, so the crate keeps the conventional maturin naming (`[lib] name = "_core"`, `module-name = "lytk._core"`) and pyo3/numpy stay unconditional — a feature toggling them would be a switch with one position. The bindings did move out of `src/lib.rs` (1002 → 52 lines) into `src/python.rs`, which scoped the pyo3 `useless_conversion` allow and exposed 4 real lint hits in `ly_to_ir` (fixed) |
| EGT7 | Third-party licence compliance | ✅ The vendored tree-sitter-lilypond parser in `src/tree-sitter/` shipped inside the wheel with no licence text — MIT requires the notice to travel with it. Verbatim upstream notice added at `src/tree-sitter/LICENSE` (© Nathan Whetsell) + provenance `README.md`, and added to `license-files` (confirmed present in the built wheel and sdist) |
| EGT8 | Public-repo hygiene | ✅ `CONTRIBUTING.md`, `SECURITY.md` (parser threat model), README badges + extras matrix + data-loader section; removed the reference table pointing at a dozen directories absent from the repo, the stale "Humdrum not implemented" claim, stale test counts, and `ruff` commands for tooling configured nowhere. `rust-version = "1.85"` + MSRV CI job (the sdist compiles on the user's toolchain). Fixed a latent `release.yml` failure: the test gate lacked the `pip` upgrade needed for PEP 735 groups |
| EGT9 | Deep-learning data loaders + install API | ✅ `to_pytorch_dataloader` / `to_tensorflow_dataloader` / `pad_collate` return `(padded, lengths)` for ragged scores — the missing step between `to_*_dataset()` and training. `FolderDataset` now discovers `.abc`/`.krn`/`.kern` (silently invisible before). Extras carry version floors plus `lytk[all]`; both frameworks stay lazily imported. CI installs `tensorflow-cpu` so the tf path is actually exercised. Tests: `tests/test_datasets.py` (33) |

### Epic H: Multi-voice / multi-staff bar-splitting rework ✅

**Done 2026-09-24:** EHT1–EHT3 and EHT5 landed as the positioned reader
(`ly_to_ir/timeline.rs`) — see *Latest status*. The history below is kept for
reference.

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
| EHT1 | **Defer bar-splitting.** Add a meter-agnostic `VarDef::Stream` (flat `Vec<VoiceEvent>` with positions + `|`/attribute/partial/cadenza markers) or make `VarDef::Measures` store the *active def-meter* and never auto-split (rely on `|`). Pre-parse stops calling the `state.rs:211` auto-flush; record `\time` as an event. | ✅ `VarDef::Music { tl, len, … }`: variables are positioned timelines spliced where used |
| EHT2 | **Position-based voice overlay.** Replace `merge_simultaneous_block` index-zip with a merge that lays each branch's events onto a shared timeline by absolute onset, so `<< { } \\ { } >>` and `<< { } \new Voice { } >>` overlay correctly regardless of per-branch bar counts. Reuse the `walk_parallel_music_voices`/`_staves` split detection but feed the new merge. | ✅ every branch starts at the block's start; overlapping runs move to a free lane (`Timeline::place_run`) |
| EHT3 | **Single authoritative bar-splitter.** One function: given per-voice event streams + the unified timeline (meters, partial, cadenza spans) → measures. Subsumes `resplit_measures_for_time_sig`, `resplit_measures_with_time_changes`, the pickup/senza handling, and `synchronize_time_signatures`. Splits every voice at the same boundaries; voices in a bar align by position. | ✅ `timeline::Grid` + `timeline::split`; all the listed passes deleted |
| EHT4 | **Score-wide cadenza.** ✅ **DONE (2026-06-15).** The `\cadenzaOn/Off` end cadenza now collapses to ONE `senza_misura` bar holding both hands, then the strict-time coda — matching the LilyPond reference. Implemented *without* the planned span-union: per-measure `measure_has_cadenza` flag + `resolve_variable` senza-flagging keep the whole cadenza flagged; resplit/unify preserve `senza_misura`; `merge::collapse_cadenza_runs` collapses each ≥2 run to one bar (re-joining voices by number); and for a score-wide cadenza (every staff free) each staff is collapsed *before* the PianoStaff index-merge so the bass coda isn't folded in. pedal.ly (single-hand) stays aligned via the auto-split. Test: `chopin_cadenza_is_single_senza_bar_with_both_hands`. | ✅ |
| EHT5 | **Regression bar.** Golden per-staff bar-fill + RH/LH total-duration equality for `chopin_n.ly`, `pedal.ly`, `repeats.ly`; the existing main-body-bar-perfect property must not regress; full `cargo test` + render diff vs the LilyPond reference. | ✅ `piano_fixtures_staves_stay_in_step`; chopin bars 1–69 still bar-perfect. (Render diff not run: no LilyPond in CI.) |

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

## Epic I — MIDI and ABC conversion repair (2026-09-25 – 2026-09-27, ✅ released in v0.2.0)

The ABC and MIDI round trips passed while real conversions were wrong: every
check compared lytk's reader with lytk's own writer, so shared mistakes
cancelled out. The ABC reader ignored the key signature; the ABC writer
never wrote naturals; the MIDI reader drifted, dropped meters and keys, and
filled legato input with 64th rests; the MIDI writer padded pickups with
silence and ignored MusicXML dynamics. Epic I measures against independent
references first, then fixes each direction. References (clean-room, GPL
sources read, never copied): ABC 2.1 for ABC; MuseScore's import pipeline for
MIDI import; LilyPond's performers for MIDI export, with the `lilypond`
binary as the oracle.

| Task | Description | Status |
|------|-------------|--------|
| I0 | Measurement: ABC 2.1 oracle + writer/reader boards (`tests/abc_standard.rs`), MIDI source-truth / LilyPond-parity / synthetic / corpus / quality boards (`tests/midi_truth.rs`), bar structure on every fidelity board, `lilypond` oracle test + CI job, MIDI/ABC benchmarks | ✅ |
| IA1 | ABC pitches: key signatures (modes, `exp`, explicit accidentals) and bar-scoped accidentals on read; unambiguous accidentals and every key name on write; `w:` lines, several tunes, `M:` bounds | ✅ 2026-09-25 (+ 20 fixes from a 67-agent adversarial review: chord/tuplet ties, tied accidentals, `-8` clefs, blank lines, …) |
| IB | MIDI writer: bar times from actual bar lengths (pickups, cadenzas), exact tuplet ticks, unrolled repeats, LilyPond's dynamics table + hairpins + articulations + grace timing, channel fixes, CC64 pedal, lyrics, `<transpose>`; per-note velocity in the IR | ✅ 2026-09-25: matches LilyPond's MIDI on 7,998/7,998 onsets+pitches and 7,995 note-offs |
| IL | One bar-splitter: the lowering (Layer 1 → Layer 2) moves onto the positioned `Timeline`/`Grid`; metric duration splitter; `Music::Partial` for irregular bars; volta repeats kept instead of unfolded | ✅ 2026-09-25: notes tied over bar lines instead of copied, ABC repeats/endings/pickups kept; XML→ABC bars 126 → 135, XML→KRN 119 → 133. Beams/stems move to `src/ir/` and `Grid` with an explicit end: moved to IC |
| IA2 | ABC durations: broken rhythm, `Z`/`X` bar rests, `x` skips, inline `[V:]`, per-voice `L:`/`M:`, `&` overlays, compound-meter tuplets, additive meters | ✅ 2026-09-25 (+ irregular bars kept, from IA3; kern repeat signs) |
| IA4 | ABC content: decorations, chord symbols, annotations, slurs, tempo, lyrics, both ways; several tunes per file (`from_abc_tunes`) | ✅ 2026-09-25 |
| IA3 | ABC structure: on write, overflow ties, `x` spacers and inner voices (as `&` layers); lift keeps irregular bars | ✅ 2026-09-25 (irregular bars on read in IA2; repeats, endings and pickups in IL). ABC writer board 148/158, 212 notes misread (from 6,702 at IA1) |
| IC | MIDI reader for notation-exported MIDI: grid re-anchored at meter changes, quantization on absolute positions with tuplet detection, voice separation, piano staves, drums, pedal, lyrics | ✅ 2026-09-25: LilyPond sources 7,938/8,107 onsets (from 6,135), 7,387 with duration (from 5,535), all 18 time and 7 key signatures; synthetic cases 16/16; `from_midi` 10.9 ms (1.3× the old reader) |
| ID | MIDI reader for performed MIDI: this is PE-Epic P9 (T9.1–T9.9) | ✅ 2026-09-27: Viterbi onset quantizer, beat tracking (drift, rubato, late start), hand-split cost model, swing, played staccato; follow-ups: tempo induction far from the file's tempo, uneven chord releases, low-PPQ and mixed exact/played files |

Baselines measured at I0 (2026-09-25):
- ABC writer vs the standard: 121/158 sources with right pitches, 119/158
  with right notes under both accidental rules; 7,285 of 20,809 source notes
  misread. ABC reader: 2/4 fixtures read as the standard reads them.
- MIDI reader vs LilyPond sources: 6,116/8,120 notes by onset+pitch,
  5,511/8,120 with duration; 15/18 time and 5/7 key signatures in place; 0/5
  bar structures right. Synthetic cases: 7/15 read as written.
- MIDI writer vs LilyPond's MIDI: 4,717/7,998 notes on onset+pitch, 49 with
  velocity. MusicXML corpus → MIDI → IR: 121/142 notes, 88/142 bars.
- `lilypond` compiling lytk's LilyPond (2.22.1 locally): 139/148 fixtures
  compile; its MIDI matches lytk's on 5,939/9,401 onsets. Not compiling:
  additive meters (`\time 3+2/8`), two invalid durations (chopin_n, 75a) and
  three grace-note fixtures — LilyPond-writer bugs found by the oracle.
- Benchmarks (release, chopin_n): `from_midi` 8.5 ms, `to_midi` 2.4 ms,
  `to_abc` 1.5 ms, `from_abc` 0.35 ms.

After IA1 + IB: ABC writer 139/158 right pitches, 138/158 right notes,
6,702 notes misread (repeats and inner voices still dropped: IA3); ABC reader
3/4 fixtures. LilyPond parity 7,998/7,998 onsets, 7,995 note-offs, 4,319
velocities (the rest: LilyPond ignores dynamics in another voice or a
`Dynamics` context, and example2 removes its `Dynamic_performer`; lytk plays
them, as MuseScore and a player would). MusicXML corpus → MIDI → IR 113/142
(planned drop from 121: grace, staccato, transposition now played; the reader
learns to undo them in IC). `lilypond` oracle 6,371/9,401 onsets.

Follow-ups the oracles found outside IA1/IB (LilyPond writer and reader):
- `ir_to_ly`: `\time 3+2/8` is not LilyPond 2.22 syntax (11c–11f); invalid
  durations in chopin_n and 75a; three grace-note fixtures (24a, 24e, 61f)
  make LilyPond write no MIDI; chord names are performed as notes (71a–g);
  `\transposition` not written for transposing instruments (72a–d); pedal
  and example2 `.ly` output plays differently from the source.
- `ly_to_ir`: `\set tieWaitForNote = ##t` ties (3 notes in chopin_n);
  `\relative` applied to a variable reached through a non-block argument.
- ~~`lift`: a repeat with no forward repeat barline (45b) is not rebuilt~~ ✅
  (phase L review).
- ABC `[1,2` / `[1-3` endings keep only their first number: `Barline` needs a
  list of ending numbers (MusicXML `number="1, 2"`).
- `**kern` writes no endings (`*>` expansion lists).
- ~~`lift` produces no `Music::Partial` for a mid-piece irregular bar~~ ✅
  (IA3); the lowering drops part-group names.
- The ABC reader's own Music tree keeps repeats as bar lines (a score and
  its lift rebuild `Music::Repeat`), so `convert_str_to_music` note arrays
  don't unfold them.
- The LilyPond reader loses the notes of a grace group written with nested
  braces (`\grace { { a16 b } }`); the writers no longer produce it.
- ABC has no repeat count: repeats of three or more passes are written out.
- MIDI reader (after IC): an accent (+20) in a LilyPond-played file changes
  the velocity band for one note and adds a dynamic mark; chopin_n's
  cadenza (irregular tuplets in free time) doesn't read back exactly; a
  staccato is only recognised when played 4 louder.

---

## Release plan 0.3.0 → 0.5.0 — LilyPond input you can trust (planned ⬜)

Planned 2026-09-27. Three releases that make lytk a dependable reader of
LilyPond written by people and by models. They come out of
[lilycorpus](https://github.com/matteospanio/lilycorpus), the LilyPond
training corpus built on lytk (its `docs/lytk-wishlist.md` is the request
list), and out of LLM evaluation, where the question "is this LilyPond
well-formed, and where is it wrong?" needs an answer in milliseconds instead
of a `lilypond` run.

Today the reader never fails and never reports:
- `""`, `"}}} INVALID {{{"` and `"{ c4 d e f"` all return a score;
- every walker drops the nodes it does not recognise without a trace;
- strings are cut at their first escape;
- `\include "english.ly"` loses notes;
- six input families panic.

**Measured 2026-09-27** (lytk's grammar compiled from `src/tree-sitter/src/parser.c` and loaded with py-tree-sitter 0.25; LilyPond sources at `v2.27.3`, the reference checkout in `lilypond/`):

| Corpus (valid LilyPond) | Files | With ERROR/MISSING nodes | Cause |
|---|---|---|---|
| `input/regression/*.ly` | 2,155 | 1 | a byte-order mark mid-file (`bom-mark.ly`) |
| `input/regression/*/*.ly` | 162 | 1 | `other/display-lily-tests.ly` (`##[ #]`) |
| `Documentation/snippets/*.ly` | 389 | 0 | |
| `tests/fixtures/ly/*.ly` | 35 | 0 | |

- Parsing all 2,155 top-level regression files takes 0.34 s, 0.16 ms a file.
- **Caught by the tree:** unclosed braces, strings, chords (`MISSING >`), `<< >>` (`MISSING >>`), Scheme parentheses and block comments.
- **Not caught:** unknown commands, `c3`, `\time 0/0`, stray tokens and plain words. The grammar is nearly token-level (`c4` is `symbol` + `unsigned_integer`), so these can only be found by the walk, where they are dropped today.

So strict mode can rest on the tree for syntax, and the walk must report
what it drops. Principles, as in Epic I:
- measure first, gate on committed baselines that may only improve;
- keep the Python API backward compatible within the series: new keywords
  default to today's behaviour, and new exceptions subclass the ones raised
  today.

### Epic J — 0.3.0: trustworthy LilyPond reading

| Task | Description | Acceptance | Size | Status |
|------|-------------|------------|------|--------|
| J0 | **Measure.** Add `tests/ly_corpus.rs`, marked `#[ignore]`, run by a new CI job that sparse-clones LilyPond at a pinned tag (`input/regression`, `Documentation/snippets`), like `lilypond-oracle`. It holds four boards: (a) error diagnostics on valid files, (b) detection of structural mutations of the fixtures (a brace, quote, `>`/`>>` or `)` deleted; truncation), (c) panics under LilyPond-shaped fuzz, (d) dropped-input counts per diagnostic code on valid files, to calibrate J3's warnings | Boards committed with the numbers above as baselines | M | ✅ 2026-09-27: boards (a), (b) and a reader board (valid files refused: 0 of 2,626); (c) is J1's fuzz; (d), the warnings per code, came with J3 |
| J1 | **No panic reaches Python.** (1) Fix the six known families at their source, each becoming a diagnostic: `\time N/0` (`consume.rs` `parse_fraction`); `\time 3/256` (`den as u8`, `music.rs:404`); `\tuplet 0/N` and `\times N/0` (`music.rs:503-509`); `ly:make-moment N 0` (`consume.rs:794-815`); figured-bass dot overflow (`figured_bass.rs:191`). (2) Make the shared sinks total: `TimeSignature::beats_fraction` (`measure.rs:122`) and `Duration::actual_duration` (`duration.rs:85`); reject `beat_type == 0` and `tuplet_actual == 0` in `Score.from_dict`/`from_json`. (3) Bound expansions: `s*N` and `\skip` loops, figured-bass `*N`, nested `\repeat unfold`, with a total-events limit like MIDI's 500,000 bars. (4) Firewall: one `guard()` helper around every `#[pyfunction]` that reads, writes or transforms, turning a panic into `lytk.InternalError` with its message; a panic hook installed at module init keeps guarded panics off stderr (pattern: `mxml_to_ir::catch_read`, `tests/common` `safe()`). (5) Fuzz: a LilyPond-shaped proptest strategy in `tests/fuzz_inputs.rs` (edge numbers 0/1/255/256/`u32::MAX` in `\time`, `\tuplet`, `\times`, `*N`, dots, nesting) over `convert_str`, `_multi`, `_to_music`, `with_language` | Six families return diagnostics; fuzz board 0 panics; no `PanicException` from the bindings (Python tests on the six inputs) | M | ✅ 2026-09-27 (the six families are dropped constructs until J3 reports them); follow-ups in the devlog |
| J2 | **Exception hierarchy.** `lytk.LytkError(Exception)`; `ParseError(LytkError, ValueError)`; `LilyPondSyntaxError(ParseError)` carrying `.diagnostics`; `InternalError(LytkError)` for caught panics; I/O stays `OSError`. A non-UTF-8 `.ly` becomes a `ParseError` (today `OSError`: changelog *Changed*). Stubs list what each function raises | `except ValueError` code keeps working; `tests/test_bindings.py` covers each class | S | ✅ 2026-09-27: every reader, `from_json`/`from_dict` and `flatten`; `ParseError`/`LilyPondSyntaxError` are made with `type()` at module init (two bases); `Diagnostic` pickles, so the error crosses process pools |
| J3 | **Diagnostics and strict mode.** New `src/diagnostics.rs`: `Diagnostic { severity, code, message, line, column, start, end }`, collected in `WalkState`. *Syntax:* ERROR and MISSING nodes, at the empty `has_error()` block in `ly_to_ir/mod.rs:203-205`, as `syntax-error` / `missing-token` ("missing `>>`"). *Semantic errors:* `invalid-duration` (`c3`), `invalid-ratio` (from J1), bare words at top level (`not-lilypond`). *Warnings, where the walk drops input today:* `unknown-command`, `unrecognized-token`, `ignored-include`, `unknown-language`, `dropped-music` (top-level music beside a `\score`, movements a single-score reader drops), `skipped-score` (`\midi` without `\layout`), `expansion-limit`. Ignored Scheme is normal LilyPond: no warning. The drop sites are the catch-all arms in `walk.rs` (135-137, 350, 515, 677, 836, 850, 1132, 1209, 1336, 1339) and `music.rs` (139-145, 875-888, 910). A UTF-8 BOM anywhere is whitespace, as in LilyPond. *Python:* `lytk.check_lilypond(text, *, semantic=False) -> list[Diagnostic]` (syntax only by default, no IR); `strict=False` on every `from_lilypond*` reader (`True` raises `LilyPondSyntaxError` on any error); `Score.diagnostics` and `MusicDocument.diagnostics` (kept out of `to_dict`/`to_json`); `Diagnostic.__str__` → `3:12: error: missing '}' [missing-token]`. *CLI:* `lytk check FILE… [--json] [--semantic]`, exit 1 on errors | Board (a) ≤ 1 error on the valid corpus after the BOM fix; board (b) ≥ 99 % of structural mutations detected; the semantic cases above detected; `check_lilypond` median ≤ 1 ms on the regression files; board (d) published in the devlog | L | ✅ 2026-09-27: (a) 1 file (`##[ #]`), in the reading too; (b) 499/502, all from the syntax check (a slur's parenthesis no longer counts as a Scheme one); median 0.077 ms. Changed from the plan: `expansion-limit` is not needed (past a bound the reading fails, and `check_lilypond` reports `too-large`); `unsupported-value` for values lytk cannot hold; `\time 1/0` is a warning, as in LilyPond 2.26. Three syntax checks beyond the tree: a stray `>>`, the leftovers of a Scheme `(` deleted, a markup `\override` without a pair. `unknown-command` knows LilyPond 2.26's 1,347 commands (`scripts/ly_builtins.py`) |
| J4 | **Strings and headers.** `extract_string_value` (`consume.rs:442-451`) joins every `string_fragment` and decodes each `escape_sequence` as LilyPond's lexer does (`lily/lexer.ll`: `\n` `\t` `\\` `\'` `\"`; any other backslash is kept). One fix for all fifteen callers: headers, lyrics, `\tempo`, markup, `\with`, `\mark`, `\clef`, `\bar`, `\set`, `instrumentName`, `\lyricsto`, `\context = "…"`, `\language`. Header values given as `\markup` become their plain text, and `#"…"` becomes a string. Headers are scoped: a `\score`'s `\header` no longer leaks into the next movement, and a top-level `\header` applies to every movement. *Python:* `Score.header` / `MusicDocument.header` → `dict[str, str]` of every field, plus `MusicDocument.lyricist`; `lytk.header_fields(text) -> list[HeaderField]` (key, value, byte span, score index) from the tree, no IR, so a caller can read a field and cut it. *Writers:* the Score path writes `\header` whenever any field is set (today only for title/composer/arranger/lyricist, `ir_to_ly/mod.rs:156-159`); the Music path writes lyricist and `extra` (`music_emit.rs:129-151`); `extra` keys are written sorted, not in `HashMap` order | String round trip in every context with `\"`, `\\`, newline and non-ASCII; `texidoc` and `categories` of all 389 official snippets equal to an independent escape-aware decoder | M | ✅ 2026-09-27: `ly_to_ir/text.rs` decodes strings, `#"…"` and markup text; the 389 snippets' `texidoc` and `categories` (778 values, 108 with escapes) equal the oracle (`snippet_headers`); the string round trip covers headers, `\tempo`, `\mark`, instrument names, lyrics and markup text (`tests/ly_text.rs`). `HeaderField` spans are character offsets in Python, as `Diagnostic`'s. Also: the Score-path writer now writes note text directions (the round trip found them lost) |
| J5 | **Pitch language.** `\include "<file>.ly"` for LilyPond's language files sets the language as `\language` does: the 11 lytk supports; `arabic.ly` → italiano; `bagpipe`, `makam`, `persian`, `turkish-makam` → `unknown-language` warning. `\language` is honoured inside `\score` and music (today top level only, `walk.rs:36-46`). An unknown name keeps the current language and warns (today it resets to Dutch, `walk.rs:41-42`) | `\include "english.ly" { cs4 d4 }` reads C♯4 D4 (today D4 only) | S | ✅ 2026-09-27: `hel-arabic.ly` joins the unreadable ones; a variable read again inside `\relative` keeps the language of its definition. On LilyPond 2.26's corpus `unrecognized-token` fell from 595 to 437 and `ignored-include` from 245 to 221 |
| J6 | **Release hygiene.** `lytk.__version__` (`env!("CARGO_PKG_VERSION")` exported by `_core`); one version source (pyproject `dynamic = ["version"]`, or a test that `Cargo.toml` and `pyproject.toml` agree); Python CI on 3.10–3.13 (only 3.12 today; lilycorpus supports 3.10); the release workflow fails when the tag differs from the version. Docs that say the reader follows `\include` (changelog 0.1.0, SECURITY.md; CLAUDE.md done in J3, `docs/import-export.md` in J5) are corrected; so are README's pre-0.2.0 MIDI limitations, `design.md` test counts, `development.md`'s Rust CLI, `python.rs`'s "python feature" and T11.2's batch `catch_unwind` (deleted with `src/main.rs`). The upstream commit of tree-sitter-lilypond is recorded in `src/tree-sitter/README.md` | CI green on four Pythons; `import lytk; lytk.__version__ == "0.3.0"` | S | ⬜ |

Order: J0 → J1 + J2 → J3 → J4 + J5 → J6.

### Epic K — 0.4.0: source-level API

| Task | Description | Size | Status |
|------|-------------|------|--------|
| K1 | **`\version`.** Keep it in `ScoreMetadata` and expose it as `Score.lilypond_version`. `lytk.lilypond_version(text)` reads it from the tree (a commented-out one does not count). `set_lilypond_version(text, v)` and `strip_lilypond_version(text)` edit by span. A `LilyPondVersion` type compares numerically (`2.24` == `2.24.0`). `to_lilypond(…, version=)` and `to_lilypond_music(…, version=)` expose `IrToLyAdapter::with_version`: Python always gets `\version "2.24.0"` today (`python.rs:398, 421`). No convert-ly wrapper | S | ⬜ |
| K2 | **Includes.** Bind `flatten_string(text, *, base_dir=None, include_paths=(), add_markers=True)` (Rust `flatten_str` exists, `ly_flatten.rs:100-107`). Add `include_paths=` on the readers: given, they flatten first and report unresolved includes as `ignored-include`; not given, includes stay unfollowed, as SECURITY.md should say. Find includes on the tree instead of line by line: this catches includes not at the start of a line and fixes the one-line `%{ %}` that leaves flatten "inside a comment" and the false `MultipleHeaders` for a top-level plus a per-score `\header` | M | ⬜ |
| K3 | **Tokens.** `lytk.tokenize(text) -> list[Token]` (kind: comment, string, scheme, command, symbol, number, fraction, punctuation; text; byte span; line; column) from the tree's leaves; `lytk.strip_comments(text)`. Scheme can be sub-tokenized with the compiled but unused `LANGUAGE_LILYPOND_SCHEME` | S | ⬜ |
| K4 | **Statistics.** `lytk.info(score) -> dict` becomes the library home of `lytk info --json` (moved from `cli.py:484-529`; the CLI calls it), plus voices, total bars, duration in quarters, lyrics, chord symbols and grace notes. `lytk.source_stats(text)` from tokens: comments, Scheme expressions, bytes, tokens | S | ⬜ |
| K5 | **Movements.** Single-score readers report what they drop (`dropped-music`, from J3); add `from_lilypond_music_movements` for Layer-1 users (the datasets) | S | ⬜ |

### Epic L — 0.5.0: datasets for curated corpora

| Task | Description | Size | Status |
|------|-------------|------|--------|
| L1 | **`FolderDataset` robustness.** `on_error="raise" \| "skip" \| "warn"` with `dataset.errors` (today the first bad file ends every iteration). Cache keyed by file content + lytk version + normalized representation kwargs, written atomically (today keyed by path, never invalidated; `to_note_arrays()` and `iter_representation("note_array")` write different files). `Subset` delegates `_convert_item` to its parent: today it skips the cache, so the README's `split()` → dataloader example never uses it. `movements="first" \| "all"`. Pass-through of `language`, `include_paths`, `strict` and MIDI `quantize` | M | ⬜ |
| L2 | **Records dataset.** `RecordsDataset.from_jsonl(path, *, text_field="text", id_field="id", format="lilypond", split_field=None)` and `from_records(iterable)`. Items keep their id and metadata; `split(field=…)` returns the records' own splits (a deterministic, decontaminated split must not be re-shuffled by ratio). `Dataset.split(…, groups=)` for group-aware ratio splits | M | ⬜ |
| L3 | **Identity.** `FolderDataset.ids` (paths relative to the root); `return_ids=True` on the torch and tf adapters | S | ⬜ |
| L4 | **Python API reference.** `docs/python-api.md` generated from or checked against the stubs: lytk has none today (README + `_core.pyi`). It states that `from_lilypond_string` and `from_lilypond_music_string` share one parse (the Music tree is lifted from the Score) | S | ⬜ |

### After 0.5.0

The pre-1.0 queue resumes: **P5** (enumerated notation types, the one
epic that breaks the Python API) as 0.6.0, then P3 → P4 → P6 → P8 → 1.0.0.
Decision for the owner: J–L go before P5 because they unblock lilycorpus
and are backward compatible, whereas the 0.1.0 plan put P5 first. P3's
import-options object should absorb J3/K2's keywords one to one:
`LilyPondReadOptions(language, strict, include_paths)`.

Not planned:
- Semantic validation equal to LilyPond's. User-defined commands can come
  from files lytk never reads, so compiling stays the authority. Strict
  mode means "well-formed, and nothing lytk had to drop".
- A convert-ly wrapper: callers already run convert-ly.
- Making numpy and typer optional: `import lytk` imports neither already,
  and moving typer to an extra would break `pip install lytk && lytk …`.

---

## Humdrum (`**kern`) support — 2026-07-10 ✅

New `humdrum_to_ir`/`ir_to_humdrum` adapter pair (Layer-1, ABC-shaped), all
surfaces wired (CLI `.krn`, Python `from_humdrum`/`to_humdrum`, batch, fuzz
net). music21 kern corpus: 1325/1328 round-trip cleanly; spine rearrangement
(`*^`/`*v`) is a documented clear error. `lower_to_score` now honours
`metadata.partial_duration` (anacrusis). Details in the devlog.

---

## Correctness backlog — 2026-07-10 full review (planned ⬜)

A multi-agent audit + empirical round-trip sweep (313 MusicXML files incl. the
159-file LilyPond acid corpus, 35 .ly fixtures, 52 MIDI cases) found zero
crashes but a set of **silent data-loss and correctness defects**. Full detail
with evidence in `docs/devlog.md` (2026-07-10 entry). Priority order:

| # | Area | Defect | Sev |
|---|---|---|---|
| R1 | ir_to_ly | ~~`\repeat`/`\alternative` re-emission malformed~~ **✅ 2026-07-10** (producer barline placement + stateful emitter) | HIGH |
| R2 | ly_to_ir | ~~`part_is_dynamics_only` collapses all-rest parts~~ **✅ 2026-07-10** (spacers-or-directions rule) | HIGH |
| R3 | transforms | ~~transpose: no key-aware respelling; diatonic key-sig from semitones; harmonies not transposed~~ **✅ 2026-07-10** (respell to target key, line-of-fifths delta, harmony root/bass) | HIGH |
| R4 | transforms | ~~retrograde: attributes not moved, tie/slur/tuplet pairing not reversed, graces stranded~~ **✅ 2026-07-10** | HIGH |
| R5 | midi | ~~cross-voice same-pitch overlap drops notes~~ **✅ 2026-07-10** (FIFO pairing + off-before-on; fidelity gate 4/4/3) | HIGH |
| R6 | cli | ~~multi-`\score` input silently writes `out_01.*` instead of the requested path~~ **✅ 2026-07-10** | MED |
| R7 | mxml_to_ir | ~~microtone alters, orphan/id-less parts, quote-tokenizer drops, words escaping~~ **✅ 2026-07-10** (xml fixtures 142/142, acid 158/159) | MED |
| R8 | surfaces | ~~compressed-MXL write unreachable~~ **✅ 2026-07-10** (convert_mxl_bytes; CLI/batch/Python wired) | MED |
| R9 | bindings | ~~Layer-1 transforms + `transpose_to_key` unbound; ly→ly transforms flatten structure~~ **✅ 2026-07-10** (Score-or-MusicDocument dispatch; CLI Music path) | MED |
| R10 | perf | ir_to_mxml emission dominates; `resolve_variable` deep-clones (**GIL release + small clone/queue fixes ✅ 2026-07-10**; rest deferred, benchmark first) | MED |

Done in the review pass itself: 4 malformed-input panic fixes (+ firewall
widening), warning/clippy zero, CLAUDE.md/README doc corrections. ✅

---

## Pre-1.0.0 Expansion — MuseScore-comparison backlog (planned ⬜)

Sourced from a lytk-vs-MuseScore CLI/converter comparison (see the comparison
report + `memory/musescore-comparison-backlog.md`). Full atomic-task detail,
rationale, and file anchors live in the plan file
`~/.claude/plans/add-as-a-next-mighty-matsumoto.md`. **These 12 epics gate the
1.0.0 release** (single milestone, by owner decision). A **0.1.0 preview** ships
before them (2026-09-24 decision); P5 goes first afterwards because it is the only
epic that breaks the Python API.

Three scoping decisions:
- **Licensing:** lytk is **MIT** (relicensed 2026-09-24; was GPL-2.0-only);
  MuseScore (GPL-3.0) may be *read to learn the approach* but not
  copied/transcribed — write lytk's own (ideally better) implementation.
- **Layout (PE-Epic 4 / item 12):** target is **sensible default positions /
  placement hints**, NOT a layout/spacing engine.
- **Testing:** the `tests/fidelity.rs` semantic scoreboard stays a **hard,
  non-decreasing CI gate**; visual regression deferred (no rendered output).

Epic labels are `P`-prefixed to avoid colliding with the historical Epic 1–3 /
A–H above. Build order (waves): **P1, P10, P11, P2, P4.T4.1** → **P3, P4, P5,
P7** → **P6, P8** → **P9**; **P12** enforced throughout.

### PE-Epic P1 — CLI & I/O ergonomics *(items 1, 6)* 🟡
| Task | Description | Status |
|------|-------------|--------|
| T1.1 | stdin/stdout streaming (`-`); `--from` for stdin input, `--format` for stdout output | ✅ |
| T1.2 | CLI `invert` subcommand (`--axis c4`/`fs3`/`bf5`) | ✅ |
| T1.3 | CLI `retrograde` subcommand | ✅ |
| T1.4 | CLI `change-language` subcommand (`-l <lang>`) | ✅ |
| T1.5 | `abs2rel` subcommand — re-emit LilyPond in `\relative` form (Score path) | ✅ |
| T1.6 | `rel2abs` subcommand — re-emit LilyPond in absolute form | ✅ |
| T1.7 | LilyPond `indent` + `reformat` CLI (source-preserving reindenter over tree-sitter) | ⬜ deferred — needs a whitespace-only, comment-preserving reindenter; a parse→emit shortcut would be lossy and redundant with `convert in.ly -o out.ly` |

### PE-Epic P2 — Transpose modes & enharmonic spelling *(items 10, 11, 3)* ✅
| Task | Description | Status |
|------|-------------|--------|
| T2.1 | `Interval` type (`src/ir/interval.rs`, `(diatonic, chromatic)` + name parser) + `TransposeMode { Chromatic, Diatonic }` | ✅ |
| T2.2 | `Pitch::transpose_diatonic` — spelling-correct (distinct M3 vs d4 etc.), microtone-safe | ✅ |
| T2.3 | `pitch::respell(pitch, fifths)` — key-aware enharmonic spelling | ✅ |
| T2.4 | `transpose_interval`/`transpose_to_key` fns; CLI `--interval`/`--to-key` (mutually exclusive); Python `transpose_interval` + stub/re-export | ✅ |
| T2.5 | ABC emitter respells against the active `K:` (enharmonic spelling follows the key; explicit accidentals still printed — see note) | ✅ |

### PE-Epic P3 — Structured import/export option objects *(item 7)*
| Task | Description | Status |
|------|-------------|--------|
| T3.1 | `MxmlExportOptions`/`ExportOptions` (divisions, layout, breaks, invisible, compat, text_inference) | ⬜ |
| T3.2 | Thread via `with_options()`; mirror import options | ⬜ |
| T3.3 | Honor layout/breaks/invisible booleans (replace hardcoded path) | ⬜ |
| T3.4 | `CompatMode` (Generic/Finale/MuseScore/Dorico) + text-inference toggle | ⬜ |
| T3.5 | Surface to CLI + Python | ⬜ |

### PE-Epic P4 — MusicXML fidelity: validation, keys, time, positions, repeats *(items 2, 4/24, 23, 12, 20)*
| Task | Description | Status |
|------|-------------|--------|
| T4.1 | Opt-in MusicXML XSD validation (`--validate`, off by default; XSD in `musicxml-std/schema/`) | ⬜ deferred — true XSD validation needs a native libxml2 dep (build + Python-wheel cost); revisit on demand. The `musicxml` crate already rejects malformed/wrong-structure XML on parse. |
| T4.2 | Non-traditional key sigs — IR (`KeySignature::Custom`) | ⬜ |
| T4.3 | Non-traditional key sigs — MusicXML parse (`part.rs:354`) + emit | ⬜ |
| T4.4 | Non-traditional key sigs — LilyPond + ABC round-trip | ⬜ |
| T4.5 | Time sigs — IR (interchangeable, single-number, senza-misura at ts level) | ⬜ |
| T4.6 | Time sigs — adapter round-trip (mxml + ly) | ⬜ |
| T4.7 | Default positions: conservative `default-y` on directions (dynamics/wedge/pedal/words) by placement ✅; `placement` + `<print>` breaks already emitted ✅; `<staff-layout>` staff-distance ⬜ deferred (editors default it; needs multi-staff threading for marginal value). `default-x` intentionally unset (no spacing engine). | 🟡 |
| T4.8 | Measure-repeat — `Measure.measure_repeat: Option<u8>` (serde default/skip) | ✅ |
| T4.9 | Measure-repeat — MusicXML `<measure-style><measure-repeat>` round-trip ✅; LilyPond `\repeat percent` ⬜ deferred (Score→LY path) | 🟡 |

### PE-Epic P5 — Enumerated notation typing *(item 13)*
| Task | Description | Status |
|------|-------------|--------|
| T5.1 | `ArticulationType` enum (+ `Other(String)`) | ⬜ |
| T5.2 | `OrnamentType` enum | ⬜ |
| T5.3 | `TechnicalType` enum | ⬜ |
| T5.4 | `DynamicType` enum | ⬜ |
| T5.5 | Migrate adapters + Python API; `str<->enum` map keeps fixtures green | ⬜ |

### PE-Epic P6 — Beams & tuplets *(items 21, 22)*
| Task | Description | Status |
|------|-------------|--------|
| T6.1 | Beam model: typed levels/hierarchy + fan-beam | ⬜ |
| T6.2 | Beam round-trip (mxml `<beam>` + ly) | ⬜ |
| T6.3 | Tuplet model: nesting (recursive) | ⬜ |
| T6.4 | Tuplet state machine (open/continue/close across a voice) | ⬜ |
| T6.5 | Tuplet round-trip (mxml `<time-modification>/<tuplet>` + ly `\tuplet`) | ⬜ |

### PE-Epic P7 — Harmony & figured-bass analysis *(item 19)* 🟡
| Task | Description | Status |
|------|-------------|--------|
| T7.1 | `Harmony.function: Option<String>` (MusicXML `<function>` Roman numeral; serde default/skip) | ✅ |
| T7.2 | MusicXML round-trip: parse `<function>` on import, emit on export | ✅ |
| T7.3 | Figured-bass enhancements (typed accidentals, extension lines) | ⬜ deferred — low value; `Figure` already has prefix/suffix. (Numeral element + root-optional functional harmony also deferred — would ripple `Harmony.root` to `Option`.) |

### PE-Epic P8 — Instruments, tablature, percussion, fretboard *(items 15, 16, 17, 18)*
| Task | Description | Status |
|------|-------------|--------|
| T8.1 | Instruments: mid-part instrument changes as Part state; GM completeness | ⬜ |
| T8.2 | Tablature — IR (string/fret) | ⬜ |
| T8.3 | Tablature — MusicXML + LilyPond round-trip | ⬜ |
| T8.4 | Percussion — IR unpitched/drum note variant | ⬜ |
| T8.5 | Percussion — mxml/ly/midi round-trip + drum-name map | ⬜ |
| T8.6 | Fretboard diagrams — IR + MusicXML `<frame>` + LilyPond `\fret-diagram` | ⬜ |

### PE-Epic P9 — MIDI reconstruction (study-then-reimplement) *(item 14)*
Depends on P6 (tuplets) + P8 (percussion). Study MuseScore `importmidi_*` for the
approach; write lytk's own. Each sub-task is independently SMF-fixture-testable.
Scheduled as Epic I's last phase (ID), after the reader rework for
notation-exported MIDI (IC).
| Task | Description | Status |
|------|-------------|--------|
| T9.1 | Configurable quantization grid | ✅ `MidiToIrAdapter::with_quantize`, `from_midi(…, quantize=16)` (played files) |
| T9.2 | Clef guessing (pitch centroid) | ✅ (IC) |
| T9.3 | Voice separation (≤4 voices) | ✅ (IC) |
| T9.4 | Tuplet detection (emits P6 tuplets) | ✅ exported files: 3/5/6/7/10/12 a beat (IC); played files: triplets by a Viterbi search over plain and triplet points, two onsets off the 16th grid a beat (ID) |
| T9.5 | L/R hand split (piano) | ✅ MuseScore's cost model (hand span, textures, a busy hand) as a Viterbi over chord cuts, plus a movement cost (ID) |
| T9.6 | Drum mapping (ch-10 → P8 percussion) | ✅ percussion part and clef; hits held to the next (IC/ID) |
| T9.7 | Swing detection/normalization | ✅ swung eighths (3:2 to 2:1) straightened and marked "Swing", in played files by the beats' votes, else `swing=True`; a 3:1 shuffle reads dotted unless asked (ID) |
| T9.8 | Lyrics/karaoke extraction | ✅ lyric events (IC), karaoke text events (ID) |
| T9.9 | Articulation inference (gate-time → staccato) | ✅ notation files: 4 louder (LilyPond, lytk); played files: a note lengthened by 30 % or more to one written value (MuseScore's rule); legato gaps closed (ID) |

### PE-Epic P10 — Machine-readable automation outputs *(item 8)* ✅
| Task | Description | Status |
|------|-------------|--------|
| T10.1 | `info --json` — curated metadata + parts (id/name/measures/staves/program) + note count | ✅ |
| T10.2 | `positions` — per-part measure start/duration in quarter notes (temporal, not graphical) | ✅ |
| T10.3 | `bundle` — export each part to `<stem>_<part>.<ext>` in a dir | ✅ |
| T10.4 | `diff a b [--json]` — semantic compare (parts/note-count/pitch-multiset), `diff`-style exit code | ✅ |

### PE-Epic P11 — JSON batch-job API *(item 5b)* ✅
| Task | Description | Status |
|------|-------------|--------|
| T11.1 | Batch-job JSON schema `{in/input, out/output, format?, from?, transpose?, interval?}` | ✅ |
| T11.2 | `batch` executor (reuses rayon pool + per-job `catch_unwind` + non-zero exit on any failure) | ✅ |
| T11.3 | Per-job transform options (transpose/interval) ✅; visible-parts filter + filename templates ⬜ (overlap excerpt-selection, deferred) | 🟡 |
| T11.4 | Diagnostic sidecars: `--report <path>` JSON of per-job ok/error | ✅ |

### PE-Epic P12 — Testing gate & CI *(item 9)* — ongoing
| Task | Description | Status |
|------|-------------|--------|
| T12.1 | Formalize semantic scoreboard as hard CI gate (non-decreasing) | ✅ `tests/fidelity.rs` runs in `cargo test` on every CI OS against committed non-decreasing baselines |
| T12.2 | Document visual-regression deferral (no rendered output → N/A) | ⬜ |
| T12.3 | Convention: each epic adds fidelity fixtures + bumps baselines in-PR | ⬜ |

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
| **11** | **P1–P12** | **Pre-1.0.0 expansion (MuseScore-comparison backlog): CLI/IO + transforms, MusicXML fidelity, IR modeling, MIDI reconstruction — see plan file** |
| **12** | **0.1.0** | **Preview release before the remaining P-epics; then P5 → P3 → P4 → P6 → P8 → P9** |
| **13** | **I → 0.2.0** | **MIDI and ABC conversion repair (Epic I, with P9 performed MIDI): tagged `v0.2.0` 2026-09-27** |
| 14 | J → 0.3.0 | Trustworthy LilyPond reading: no panics, diagnostics + strict mode, whole strings and headers, pitch-language files |
| 15 | K → 0.4.0 | Source-level API: `\version`, includes from strings, tokens, statistics, movements |
| 16 | L → 0.5.0 | Datasets for curated corpora: robust `FolderDataset`, records dataset, ids, Python API reference |
| 17 | P5 → 0.6.0, then P3 → P4 → P6 → P8 → 1.0.0 | Pre-1.0 queue resumes (P5 breaks the Python API) |

## Key Decisions

- **Split before rewrite:** Split ly_to_ir.rs (E2T1) first as a pure refactor, then rewrite each sub-module to emit Music tree (E1T2). Lower risk, easier to review.
- **Full Forward/Backup removal:** Remove from VoiceElement entirely (E1T1). Convert to spacer rests in mxml_to_ir, generate during serialization in ir_to_mxml. Clean break.
- **Fidelity before features:** v1.0.0 prioritizes making the existing three formats round-trip *semantically* (Epics B/C) before adding ML surface area. A real test bar prevents "parses-but-wrong" regressions.
- **ML representations are in-scope for v1.0.0:** the stated goal is symbolic-music generation/understanding, so note-array/event/piano-roll + datasets + metrics ship in v1.0.0 (modeled on muspy), not deferred.
- **Representations go through the Music tree**, not Score — format-agnostic, reuses `Frac` durations and `moment.rs` offsets.
- **New formats priority:** ABC first (simplest, many folk datasets). MEI stays deferred past v1.0.0 (Humdrum `**kern` shipped 2026-07-10).

## Deferred past v1.0.0

### Epic 8 (remainder): MEI adapter
MEI parser/emitter (`mei_to_ir.rs` / `ir_to_mei.rs`). Reference material in `MEILER/`.
(Humdrum shipped 2026-07-10 — see above.)

### Generation-evaluation metrics: JS-similarity & Fréchet Music Distance ✅
Implemented in `src/lytk/metrics.py` (ported from `lilybench/`), behind the
optional `lytk[eval]` extra (lazy imports — the module loads without the deps):
- **JS-similarity** — `js_similarity` / `js_descriptor_similarity`;
  `100·exp(-2·mean(JS div))` over Gaussians fit to the `polyphony_rate`,
  `groove_consistency`, `scale_consistency` descriptors, sourced from lytk's own
  `compute_metrics` (no muspy round-trip). Needs `scipy`.
- **Fréchet Music Distance** — `frechet_music_distance` (numpy + scipy) +
  `lilybert_embed` (LilyBERT layer-6 embeddings of raw `.ly`; needs
  `torch`/`transformers` + a checkpoint).
Tests: `tests/test_metrics_eval.py`.

### Audio rendering / synthesis (v1.1)
Listen-back via a synthesizer (reference muspy/symusic synth).

### music21 Feature Parity (v2)

[music21](https://web.mit.edu/music21/) is the standard Python toolkit for Music
Information Retrieval (MIR) but is slow, poorly designed, and frequently buggy. lytk aims to provide equivalent or superior analytical capabilities with a clean API and Rust performance. Planned for v2 or a separate package.

Areas: Pitch & Interval Analysis, Score Analysis (key-finding, ambitus, histograms), Rhythm & Meter, Harmony & Voice Leading, Melodic Analysis, Data Augmentation Transforms.

## Out of Scope (v1)

These MusicXML elements have no planned IR representation:
- `<harp-pedals>` / `<accordion-registration>` — hardware-specific notation
- `<scordatura>` — tuning override notation
- `<percussion>` pictogram elements — complex symbol table
- `<image>` — embedded raster images
- `<listen>` / `<listening>` — performance instructions (MusicXML 4.0 new)
- `<staff-divide>` arrow — orchestral condensed score notation
