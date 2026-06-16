# Changelog

## 2026-06-16 (cont.) — Simplicity pass ("keep it simple")

A behavior-preserving readability/simplification sweep across the package
(driven by a parallel multi-agent review, then ablated one change at a time
against the full test suite). No public behavior changed: 514 Rust unit +
integration tests, 117 Python tests, `cargo clippy -D warnings`, and `uv build`
all stay green.

**Removed duplication / dead code**
- `ly_to_ir::apply`: the chord attachment handler's second loop was a ~190-line
  verbatim copy of `apply_note_attachments`. It now delegates to that function
  (filtering out `\arpeggio`/`\glissando`, which the chord-specific first loop
  still handles); the chord and its notes share one duration so beam levels are
  identical. Biggest single cleanup.
- Batch CLI (`main.rs`): dropped a dead `transform: Option<&F>` generic that the
  only caller always passed as `None`; `run_batch`/`process_one_file` are now
  plain functions.
- `transpose_key`: removed a `-7..=7` clamp that could never fire (every table
  entry is already in range) — output is unchanged.
- New small shared helpers replacing copy-paste: `push_wrapped` + `octave_marks`
  (ir_to_ly), `rest_direction_elements` (ir_to_mxml, was triplicated),
  `note_audible` + `placement_or_unspecified` (mxml_to_ir), `parse_language`
  (lib.rs, ×6), `next_channel` (ir_to_midi), `make_time_sig`/`make_key_sig` and
  a `rev().find_map` (midi_to_ir), `dedup_keep_last` (ly_flatten), and a `build`
  helper shared by MusicXML `convert`/`write`.
- Merged identical match arms (group contexts and Volta/Unfold repeats in
  `lower/walk.rs`; a redundant time-sig-boundary split in `lower/build.rs`),
  dropped two dead params from `build_voice_from_events`, and deleted a dead
  `_alt_blocks` traversal in `ly_to_ir::modifiers`.
- Python: `_invert_ext` reduced to its two real cases; `Dataset._convert_item`
  no longer threads a redundant `fn` (derivable from `representation`);
  `Dataset.split` flattened.

**Left as-is (deliberately)**
- Reverted one suggested change: the `let result = …; result` bindings in the
  tree-sitter walkers are load-bearing (they keep a cursor borrow alive); the
  borrow checker rejects the "simpler" direct return.
- Skipped a few low-gain/medium-risk suggestions (inline `\relative` reuse in
  the parser, a `beats_fraction` swap with a `.max(1)` edge case, a closure→fn
  in the LY emitter) to avoid risk that outweighs the readability gain.

## 2026-06-16 — Second REVIEW.md round: batch exit codes, CLI parity, measure labels

Addressed a fresh round of review findings (behavior/CLI/IR), all approved approaches.

**Fixed**
- **High — batch conversion masked partial failures.** Both `run_batch` (`src/main.rs`)
  and the Python `_run_batch` printed per-file errors but still exited `0`. They now count
  failures, print a summary, and exit non-zero when any file fails — while still converting
  the valid files. Tests in `tests/cli.rs` and `tests/test_cli.py` feed a malformed file
  alongside a good one and assert the non-zero exit + that the good file is still produced.
- **Medium — Python `lytk` lacked the documented `flatten` subcommand.** `flatten` had no
  Python binding at all. Added a PyO3 `flatten(input, output=None, *, include_paths=None,
  add_markers=True)` wrapping `ly_flatten::flatten`, exported `lytk.flatten` + a `.pyi`
  stub, and registered the `flatten` subcommand (`-o`, repeatable `-I`, `--no-markers`) in
  `cli.py`. Now at parity with the Rust CLI. + CLI tests.
- **Medium — non-numeric MusicXML measure numbers collapsed to `0`.** Added
  `Measure.number_label: Option<String>` (`#[serde(default, skip_serializing_if)]` so
  existing numeric-measure JSON is unchanged). The importer preserves the raw label whenever
  it isn't the plain decimal of `number` (e.g. `"3A"`, `"X1"`, `"03"`); the MusicXML exporter
  emits it verbatim. Round-trip test in `src/adapters/mxml_to_ir/tests.rs`.
- **Low — `cargo test` was red (`cli_help_flag`) — a regression from the prior round.**
  Adding a `Cargo.toml` `description` made clap source the `--help` banner from it
  (hyphenated "music-notation"). The Rust CLI now sets an explicit `about` matching the
  Python entry point ("lytk — music notation conversion and augmentation toolkit."), keeping
  the manifest description for package metadata. Suite is green again.

Counts: 876 Rust tests (514 unit + 362 integration) + 117 Python; `cargo clippy -D warnings`
and `uv build` both clean.

## 2026-06-15 (cont.) — REVIEW.md follow-up: dataset/CLI/docs fixes

Addressed the four findings raised in `REVIEW.md` (package/ML-layer polish; the
parser/IR/adapter core was already sound).

**Fixed**
- **Cache-key collision in `FolderDataset` (correctness).** The on-disk representation
  cache keyed only on `path.stem` + representation + kwargs, so with the default recursive
  discovery `train/foo.ly` and `valid/foo.xml` clobbered each other. The key now includes a
  stable short hash of the path relative to the dataset root (`_path_hash`), so distinct
  sources never share a cache file. Regression test in `tests/test_datasets.py`.
- **Python CLI `--jobs` was a no-op.** The shipped `lytk` console script declared `--jobs`
  but ran batch conversion serially (help text said "Currently ignored"). Batch mode now
  dispatches per-file work across a `ProcessPoolExecutor` honoring `--jobs` (0 = auto, one
  per CPU; 1 = serial). Output subdirectories are pre-created in the parent to avoid worker
  races. Tests assert the pool is actually used for `--jobs > 1`, skipped for `--jobs 1`,
  and that a real cross-process run is byte-identical to the serial run.
- **Eager dataset materialization.** `to_pytorch_dataset` / `to_tensorflow_dataset` no longer
  materialize the whole corpus up front — they convert (and cache) one item at a time on
  access. Added `Dataset.iter_representation()` (a memory-bounded generator); `to_representation`
  now delegates to it as the explicit eager "materialize all" path.
- **Stale release-facing docs/metadata.** README moved ML representations, the ABC adapter,
  and dataset loaders out of "Not yet implemented" into "Implemented", and refreshed the test
  counts (874 Rust + 112 Python). Added crate metadata to `Cargo.toml`
  (description, authors, license, homepage, repository, documentation, keywords, categories);
  `uv build` no longer warns about missing manifest fields.

## 2026-06-15 (cont.) — E2E cross-format conversion fidelity audit + fixes

Built an end-to-end audit converting every fixture A → format B (ly/xml/mxl/midi/abc),
re-parsing, and comparing the note signature (pitch multiset / count / durations) via the
format-agnostic note-array. Added `tests/test_e2e_conversion.py` to pin the lossless
invariant (pitch multiset preserved across A→{ly,xml,abc}→A) + regressions. The audit
(a 7-way parallel root-cause workflow, each verdict adversarially verified) found:

**Fixed**
- **ly→ly relative-mode octave shift** (single-staff): `\relative a' { a' }` re-parsed an
  octave high — the first note's marks were absolute-from-C3, not relative to the reference.
  Seed the emit body with the reference pitch (`ir_to_ly/{parts,emit}.rs`). Fixed ~13 fixtures.
- **ly→abc emitting zero notes**: `ir_to_abc` walked only the first branch of a `Simultaneous`,
  so Music21 fixtures (`Simultaneous[empty staff, real staff]`) produced nothing. Walk every
  branch, descend into the one with the most events. 0 → full note content.
- **midi→midi note multiplication** (ties): `ir_to_midi` was tie-unaware, emitting a
  NoteOn/Off per tied segment. Collapse tie chains into one sounding note.

**Found, root-caused, not yet fixed** (documented for follow-up)
- **ly→ly relative shift, multi-staff/multi-voice**: the `<<\\>>` relative flow + shared
  per-staff `\relative` reference still shifts; needs per-voice prev-threading.
- **midi→ly duration change**: `ir_to_ly` ignores a note's tuplet ratio for bare notes
  (triplets emit without `\tuplet`), so MIDI-quantized triplets change duration on re-parse.
- **xml→ly note loss** (one fixture): a backward repeat with no preceding forward (repeat-from-
  the-top idiom) desyncs the `\repeat volta` block tracking in `ir_to_ly/emit.rs`.
- **ly→midi grace notes**: `ir_to_midi` has no grace handling, so graces steal metrical time.

**Format-inherent (not bugs)**
- **→ ABC for polyphony**: ABC v1 emits a single melodic line; multi-voice/piano sources drop
  voices (full fix = ABC v2 `V:` multi-voice support — a feature).
- **→ MIDI**: durations are quantized and free-time (`\cadenzaOn`) can't be represented, so
  duration equality and exact note boundaries are not preserved (pitch set is).

## 2026-06-15 (cont.) — Epic G: Python distribution & docs (release readiness)

Release-prep, minus the version tag (intentionally deferred).

- **ABC bound to Python.** `from_abc`, `from_abc_string`, `to_abc` (emission lifts
  Score→Music internally) added to `lytk._core`, re-exported from `lytk`, and stubbed
  in `_core.pyi`. The Python CLI gains full `.abc` support (read/write/info, `-f abc`).
  5 new pytests. (The Rust ABC adapter from Epic E was previously unreachable from
  Python.)
- **Wheel/release workflow.** `.github/workflows/release.yml` builds abi3 wheels
  (Linux x86_64+aarch64, macOS x86_64+arm64, Windows x64) + an sdist via
  `PyO3/maturin-action`, with a PyPI publish job (Trusted Publishing/OIDC) gated on a
  `v*` tag. One abi3 wheel per platform covers CPython 3.10+.
- **Package metadata.** `pyproject.toml` now declares `license = "GPL-2.0-or-later"`
  (matching the repo's existing GPL-2.0 LICENSE), classifiers, keywords, project URLs,
  and `torch`/`tensorflow` extras — verified in the built wheel's METADATA.
- **Docs.** README Python quickstart expanded (ABC, representations, MIDI, extras);
  `docs/import-export.md` gains a top-level format-support matrix, a real ABC
  import/export section (was "planned"), and a corrected `\cadenzaOn/Off` entry.
- Verified: 78 pytests, 874 Rust tests, CI clippy clean, wheel builds.

## 2026-06-15 (cont.) — Epic H: end cadenza collapses to one senza-misura bar

The free-time `\cadenzaOn…\cadenzaOff` end cadenza rendered as ~10 over-full/empty
bars (chopin's mangled last page). It now collapses to ONE bar-less measure holding
both hands, followed by the strict-time 4/4 coda — matching the LilyPond reference
(verified by rendering both with `lilypond`/`mscore3`).

- **Cadenza measures stay flagged through the whole pipeline.** A per-measure
  `measure_has_cadenza` flag (state.rs) flags a bar senza even when `\cadenzaOff`
  cleared `cadenza_active` before its lazy flush; `resolve_variable` flags measures
  spliced from a sub-variable inside a cadenza (trebleCadenza splices cadenzaA/cadenzaB,
  which carry no `\cadenzaOn`); and the resplit/unify passes now **preserve**
  `senza_misura` (position spans) instead of dropping it via `Measure::new`.
- **`collapse_cadenza_runs` (merge.rs)** collapses each maximal run of ≥2 consecutive
  senza measures into one, re-joining each voice's element stream by number (the
  inter-bar gaps are meter-split artifacts). A lone senza bar is a run of 1 → untouched.
- **Score-wide collapse before the index-merge (walk.rs).** When every staff is in a
  cadenza (chopin's two hands), each staff's run is collapsed to one bar *before* the
  PianoStaff index-merge, so a longer treble cadenza can't fold the bass coda into its
  senza span. Single-hand cadenzas (pedal.ly) skip this and stay aligned via the
  auto-split.
- **Result:** chopin cadenza 10 bars → 1 senza bar (both hands); the 4/4 coda keeps both
  hands. pedal.ly's single-hand cadenza stays staff-aligned — its two free bars now merge
  into one genuine one-bar cadenza (accepted). 874 tests green, clippy clean. Test:
  `chopin_cadenza_is_single_senza_bar_with_both_hands`.

## 2026-06-15 (cont.) — Epic H: `\tuplet 3/2 4 {…}` fix resolves the RH/LH desync

The "RH ends ~70 beats before the LH" symptom was **not** a cadenza re-barring
problem — it was a tuplet parser bug, found by bisecting the over-long `lowerStaff`
stream (618 q vs 533 q expected; the whole +85 q lives in the LH **Agitato**, which
measured 250 q vs 168 q ≈ **3/2×**).

- **`\tuplet` ratio/group-duration form (`\tuplet 3/2 4 { … }`)** now parses. The
  `4` is LilyPond's optional group-duration argument (per-group beaming span; ratio
  unchanged). The parser expected the music block immediately after the fraction, so
  the block fell through and its notes parsed at **full duration** (3/2× too long)
  with no `<time-modification>`. chopin's LH Agitato wraps ~40 bars in one such
  tuplet, so the LH over-parsed by ~76 q and ran far past the RH (RH ended bar 166,
  LH bar 185). Fix: skip an optional `unsigned_integer` (+dots) between the fraction
  and the block (`music.rs`). No-op for the plain `\tuplet a/b { … }` form.
- **Result:** chopin **RH/LH gap 76.5 q → 6 q**, output **186 → 167 measures**, the
  ~19 phantom LH bars gone. 873 tests green, clippy clean. New test
  `test_parse_tuplet_with_group_duration_arg`.
- **Remaining:** the residual 6 q is the genuine free-time cadenza-length difference
  (`trebleCadenza` ≈31 q vs `bassCadenza` ≈24 q) — the LH cadenza ends ~2 bars before
  the RH. That is the senza-misura/bridging task (EHT4), now much smaller and no
  longer blocked by a large pre-cadenza misalignment. (This supersedes the
  "cadenza blocked / 20 extra LH beats" diagnosis below — that was a downstream
  symptom of the over-long LH Agitato being packed by the index-merge.)

## 2026-06-15 — Epic H (Steps 0–3): multi-voice collapse fixed; cadenza blocked

Atomic, non-regressive subset of the Epic H bar-splitting rework (branch
`epic-h-bar-splitting`; all 872 tests green at each step):

- **Step 1 — multi-voice collapse fixed (chopin post-Agitato desync).** A
  `<< { } \new Voice { \voiceTwo … } >>` left two *simultaneous* `Voice`s sharing
  a number; the `resplit_*` by-`v.number` flatten folded them into one over-full
  voice (bar 72 → 6/4), putting the RH 3 beats ahead of the LH for the rest of the
  piece. `disambiguate_colliding_voice_numbers` renumbers the colliding voice
  before each by-number flatten (no-op when already distinct → byte-identical
  elsewhere). Re-syncs the entire post-Agitato section. +3 tests.
- **Step 2 — `Measure.senza_misura` + `<senza-misura/>` exporter** (inert).
- **Step 3 — `\cadenzaOn/Off` parsed**, flags cadenza measures senza without
  suppressing auto-split (single-hand cadenzas stay aligned; pedal.ly gains a
  correct `<senza-misura/>`).
- **Step 4 — cadenza bridging attempted and reverted (BLOCKED).** Collapsing each
  staff's senza run into one aligned free bar is the right design, but chopin's
  hands reach their cadenzas at different absolute positions (RH ≈126 q vs LH
  ≈146 q — ~20 extra LH beats / 36 extra measures before the cadenza), so the
  union-of-spans can't merge them. Real prerequisite: align the staves through the
  cadenza-adjacent region first (the bars-156–158 spurious 9 q `skip-of-length`
  spacer-voice artifact + bassCadenza placement). Documented as the next task in
  Epic H (`docs/roadmap.md`).

## 2026-06-14 — `#(skip-of-length)`; chopin RH/LH-sync root-cause + Epic H plan

- **`#(skip-of-length VAR)`** now emits a spacer the length of music variable
  `VAR` (was ignored, leaving the bass cadenza ~9 beats short). Adds
  `WalkState::variable_total_duration` + an `embedded_scheme` music handler.
  Correctness fix; does not by itself re-sync the staves. 868 tests, clippy clean.

- **chopin RH/LH desync — diagnosed to root cause, planned as Epic H.** Stage-by-
  stage instrumentation of the full conversion pinned two failure classes that
  the main-body-perfect render still has: (1) multi-voice bars collapsing both
  branches into one over-full voice (bar 72 → 6/4, bar 156 → 9/4), so the RH runs
  3 beats ahead of the LH after the Agitato; (2) the free-time cadenza
  auto-splitting into a different bar count per hand, so the RH ends ~70 beats
  before the LH. Both stem from **bar-splitting happening during per-variable
  pre-parse at the wrong (default 4/4) meter** — `\time 6/8` lives in `\global`,
  resolved later — compounded by index-based voice merges and after-the-fact
  resplits. Verified: `merge_simultaneous_block` decides MERGE correctly, yet the
  collapse appears at different stages for different bars (bar 156 pre-merge, bar
  72 during the multi-staff merge), and disabling `unify` changes *which* bars
  break, not *whether*. The fix is a focused rework (defer bar-splitting to one
  authoritative position-based pass at score assembly; score-wide cadenza spans),
  written up as **Epic H** in `docs/roadmap.md` with exact code locations. Not
  attempted as ad-hoc edits to avoid regressing the bar-perfect main body.

## 2026-06-14 — `\repeat unfold N` + free-time cadenza investigation

- **`\repeat unfold N { … }` now writes the body out N times.** It was walked
  only once, dropping N−1 copies (e.g. chopin's `cadenzaA` lost half its notes).
  Fixed in `consume_repeat`. 868 tests (+`test_repeat_unfold_emits_n_copies`).

- **Free-time cadenza — investigated, deferred as a known limitation.** I built
  and verified the individual primitives (`\cadenzaOn`/`\cadenzaOff` senza-misura
  mode that suppresses auto-bar-splitting; `#(skip-of-length VAR)` Scheme idiom
  computing a variable's length; a `Measure.senza_misura` flag preserved through
  the re-barring passes; inlining nested cadenza variables) — each works in
  isolation and in single-staff/multi-staff cadenzas where the lanes are equal
  length. But the **full chopin cadenza is not yet supportable** and the
  machinery regressed `pedal.ly`, so it was reverted. Two architectural blockers:
  1. **`\cadenzaOn` is score-wide in LilyPond** (it suppresses the barline for
     *all* staves), but each staff/voice is parsed from an independent variable,
     so a cadenza in one hand (e.g. pedal.ly's `voicea`) leaves the other hand
     barred and the staves desync.
  2. The chopin cadenza has **three independent free-time lanes** (treble Staff,
     bass Staff, Dynamics) aligned only by `#(skip-of-length …)`; merging them
     into one measure needs flawless duration computation of very complex content
     (dotted-dotted `1..`, `\tuplet 2/1`, mixed spacers) — any rounding cascades
     into misalignment.
  Proper support needs score-wide cadenza timing + multi-lane free-time
  alignment (a dedicated epic). The `\repeat unfold` fix and the earlier ornament
  fixes do make the cadenza's note *content* more complete.

## 2026-06-14 — chopin_n.ly conversion: timing, ornaments, structure ✅

Branch `fix-chopin-conversion`. `tests/fixtures/ly/chopin_n.ly` (Chopin
Nocturne Op. 9 No. 3) converted to MusicXML with **121/181 bars wrong**. A
verified multi-agent audit isolated the causes; six fixes land them. The
**entire main body (bars 1–69) is now bar-for-bar correct** and the MuseScore
render matches the LilyPond reference (anacrusis, LH accompaniment, 5/7/4/8/9
tuplets, pedal below the bass, dynamics, trills, fermatas).

- **Tuplet chords overflowed the bar.** `apply_tuplet_ratio` scaled only the
  `Chord` wrapper's duration; the exporter reads each chord member's own
  duration, so tuplet chords emitted full un-scaled durations. Now every
  `c.notes[*]` carries the ratio (and `push_voice_element` folds the *product*
  of the whole `tuplet_stack` for nested tuplets).
- **`q` chord-repetition was dropped.** It parsed as an unknown pitch and fell
  through, losing notes and leaking the trailing tie/articulation onto the
  previous note. Added `WalkState.last_chord_pitches` and a `q` arm that
  re-emits those pitches with q's own duration + attachments.
- **Ties were never closed.** `~` recorded only a tie *start* (43 starts, 0
  stops). New `resolve_ties` postprocess pass adds a tie-stop to the next
  element of matching pitch in the voice (runs after q-expansion).
- **Slurs all used `number="1"`.** In a one-part piano score the RH and LH
  slurs cross-paired into giant slurs across both staves. `assign_slur_numbers`
  gives each voice a distinct lane.
- **`\partial` pickup absorbed into bar 1.** The 1/8 anacrusis lived inside the
  `upperStaff`/`lowerStaff` variables; the cadenza's per-staff time changes made
  `unify_staff_time_signatures` re-bar via `resplit_measures_with_time_changes`,
  whose boundary loop ignored the pickup — so the boundaries (0, 6/8, …) no
  longer aligned with the bars, the pickup merged into bar 1, and every later
  bar shifted (13/16, 11/16…). Both resplit paths now detect a short leading
  measure as an anacrusis, span the first boundary by it, and flag it implicit.
- **Fingered chords in grace blocks dropped.** `parse_grace_block` handled only
  bare note symbols, so `<gisis-1>`/`<cis-2>` inside an `\acciaccatura` were
  lost. It now flattens chord notes into the grace stream.

867 tests (+6 regression: q-repeat, tuplet-chord scaling, tie-stop, slur lanes,
pickup-in-variable); clippy clean. **Remaining (follow-ups):** the free-time
cadenza (`\cadenzaOn`/`\cadenzaOff`, Scheme `skip-of-length`, `\repeat unfold`)
— ~11 bars — and 6 scattered multi-voice measures in the 4/4 Agitato section.

## 2026-06-14 — Pedal renders below the left-hand staff (anchor + onset) ✅

Branch `fix-piano-multistaff-directions`. Follow-up to the same-day multi-staff
fix: in MuseScore 3 the sustain pedal in `pedal.ly` still rendered **above** the
left hand (in the inter-staff gap) instead of below it. Verified empirically by
round-tripping the emitted MusicXML back through `mscore3` (which re-anchored the
pedal **start** to `<staff>1</staff>`) and by rendering to PDF. Two root causes,
both fixed:

- **No anchor in empty staff bars.** `pedal.ly` bar 1 has the LH resting
  (`\skip 4*3`), so staff 2 was emitted as bare `<forward>` spacers — no
  ChordRest for the pedal `<direction>` to bind to. MuseScore then re-anchors the
  spanner to the only note at that tick (the RH on staff 1), dragging a
  `placement="below"` pedal into the gap. `ir_to_mxml/part.rs` now emits an
  **invisible rest** (`<note print-object="no">`, matching MuseScore's own export
  of empty staff bars) instead of a `<forward>` when a staff's sole voice is
  all-spacer *and* hosts directions (`anchor_spacers_as_rests`).
- **Pedal events placed after their note, not at it.** `\sustainOn`/`\sustainOff`
  are LilyPond post-events that occur at the **onset** of the note they follow,
  but the offset was read from `elapsed_in_measure` *after* that note's duration
  — so `s2\sustainOn` landed at beat 3 (offset 1/2) instead of the downbeat, and
  a release at a bar's end boundary spilled into the next measure. Added
  `WalkState.last_element_onset` (set in `push_voice_element`); the pedal handler
  now reads it. Pedal-downs now sit on the correct beat, aligned to staff-2 note
  onsets.

Net effect: `mscore3` re-export keeps **82/92** pedal starts on staff 2 (was the
majority re-anchored to staff 1), and a PDF render shows every **Ped./✱** below
the bass staff. 862 Rust tests (+ `pedal_events_attach_at_note_onset`,
`empty_lower_staff_bar_gets_anchor_rest_for_pedal`); fmt + clippy clean. The
remaining starts/stops land at mid-bar ticks with no LH note onset (pedal lane
rhythm ≠ LH rhythm) and would need offset-snapping to the nearest LH note — a
possible follow-up; they already render below the staff.

## 2026-06-14 — Fix multi-staff piano LY→XML: staves, dynamics, direction placement ✅

Branch `fix-piano-multistaff-directions`. Piano scores with `\new Dynamics`
lanes interleaved among the staves (repeats.ly, chopin_n.ly) converted to
MusicXML badly; pedal/dynamics also rendered in the wrong vertical lane. Five
coordinated fixes:

- **Direction staff/placement (IR + emit).** Added `Direction.staff` (`0` =
  unset, `#[serde(default)]`) and emit `<staff>` in `ir_to_mxml`'s
  `build_direction`. Content-based assignment for piano grand staves
  (`merge::assign_piano_direction_staff`): **pedal → bottom staff, below;
  dynamics/hairpins → staff 1, below (between staves)**; text/tempo stay above.
- **Dynamics treated as staves (the big one).** `merge_piano_staff_parts`
  counted every part in a `PianoStaff` as a staff, so the 3 `\new Dynamics`
  lanes in repeats.ly became staves (→ 3 `<staves>`, 342 measures, scrambled
  RH/LH). It now partitions real Staff parts from Dynamics-only parts
  (`part_is_dynamics_only`), uses the first **staff** as the base, scopes the
  bar-58 `unify_staff_time_signatures` to real staves, and folds the Dynamics
  lanes in as directions. repeats.ly → **2 staves, 164 measures**.
- **Dynamics dumped at the end.** `<< \silent \dynamics >>` (two spacer-only
  variable branches) concatenated instead of overlaying, so dynamics landed
  *after* the music and clamped to the last measure. `walk_parallel_music_staves`
  now wraps variable-ref children in the simultaneous-merge so spacer-only
  siblings overlay; `merge_spacer_by_duration` clamps overflow to the nearest
  in-range measure. repeats.ly dynamics now span **134 measures**, not 1.
- **Voice-number collisions.** `merge_piano_staff_parts` renumbered voices
  per-measure (`max+1`), so a voice drifted between numbers and collided across
  staves (chopin: 9 same-measure collisions, 127 cross-measure). Now each extra
  staff gets one global voice-number offset, applied consistently → 0 collisions.

repeats.ly / pedal.ly / chopin_n.ly all now emit 2 correctly-split staves with
directions distributed and pedal below the lower staff. 859 Rust tests
(+ `repeats_has_two_staves`, `repeats_dynamics_are_distributed_not_dumped`,
`repeats_pedal_below_lower_staff`, `direction_emits_staff_and_placement`);
fmt + clippy clean. chopin-specific grace/tuplet issues remain a follow-up.

- **Directions emitted inline per staff.** Measure-level directions were
  emitted in an upfront/trailing block (after a generic `<backup>`), so a pedal
  with `<staff>2</staff>` was anchored ambiguously and rendered above the lower
  staff's voice instead of below the staff. `ir_to_mxml/part.rs` now interleaves
  each direction **inline within its target staff's first voice** at the right
  beat (matching how notation software exports them, e.g. 33a-Spanners). All
  pedal directions now land in the lower-staff stream (after the staff-2
  `<backup>`) with `<staff>2</staff>` placement="below" → rendered below both
  staves. 860 tests (+ `repeats_pedal_emitted_in_lower_staff_stream`).

## 2026-06-13 — Epic E: ABC notation adapter ✅

Branch `epic-e-abc-adapter`. Adds ABC as a fourth interchange format (the first
new format of v1.0.0).

### EET1 — ABC → IR (`src/adapters/abc_to_ir.rs`)
Hand-written parser (`ToMusicAdapter` + `ToIrAdapter` via lower):
- Headers `X/T/C/M/L/K` (+ others → metadata.extra); `M:C`/`C|` symbols;
  `L:` unit length with the meter-derived default (1/16 if meter < 0.75 else
  1/8).
- Notes: explicit accidentals (`^ _ = ^^ __`), the standard octave convention
  (uppercase = MIDI 60–71, lowercase an octave up, `,`/`'` shift), fractional
  durations (`N`, `/`, `//`, `/N`, `N/M`).
- Key `K:` tonic + mode → fifths, including the church modes
  (dorian/mixolydian/…); rests, bar lines + repeats (`|: :| :: || |]`),
  chords `[...]`, ties `-`. Chord symbols `"..."`, decorations `!..!`, grace
  `{..}` and inline `[K:..]` fields are skipped gracefully.

### EET2 — IR → ABC (`src/adapters/ir_to_abc.rs`)
Emits header + body at `L:1/8`; renders pitch/duration/key/meter/barline/
chord/tie. v1 limitation: pitches keep only their explicit accidentals (the
key signature isn't used to re-spell), which is self-consistent on round-trip.

### EET3 — CLI + tests
- `.abc` wired into `convert` (input and `-f abc` output; lift on the way out).
- 3 `.abc` fixtures; `tests/abc_roundtrip.rs` (6 cases incl. parse→emit→parse
  pitch/duration identity, repeat preservation, ABC→LilyPond, ABC→MusicXML) +
  2 CLI tests. 17 unit + 8 integration tests total.

Verified by an adversarial 5-area workflow (pitch/octave, durations,
keys/meters, barlines, robustness over real folk-tune corpora). 855 Rust tests
green; fmt + clippy clean.

## 2026-06-13 — Epic F: dataset utilities (EFT1/EFT2/EFT4) ✅

Branch `epic-f-datasets-metrics` (continued). Adds the dataset/ML-pipeline
layer on top of the representations and metrics.

### New `Score.to_music_document()` binding
Exposes `lift_to_music` to Python so MusicXML/MXL/MIDI scores can be converted
to the Layer-1 Music tree the representations consume (`.pyi` updated).

### EFT1 — dataset classes (`src/lytk/datasets/`)
- `load_document(path)` loads `.ly`/`.ily` (direct) and `.xml`/`.musicxml`/
  `.mxl`/`.mid`/`.midi` (parse + lift) as a `MusicDocument`.
- `Dataset` base: `__len__`/`__getitem__` plus representation converters
  (`to_note_arrays`/`to_event_sequences`/`to_pianorolls`/`to_representation`),
  `metrics()`, `split()`, and lazy `to_pytorch_dataset()` /
  `to_tensorflow_dataset()` adapters (optional-dep imports).
- `FolderDataset` (recursive lazy file discovery) and `Subset`.
- The remote dataset (JSB Chorales downloader) is deferred — it needs network
  access and isn't sandbox-testable.

### EFT2 — splits + caching
- `Dataset.split(ratios, seed)`: deterministic, disjoint, remainder-safe
  partitioning into `Subset`s.
- `FolderDataset(cache_dir=...)`: on-disk `.npy` cache of converted
  representations, keyed by source file + representation + params.

### EFT4 — tests
`tests/test_datasets.py` (14 cases): loaders for every format, folder
discovery, representation conversion, metrics, splits (coverage/determinism),
and caching round-trip.

Epic F is complete bar the optional remote dataset. 830 Rust + 73 Python tests
green; fmt + clippy clean.

## 2026-06-13 — Epic F: objective metrics (EFT3) 🟡

Branch `epic-f-datasets-metrics`. Begins Epic F (datasets & metrics) with the
objective-metric core.

### EFT3 — objective metrics
`src/representations/metrics.rs` (modeled on `muspy.metrics`, operating on a
`NoteArray`):
- `n_pitches_used`, `n_pitch_classes_used`, `pitch_range`,
  `pitch_class_histogram` (normalised 12-bin), `pitch_entropy`,
  `pitch_class_entropy`, `polyphony`, `polyphony_rate`, `empty_beat_rate`,
  `pitch_in_scale_rate`, `scale_consistency`, `groove_consistency`. "No notes"
  cases return `NaN`, matching muspy.
- PyO3 `compute_metrics(doc, resolution, measure_resolution) -> dict` + `.pyi`
  stub + `__init__` re-export.
- 7 Rust unit tests (hand-computed values + empty/NaN edges) + 3 pytest cases.
- **Adversarially verified against muspy** via a 6-group workflow (counts,
  histogram/entropy, polyphony, empty-beat, scale, groove): **0 discrepancies**
  — including the critical `np.roll` scale-mask direction (`rem_euclid` matches
  NumPy) and the inclusive beat-marking / `+1` measure-count edges.

830 Rust + 59 Python tests green; fmt + clippy clean. Next: EFT1/EFT2
(dataset classes, splits, caching, torch/tf adapters).

## 2026-06-13 — Epic D complete: numpy/PyO3 interop (EDT4) ✅

Branch `epic-d-ml-representations` (continued). Exposes the three
representations to Python as numpy arrays, completing Epic D.

### EDT4 — numpy interop
- Added the `numpy` crate (0.22.1) — verified it compiles and runs under the
  project's `pyo3` `abi3-py39` + `extension-module` setup (a `maturin develop`
  abi3 wheel builds and imports cleanly).
- New PyO3 functions in `lib.rs` (operating on `MusicDocument`):
  `to_note_array`/`from_note_array` (`(N, 4)` int32 — onset, duration, pitch,
  velocity), `to_event_sequence`/`from_event_sequence` (1-D int64 codes),
  `to_piano_roll`/`from_piano_roll` (`(T, 128)` uint8). Decoders rebuild a
  `MusicDocument`; bad shapes raise `ValueError`.
- `.pyi` stubs (with `numpy.typing` annotations), `lytk.__init__` re-exports,
  and `numpy>=1.21` added as a Python runtime dependency.
- Verified end-to-end: `maturin develop --release` + `tests/test_representations.py`
  (12 pytest cases — shapes, dtypes, values, round-trips, error cases). Full
  Python suite now 56 tests; Rust 823; fmt + clippy clean.

**Epic D is complete** (EDT1–EDT5). All three muspy-style representations
(note-array, event-sequence, piano-roll) round-trip through the Music tree and
are available in both Rust and Python. Remaining v1.0.0 work: Epic E (ABC),
Epic F (datasets/metrics), Epic G (distribution).

## 2026-06-13 — Epic D: piano-roll representation (EDT3) 🟡

Branch `epic-d-ml-representations` (continued). Completes the three core
representations (EDT5 round-trip coverage now done); only EDT4 (numpy/PyO3
interop) remains in Epic D.

### EDT3 — piano-roll representation
`src/representations/piano_roll.rs`:
- `PianoRoll { resolution, num_steps, encode_velocity, data }` — a dense,
  row-major `T × 128` matrix (`data[t*128 + pitch]`), velocity-valued or
  binary. `cell(t, pitch)` and `shape()` accessors.
- `to_piano_roll(&NoteArray, encode_velocity)` fills `[onset, onset+duration)`
  of each pitch column; `from_piano_roll` reconstructs notes from contiguous
  nonzero runs per column (velocity read at the run start).
- Distinct-pitch and gap-separated material round-trips exactly; adjacent
  same-pitch notes merge into one held note (the classic piano-roll
  limitation — documented and tested).
- 8 tests: single note, chord columns, distinct-pitch round-trip, gap
  round-trip, repeated-pitch merge, binary mode, end-of-roll note, empty.

All three representations compose through the note-array (shared resolution).
823 Rust tests green; fmt + clippy clean. Next: EDT4 (numpy/PyO3 + `.pyi`).

## 2026-06-13 — Epic D: event-sequence representation (EDT2) 🟡

Branch `epic-d-ml-representations` (continued). Adds the event-based
representation on top of EDT1's note-array.

### EDT2 — event-based representation
`src/representations/event_sequence.rs`:
- `EventSequence { codes, resolution, max_time_shift, velocity_bins,
  encode_velocity }` and `EventOptions`. Performance-RNN-style vocabulary
  (muspy-compatible), documented in the module header and via `event_name` /
  `vocab_size`: note-on `0..128`, note-off `128..256`, time-shift
  `256..256+S` (advance `code−256+1` steps, large shifts decomposed),
  velocity-set `256+S..256+S+V` (quantised into `V` bins).
- `to_event_sequence(&NoteArray, &EventOptions)` builds timed `(time, code)`
  events, stable-sorts, and inserts decomposed time-shifts; velocity-set is
  emitted only on change.
- `from_event_sequence(&EventSequence) -> NoteArray` decodes with FIFO
  note-off matching and a running velocity. Onset/duration/pitch round-trip
  exactly; velocity is banded (exact at bin centres).
- 7 tests: exact event codes, time-shift decomposition, melody/chord
  round-trips, velocity banding, `encode_velocity=false`, vocab size.

Encoding/decoding compose through the note-array, so the resolution is shared.
815 Rust tests green; fmt + clippy clean. Next: EDT3 (piano-roll).

## 2026-06-13 — Epic D start: note-array representation (EDT1) 🟡

Branch `epic-d-ml-representations`. Begins the ML half of v1.0.0 — the
muspy-style symbolic representations. New `src/representations/` module.

### EDT1 — note-based representation
`src/representations/note_array.rs`:
- `NoteRow { onset, duration, pitch, velocity }` and `NoteArray { resolution,
  notes }` (serde-serialisable). Time is in integer steps; `resolution` =
  steps per quarter note (default 480).
- `to_note_array(doc, resolution)` walks the Layer-1 Music tree to absolute
  time: unfolds repeats (with `\alternative` voltas), resolves simultaneity,
  grace notes (zero-time, share the next onset) and tuplets (via
  `actual_duration`), and maps a running dynamic to velocity through the shared
  `dynamics_velocity` map. Rows sorted by `(onset, pitch, duration, velocity)`.
- `from_note_array(arr)` reconstructs a Music tree (each note a parallel
  `Skip(onset)·Note(duration)` branch) that re-flattens to the same rows —
  onset/duration/pitch exact; velocity exact at the default, banded otherwise.
- 11 unit tests: melody, rests/skips, chords, overlapping voices, tuplets,
  grace, dynamics→velocity, repeat + alternative unfolding, round-trip.

Per the roadmap decision, representations go through the Music tree (not the
measure-based Score), reusing exact `Frac` durations. 808 Rust tests green.

Next: EDT2 (event sequence) and EDT3 (piano-roll) share this flattening;
EDT4 adds numpy/PyO3 interop.

## 2026-06-13 — Close the 5 remaining open conversion bugs ✅

Branch `fix-pedal-bar58-and-bugs` (continued). Cleared every confirmed-but-open
bug left from the multi-agent hunt; 797 Rust tests green (up from 790).

- **Score-path `\repeat volta N` count** — was hardcoded to 2. Added
  `Barline.repeat_times`, threaded it through the LilyPond parser
  (`modifiers.rs`), MusicXML import/export (`<repeat times>`), the Score-path
  emitter (`emit.rs`), and the lift pass so the count survives all three
  round-trips. Un-ignored `ly_to_ly_score_preserves_volta`.
- **Two-note tremolo emission** — a `<tremolo type="start/stop">` pair was
  dropped in Score→LY. `emit_voice_elements` now does a 2-element lookahead and
  emits `\repeat tremolo N { a b }` (unit `1/2^(marks+2)`, `N = span/(2·unit)`).
- **Multi-staff (PianoStaff) lyrics** — the lyric variable was emitted but never
  referenced. The lyric-bearing staff's voice is now named and `\lyricsto`-ed;
  lyric extraction is staff-scoped (`lyric_staff` + a staff filter on
  `extract_lyrics`) so the syllable stream aligns with that staff only.
- **Cross-staff voice duplication** — `voice_matches_staff` matched every staff a
  voice touched, so a voice spanning two staves was emitted (and played) twice.
  A voice is now assigned to a single *primary* staff (its first staff-bearing
  element); notes are preserved, not duplicated.
- **MIDI conductor track** — tempo/time/key were read from `parts()[0]` only, so
  a meter declared solely on an inner staff (e.g. example.ly's Corno) was lost,
  including the per-measure tick advance. `build_conductor_track` now aggregates
  per measure index across all parts and advances by the unified meter.

Docs: README known-limitations trimmed to the one genuine remaining gap
(cross-staff `\change Staff` beaming); test counts refreshed.

## 2026-06-12 — Fix pedal.ly bar-58 drift + 13 conversion bugs (multi-agent bug hunt) ✅

Branch `fix-pedal-bar58-and-bugs` (stacks on the clef fix). Fixes the deferred
PianoStaff time-signature bug plus 13 further bugs found by an adversarially
verified multi-agent audit. All 790 Rust tests green; the LY→LY fidelity
scoreboard moved from 33/35 to **35/35** (baselines bumped in
`tests/fidelity.rs`).

### The pedal.ly bar-58 fix (4 coordinated changes)
1. **PianoStaff time-signature unification** — `\time` goes to the score-shared
   Timing context, so a staff omitting changes its sibling declares (pedal.ly's
   LH omits the RH's `\time 4/4` sections) must be re-barred. New
   `unify_staff_time_signatures` (`ly_to_ir/merge.rs`) merges the per-staff
   timelines by absolute position and re-bars divergent staves; called from
   `merge_piano_staff_parts` (`walk.rs`).
2. **Position-aware resplit** — `resplit_measures_with_time_changes` now tracks
   each element's absolute time, so sparse voices (a 2nd voice present only in
   some measures) land in the right bars instead of being packed from zero
   (the flaw that sank the earlier `resplit_measures_to_match` attempt).
3. **Duration `*N/M` carry-forward** — LilyPond remembers the scale factor as
   part of the duration (`a32*8/7( e a …` ⇒ all seven notes are 1/28), the
   parser didn't, shifting the RH 1/56 per septuplet run.
4. **`\override` swallowed a bar** — a 3+-component property path
   (`Staff.NoteCollision.merge-differently-dotted`) parses as one
   `assignment_lhs` node that `consume_override` didn't recognise; its
   skip-unknown fallback then consumed the following `<< {} \\ {} >>` bar
   (3×3/8 lost in pedal.ly's LH). Also: phantom extra measure from
   `<< { v1 } \context Voice = "1" { v2 } >>` (first branch's trailing bar was
   left pending during the sibling merge).

Tests: `pedal_piano_staff_bars_aligned_across_staves`,
`piano_staff_time_unification_minimal` (`tests/semantic_roundtrip.rs`),
`test_duration_scale_carries_forward`, `test_override_with_property_path_…`,
`test_parallel_context_voice_no_phantom_measure` (`ly_to_ir/tests.rs`).

### Other bugs fixed (finder → verifier confirmed, then TDD'd)
- **`\addlyrics` inside `<< … >>` injected phantom notes** — the lyric block
  fell through to the music walker, so syllables that are valid pitch names
  became notes (example.ly +9, example2.ly +9 — the whole fidelity-gate gap).
  `walk_parallel_music_staves` now mirrors the score-level handler.
- **`\breve`/`\longa`/`\maxima` silently mis-read** as the previous duration
  (`consume_duration` only looked for integers).
- **Compound `\time 3+2/8` mis-parsed as 2/8** — the grammar splits it into
  `3` `+` `2/8`; the handler now collects the leading addends.
- **`\key` tonic hardcoded in Nederlands** — `\key fis \major` under
  `\language "english"` doesn't compile; `key_to_ly` now spells the tonic via
  the emitted language's `pitch_name`.
- **Header strings unescaped** — embedded `"` produced uncompilable LilyPond;
  new `escape_ly_string` applied to header fields and instrument names.
- **Duplicate part variables for ids `P0`/`P1`** — `index_to_alpha(0) ==
  index_to_alpha(1)`, so one part shadowed the other; `part_var_name` now uses
  a bijective suffix (`n + 1`).
- **MusicXML mid-measure directions snapped to beat 1 on round-trip** — import
  stored the position only in `offset` (divisions) but export reads
  `offset_frac`; import now sets both.
- **Grace notes lost their flag in Music→Score lowering** (`lower/walk.rs`
  TODO) — they came out as regular zero-advance notes; `TimedEvent::Note/Chord`
  now carry `grace: Option<bool>` through to `is_grace`/`grace_slash`.
- **MIDI import serialized chords** — simultaneous note-ons with equal
  start/end now import as `Chord`s instead of sequential notes.
- **MIDI notes crossing a barline lost their remainder** — now split at the
  boundary and tied (`tie start` / `tie stop`), re-queued into the next bar.

### Known issues (confirmed, not yet fixed)
- Score-path `ir_to_ly` hardcodes `\repeat volta 2` (Music path preserves N);
  `ly_to_ly_score_preserves_volta` still `#[ignore]`d.
- Two-note tremolo from MusicXML is dropped in Score→LY emission.
- PianoStaff lyrics: variable emitted but never referenced in `\score`.
- A cross-staff voice is duplicated into every staff it touches (ir_to_ly).
- MIDI export conductor track reads tempo/time/key from part[0] only.

### Docs
README refreshed (chordmode/figuremode are implemented; MIDI is always
compiled, not optional; real test counts 450 unit + 340 integration + 44
Python; added Known-limitations section); CLAUDE.md test counts fixed.

## 2026-06-08 — Fix missing per-staff clefs in piano scores (pedal.ly) ✅

Branch `fix-pedal-clefs` (stacks on the example-bars fix).

### Clef fix
LilyPond leaves the default clef (treble) implicit, so a piano part whose upper
staff has no explicit `\clef` recorded no clef for it. MusicXML then emitted a
single clef with no `number=`, and the upper staff rendered with the wrong clef
(pedal.ly showed only a bass clef). New `ensure_staff_clefs` post-process fills
the first measure with a treble clef for any staff of a multi-staff part that
has none — so each staff gets a numbered `<clef>` (pedal.ly now emits
`<clef number="1">` G2 treble + `<clef number="2">` F4 bass). Defaulting a
missing staff to treble is also correct when that staff changes clef later.

Test: `pedal_piano_clefs_per_staff` (`tests/semantic_roundtrip.rs`).

### Known issue — bar-58 left-hand shift (diagnosed, not yet fixed)
pedal.ly's two hands are separate variables (`voicea` RH / `voiceb` LH). The RH
declares `\time 4/4` sections; the LH never does. In a PianoStaff the time
signature is *shared*, but each staff is pre-parsed independently, so the LH
content is barred without the 4/4 changes and under-fills those bars — by the
4/4 section the LH drifts relative to the RH (e.g. IR m70 is a 4/4 bar with the
RH full but the LH only half-full). `merge_piano_staff_parts` merges staves by
measure index, which assumes aligned boundaries. A proper fix needs a
time-signature-unification + re-bar pass across PianoStaff staves; the existing
`resplit_measures_to_match` (built for spacer/note alignment) does not handle
the multi-voice piano case cleanly (an attempt made other bars worse), so it is
deferred to a dedicated change.

## 2026-06-08 — Fix bar-splitting drift from grace notes (example.ly / example2.ly) ✅

Branch `fix-example-bars` (stacks on Epic C). Fixes wrong durations / bar
splitting when converting example.ly (and example2.ly) to MusicXML.

### Root cause
`voice_element_duration` (`ly_to_ir/merge.rs`) returned a grace note's *notated*
duration. In example.ly only the Corno staff carries `\time 4/4`; the other 5
staves have none, so `synchronize_time_signatures` resplits them to match the
Corno's measure durations. The resplit's position accounting counted each
appoggiatura/grace eighth toward the bar boundary, so every barline after a
grace note drifted — measures that should be 4/4 came out as 7/8, the next as
1/4, etc., and parts disagreed on bar lengths.

### Fix
`voice_element_duration` now returns 0 for grace notes (they never consume
measure time). This is the single source of truth for all measure-position math
(`measure_voice_duration`, the synchronize/resplit split loop, timeline
building), so all bar splitting is now grace-aware.

### Result
- example.ly → MusicXML: all 6 parts, **0 irregular bars** (full 4/4 bars + the
  legitimate cadenza half-bar). Was: 2–11 broken bars per part.
- example2.ly movement 1: all parts consistent. Movement 2 (multi-meter:
  4/4, 3/2, 6/8) consistent on all interior bars.

### Tests (`tests/semantic_roundtrip.rs`, `tests/common/mod.rs`)
- `example_bars_consistent_across_parts`, `example2_bars_consistent_across_parts`:
  assert every part agrees on per-measure durations (the bug made them disagree).
- Known remaining edge (separate, lower priority): a movement's *final* bar can
  carry ragged trailing content the resplit dumps into the last measure
  (excluded from the example2 interior-bar check).

## 2026-06-08 — Epic C: semantic round-trip quality gate ✅

Branch `epic-c-semantic`. Upgrades the test bar from "parses / non-empty" to
*semantic fidelity*, and the scoreboard immediately exposed (and let us fix) two
real CLI LY→LY bugs.

### ECT1 — signature/comparator library (`tests/common/mod.rs`)
- `signature(&Score) -> Sig` summarizes musical content: pitches (MIDI),
  note/rest counts, dynamics, articulations, lyrics, harmonies, figured bass,
  ties/slurs, time signatures. `pitch_multiset`/`sorted_dynamics` give
  order-independent comparisons. Shared across test binaries via `mod common`.

### ECT2 — per-fixture semantic round-trip suite (`tests/semantic_roundtrip.rs`)
- Macro-generated named tests assert pitch-multiset + note-count survive
  LY↔IR↔LY (Music path) and XML↔IR↔XML for representative fixtures (pedal,
  chopin, repeats, relative-repeat, …; XML pitches/chords/lyrics/repeats/grace),
  plus a dynamics-preservation check.

### ECT3 — fidelity scoreboard gate (`tests/fidelity.rs`)
- Round-trips every fixture and tallies note-count + pitch-multiset preservation
  per direction, gating on a committed baseline (non-decreasing fidelity). Runs
  in CI via `cargo test`. Current: **XML 152/152**, **LY 33/35**.

### Bugs surfaced & fixed by the scoreboard
- **Top-level music not parsed:** the Music emitter emits `<< \new Staff … >>`
  with no `\score` wrapper, but `walk_program` only handled
  `escaped_word`/`expression_block`/`assignment_lhs` — so re-parsing emitted LY
  yielded **0 notes** for complex fixtures (chopin, example, pedal…). Added
  top-level `parallel_music` and `named_context` handlers (LY note-count
  fidelity 22 → 33/35).
- **Relative octaves without `\relative`:** the Music-path emitter emitted
  relative octave marks (when the source used `\relative`) but no `\relative {`
  wrapper, so octaves shifted on re-parse. Now always emits absolute pitches —
  unambiguous and round-tripping (LY pitch fidelity 13 → 33/35).

### Tests
- `tests/common/mod.rs`, `tests/fidelity.rs` (new); `tests/semantic_roundtrip.rs`
  extended. Full suite green, clippy clean, audit 0 errors / 0 panics.

## 2026-06-08 — Epic B complete: chordmode, figuremode robustness, partial ✅

Branch `epic-b-finish`. Finishes the remaining Epic B conversion-fidelity tasks
(EBT3/EBT4/EBT6), all TDD. Epic B (EBT1–EBT6) is now done.

### EBT3: `\chordmode` import → Harmony IR
- New `src/adapters/ly_to_ir/chord_mode.rs`: text-based chordmode tokenizer
  (robust against `maj7`/`m7.5-` qualities, `/e` and `/+e` bass, and `_"..."`
  markup). Uses `parse_pitch_name` so roots are language-aware (e.g. German `h`).
  `ly_quality_to_kind` mirrors `harmony_kind_to_ly` so chords round-trip.
- Harmonies are collected from `\new ChordNames \chordmode { … }` (inline or via
  a chordmode variable) into `state.pending_harmonies` and distributed onto the
  first content-bearing part's measures at score-block assembly (mirrors the
  lyrics pending mechanism, so ChordNames-before-Staff ordering works).
- Chordmode variables are still also parsed as music, so `\context Voice \var`
  keeps rendering notes (no regression to `chord-names-bass.ly`).

### EBT4: `\figuremode` robustness
- `parse_figure_chord` now consumes a *run* of accidental punctuation:
  `!`→natural, `++`→double-sharp, `--`→double-flat (was: single `+`/`-` only,
  dropping naturals and doubles). `figure_to_ly` emits `!`/`++`/`--` so they
  round-trip; MusicXML passes the suffix string through.

### EBT6: `\partial` in multi-movement contexts
- `state.metadata.partial_duration` was shared across `\score` blocks, so a
  pickup in movement 1 leaked into later movements (wrongly marking their first
  measure implicit). Reset it at the start of each score block.

### Tests
- `tests/semantic_roundtrip.rs`: chordmode parse + round-trip, `\partial`
  no-leak across movements. `ly_to_ir/tests.rs`: figuremode natural + double
  accidentals. Full suite green (437 lib + others), clippy clean, audit still
  0 errors / 0 panics over 195 fixtures.

## 2026-06-07 — Epic B2: repeat/volta round-trip (Music path) ✅

Branch `epic-b2-repeats`. Makes `\repeat volta` + `\alternative` survive LY→LY
(the CLI path), which previously flattened repeats to linear notes.

### Parser fix (`ly_to_ir/modifiers.rs`)
- `consume_repeat` now flushes the repeat body (`state.flush_measure()`) before
  parsing `\alternative`. Without this, the body's final measure shared an index
  with the first alternative, so `consume_alternatives` mismarked the body as
  alternative 1 and the `RepeatForward` marker was overwritten/lost. This also
  improves the Score (and thus MusicXML/MIDI) representation of voltas.

### Lift reconstruction (`ir/lift.rs`)
- `lift_measures` now detects repeat groups (forward-repeat barline → body →
  volta endings → optional backward-repeat close) and rebuilds `Music::Repeat`
  with a `Sequential` body and `Sequential` alternatives. The Music-path emitter
  already renders `Music::Repeat` as `\repeat volta N { … } \alternative { … }`.
- Structural repeat/volta barlines are suppressed inside the group (represented
  by the wrapper); the redundant empty backward-repeat close measure is dropped.
- Per-measure emission refactored into `lift_one_measure`; helpers added
  (`is_repeat_forward/backward`, `is_alternative_start/stop`, `is_empty_repeat_close`).

### Scope
- Single-staff parts (`lift_measures`). Multi-staff (PianoStaff) repeats and the
  Score-path `ir_to_ly` emitter are still TODO.

### Tests (`tests/semantic_roundtrip.rs`)
- volta round-trip (body + both alternatives), simple no-alternative repeat, and
  a double round-trip stability check (re-parse the emitted LY). 9 passing, 1
  ignored (Score-path volta).

### Also: fixed the long-standing double-flat transpose bug (`ir/pitch.rs`)
- `Pitch::transposed` computed the target octave with truncating division
  (`target_midi / 12`), which disagrees with the floored chroma (`rem_euclid`)
  for negative MIDI numbers. A low pitch (e.g. a double-flat transposed far down)
  therefore round-tripped to the wrong octave. Switched to `div_euclid`.
- This clears the 2 previously-failing proptests (`pitch_transpose_roundtrip`,
  `pitch_transpose_adds_semitones`) that were flaking CI nondeterministically
  (random proptest seeds). **Full suite is now green** (20/20 proptests, verified
  at 2000 cases × 3 runs).

## 2026-06-07 — Epic B/C: conversion fidelity (lyrics, MIDI dynamics) + semantic test harness 🟡

Branch `epic-b-c-fidelity`. Test-first: new `tests/semantic_roundtrip.rs` asserts
*meaningful content* survives conversion (not just "parses").

### EBT1: Lyrics in IR→LY ✅
- **Music path** (`ir_to_ly/music_emit.rs`): previously `Annotation::Lyric` was a no-op, so
  LY→LY (the CLI path) silently dropped all lyrics. Now collects lyric syllables per verse
  from a Staff/Voice context's notes and emits `\addlyrics { ... }` after the block.
- **Parser** (`ly_to_ir/walk.rs`): added `\addlyrics` support in `walk_score_block` —
  parses the following lyric block and attaches syllables to the preceding music's part.
- Score path already emitted lyrics (`\new Lyrics \lyricsto`); verified for XML→LY.

### EBT5: MIDI velocity ↔ dynamics ✅
- New shared `adapters/dynamics_velocity.rs`: `dynamic_to_velocity` / `velocity_to_dynamic`.
- **Export** (`ir_to_midi.rs`): running velocity updated by note/chord dynamic marks
  (was a fixed 80 for everything).
- **Import** (`midi_to_ir.rs`): note velocity quantized to the nearest dynamic; a mark is
  attached only when the band changes (avoids spamming every note).

### EBT2: Repeat/volta round-trip — diagnosed, deferred 🟡
- Root cause: the LY parser stores `\repeat volta` as Score repeat-barlines + volta endings
  and never builds `Music::Repeat`; `lift.rs` doesn't reconstruct it, so LY→LY flattens
  repeats to linear notes (no data loss, structure lost). Needs a lift→`Music::Repeat`
  pass. Characterizing tests left `#[ignore]` in `semantic_roundtrip.rs`.

### Tests
- `tests/semantic_roundtrip.rs`: 6 passing (lyrics ×5, MIDI dynamics ×1), 2 ignored (volta).
- Full suite green except the 2 pre-existing double-flat transpose proptest failures.

## 2026-06-07 — v1.0.0 roadmap review + Epic A (Stabilization & Baseline) ✅

**Goal:** Review references (added `muspy` as a key symbolic-music-for-ML reference) and the
roadmap, assess current conversion status, and define a reviewed roadmap to a v1.0.0 release.

### Reviewed roadmap
- Rewrote the forward-looking sections of [`docs/roadmap.md`](roadmap.md) into a **v1.0.0
  milestone** themed "LilyPond as a first-class symbolic-music-for-ML format", with 7 epics:
  A Stabilization, B Conversion Fidelity, C Semantic Round-Trip Test Bar, D ML
  Representations (note-array/event/piano-roll + numpy), E ABC adapter, F Datasets & Metrics,
  G Distribution. MEI/Humdrum, audio synthesis, and music21-parity MIR deferred past v1.0.0.

### Epic A: Stabilization & Baseline
- **EAT1:** Resolved unresolved git merge conflicts blocking the build —
  `ly_to_ir/consume.rs` (kept the bow/harmonic articulation superset: `\upbow \downbow
  \flageolet \open \snappizzicato`) and `ly_to_ir/merge.rs` (whitespace only).
- **EAT2:** Captured baseline — 435 lib + 19 CLI + 208 fixture_regression pass; 18/20 proptest
  (2 known double-flat transpose failures, pre-existing).
- **EAT3:** Added `muspy/` to the `CLAUDE.md` reference table.
- **EAT4:** Fixed stale "feature-gated behind `midi`" wording in `docs/import-export.md`
  (the gate was removed in Epic 4).
- **EAT5:** New `tests/fixture_audit.rs` — runs the full convert matrix (read ly/xml/mxl/midi
  → IR, then IR → ly/xml/midi) over all 195 fixtures, catching panics and tallying errors per
  stage. Baseline: **0 errors, 0 panics**. This is the running scoreboard; semantic fidelity
  loss (lyrics, dynamics, repeats) is *not* yet caught here and is the focus of Epic C.

### Next up
- **Epic B + C:** conversion fidelity (lyrics IR→LY, repeat/volta round-trip, `\chordmode`
  import, MIDI velocity↔dynamics) developed test-first against a new semantic round-trip bar.

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
