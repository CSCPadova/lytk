# Changelog

## 2026-09-24 — One command line, in Python

There were two `lytk` commands: the Rust binary (13 subcommands) and the one
`pip install` put on PATH, a four-command Python subset. The Rust one is gone
and the Python one does everything it did. It's a Typer app now, in
`src/lytk/cli.py`.

- **Commands:** `convert`, `transpose` (`-s`, `--interval`, `--to-key`),
  `invert`, `retrograde`, `change-language`, `abs2rel`, `rel2abs`, `info`
  (`--json`), `positions`, `bundle`, `diff`, `batch` and `flatten`. They keep
  the Rust behaviour:
  - `-` for stdin/stdout, with `--from`/`--format`;
  - the first movement of a multi-`\score` file at the requested path;
  - LilyPond→LilyPond through the Music tree;
  - parallel folder conversion and batch jobs, where a failing file fails
    alone and the exit status is non-zero;
  - `diff` exits 1 when the scores differ.
- **Checked against the Rust binary** on 105 fixtures: `info` (text and JSON)
  is identical everywhere. `positions` differs only where there are grace
  notes. The Rust version added a grace note's written duration to its bar's
  length; grace notes take no time, so the Python version leaves them out.
- **Bindings added for it:** `from_lilypond_movements(path)`,
  `to_lilypond(..., relative=True/False)`, `to_mxl_bytes(score)`,
  `Score.lyricist` and `Part.midi_instrument`, with type stubs.
- **Messages:** a missing input now says `error: no such file: PATH`. Before,
  the text was `I/O error: No such file or directory (os error 2)`, with no
  path.
- **Removed:** `src/main.rs`, the `[[bin]]` target, `clap` and `anyhow`
  (`rayon` remains only as a dependency of `midly`). The 48 Rust CLI tests are
  now Python tests (`tests/test_cli.py`, 66 cases, in-process through Typer's
  `CliRunner` plus two through the installed command); `assert_cmd` and
  `predicates` are gone.
- **Docs:** `docs/cli.md` was rewritten as the reference for all 13 commands;
  it had covered 4 and described multi-movement output wrongly. Typer is a
  new runtime dependency.

998 Rust + 184 Python tests pass.

## 2026-09-24 — Positioned LilyPond reader, multi-staff repeats, `français`, MIT

The pre-release items: the bar-splitter refactor (Epic H), multi-staff repeat
output, the `français` pitch language, and relicensing. **1046 Rust + 141
Python tests, 0 failures**; clippy clean on 1.90 and 1.98.

### The LilyPond reader no longer builds measures while it walks

The old reader split bars as it went, using whatever time signature happened to
be active (a variable was pre-parsed at the default 4/4, before `\global` set
the real meter), then reconciled voices, staves and variables **by measure
index**, patching the results with resplit/sync passes. The new reader:

- places every note at its absolute onset in a voice *lane*, and every
  `\time`, `\key`, `\clef`, direction, barline, chord name, figure, `\partial`
  and `\cadenzaOn/Off` as an event at a position (`ly_to_ir/timeline.rs`);
- overlays simultaneous music by position: each branch of `<< … \\ … >>` or
  `<< {…} {…} >>`, each staff of a PianoStaff, and each parallel variable starts
  where its block starts. A run that would overlap music already in its lane
  moves to the next free lane;
- splices a variable (a timeline positioned from 0) in where it is used;
- cuts bars **once**, at score assembly, on a score-wide grid. This is
  LilyPond's model, where `Timing` lives in the Score: bar lines fall on the
  meter grid (anchored at the start, `\partial` and every `\time`), an explicit
  barline adds a boundary without moving the grid, a cadenza span is one free
  bar, and `|` is only a check.

`merge.rs` went from 1,796 to ~250 lines; `merge_voice_measure_streams`,
`merge_simultaneous_block`, the spacer merges, `resplit_*`,
`synchronize_time_signatures`, `unify_staff_time_signatures`,
`collapse_cadenza_runs` and `merge_leading_attribute_measures` are gone.

**Checked against 319 inputs** (the 35 `.ly` fixtures plus the 142-file MusicXML
corpus written out through both LilyPond writers), comparing structure and
(onset, duration, pitch) before and after, file by file. Every change was
reviewed; the ones that alter what a score *means* were all fixes:

- keys, clefs, tempo marks and line/page breaks written at a bar line landed on
  the bar **before** it (the old reader closed a full bar only when the next
  note arrived);
- a repeat starting after a full bar began one bar early, so that bar was
  repeated as well;
- in a piano score, music after a `<< … \\ … >>` bar lost its staff and slid
  into an extra bar; volta endings were doubled a bar apart (`repeats.ly`);
- `<< {…} {…} >>` branches of very different lengths were played one after the
  other (a length-ratio heuristic); `\partCombine`-style input doubled its bars;
- `\context Staff` inside a staff created a second, empty staff;
- a `\new Voice \var` body was dropped (`cue-clef-begin-of-score.ly` read 0
  notes);
- `\addlyrics` at top level (how the Music-path writer emits lyrics) was parsed
  as music: words like "a", "es", "f" became notes;
- chord names from `\context ChordNames` and figures in `\figures` variables
  were lost in some layouts;
- pedal marks from a `\new Dynamics` lane landed a bar off (`pedal.ly`);
- chopin's coda bar was over-full (23/16 against 15/8) followed by an empty bar.

Bars that don't fill their meter fell from 260 to 233 (30 → 17 files); the rest
are genuine: incomplete source bars, and `\set Score.measureLength`.

Speed is at parity: repeats.ly 19 ms (was 20), chopin 17 ms (18), pedal 13 ms
(12), the 556-bar six-part fixture 35 ms (30), release build. A first version
was up to 9× slower on the large file because every run scanned its whole lane
for overlaps; lanes never overlap, so only the run's own span needs checking.

Tests: 10 reader regressions (each fails on the old reader), the EHT5 bar
`piano_fixtures_staves_stay_in_step` (every staff the same length, every bar
filled alike, on chopin/pedal/repeats), timeline unit tests.

### Multi-staff repeats (and voices) through the Music path

`lift_to_music` had a separate routine for multi-staff parts that skipped the
repeat reconstruction and appended a staff's voices one after the other. So
LY→LY on any piano score lost `\repeat volta`/`\alternative` and doubled the
length of every two-voice bar. `lytk diff` and the fidelity board didn't notice,
because they compare note counts and pitches. The lift now runs the single-staff
routine per staff, and the LilyPond writer separates voices in a staff with
`\\` (without it both streams share one voice and re-read wrongly).

Fidelity board: LY onset 27 → 28, `**kern` 131/129 → 132/130. **XML→ABC pitch
and onset went down by one (124/122 → 123/121)**, the first lowered baseline:
`71d-ChordsFrets-Multistaff` only passed because the old lift turned a one-bar,
two-voice score into two bars, which the ABC writer (one stream per staff) then
kept whole. Documented in `tests/fidelity.rs`.

### Pitch languages

- **`français`** added (`do ré mi …`, `re` accepted for `ré`, `x` for
  double-sharp). The reader used to treat `\language "français"` as Dutch.
- Every language checked against LilyPond's `scm/define-note-names.scm`, both
  ways. **The writer produced names LilyPond rejects**: German/Finnish E-flat
  `ees` and A-flat `aes` (`es`, `as`), Swedish `eess`/`aess` (`ess`, `ass`),
  Norwegian B double-flat `heses` (`bess`). Catalan was using the Italian
  quarter-tone suffixes (`sb` for `qb`). Written names are now LilyPond's own;
  Spanish and Flemish gained quarter tones; all 910 LilyPond spellings,
  including English `c-sharp` and the Norwegian `is`/`es` forms, now read back.
  Dutch output is unchanged.

### Licence and packaging

- **Relicensed from GPL-2.0-only to MIT** (LICENSE, Cargo/pyproject metadata,
  CONTRIBUTING). All contributors and every runtime dependency (all
  MIT/Apache/BSD/Unlicense) allow it; no GPL code was copied (the pitch tables
  are LilyPond's note names, re-attributed in `language.rs`).
- `tests/fixtures/README.md` records the fixtures' own terms (Mutopia CC BY-SA,
  a CC BY-NC-ND score, LilyPond's GPL regression tests, …).
- **The sdist no longer ships `tests/`, `.github/` or `CLAUDE.md`**: it was
  about to publish those third-party scores to PyPI under an MIT label.
  Verified the trimmed sdist builds and imports in a clean venv.
- README rewritten as a project presentation; the reference list became a short
  acknowledgement. It had `lytk batch in_dir/ …`, which the pip-installed CLI
  doesn't have; the batch example now uses `lytk convert dir/ -o out/ -j N`.

### Upstream issues seen, not fixed here

The snapshot review also showed writer-side losses: the Music-path LilyPond
writer drops tuplets that have no `Music::Tuplet` wrapper (`23f`), writes no
`\time` when the MusicXML has none (`42b`), and got a pickup length wrong
(`\partial 4` for three beats, `21e`); `\grace \parenthesize` isn't read as a
grace (`tablature-grace-notes.ly`).

## 2026-09-24 — 0.1.0 preview release prep

The first public release ships as **0.1.0**, a preview before the remaining
pre-1.0 epics (P3–P9). **1025 Rust + 141 Python tests, 0 failures**; clippy clean
on both 1.90 and the 1.98 stable CI runs.

### CI was red on master since 2026-06-18

Five consecutive pushes failed, and nothing had been done about it. There were
three independent causes:

- **Clippy 1.98 `question_mark`** in `mxml_to_ir::parse_defaults`: the lint is new
  on stable, so the code only started failing when the toolchain moved on. It now
  uses `?`. Clippy stopped at this first lib error, so the bin and test targets had
  never been linted on 1.98; a full `--all-targets` run on 1.98 is now clean.
- **Two Windows-only CLI test failures** (`batch_jobs_runs`,
  `batch_partial_failure_exits_nonzero_and_reports`). The tests built the
  batch-job JSON with `format!` + `Path::display()`, and on Windows the
  backslashes became invalid JSON escapes. The code under test was fine; the
  tests now build the spec with `serde_json::json!`.
- **`frechet_music_distance` crashed on SciPy ≥ 1.18**, which removed the `disp`
  keyword from `scipy.linalg.sqrtm`. This one **affected users**, not just the
  tests (`TypeError` with any current SciPy). The call is now plain
  `sqrtm(A)`. Verified to give identical results on SciPy 1.10.1, 1.17.1 and
  1.18.1.

### Release

- Version `1.0.0` → **`0.1.0`** (Cargo + pyproject); classifier
  `5 - Production/Stable` → `4 - Beta`.
- `release.yml` has never run. Its x86_64 macOS wheel job targeted `macos-13`,
  which GitHub retired in December 2025, so the first tag would have failed.
  Moved to `macos-15-intel` (supported until Aug 2027).
- README: a preview note (the API may change before 1.0, e.g. notation names →
  enums), and the not-yet-implemented / known-limitations lists completed
  (tablature, percussion, MIDI reconstruction, custom key signatures, `français`,
  ABC/kern inner polyphony).
- Roadmap: 0.1.0 status, T12.1 marked done (the fidelity scoreboard already
  gates CI), and the stale "Humdrum deferred past v1" entries corrected.

### To tag (manual)

1. Create the `pypi` environment in the GitHub repo settings.
2. Register a pending Trusted Publisher on pypi.org: project `lytk`, repo
   `CSCPadova/lytk`, workflow `release.yml`, environment `pypi`. The name `lytk`
   is still free on PyPI.
3. Dry-run `release.yml` with `workflow_dispatch` (builds only), then install a
   wheel in a clean venv and smoke-test it.
4. Make the repo public, then push `v0.1.0`.

### Next

P5 (enum notation typing) first, because it is the only epic that breaks the
Python API and that is cheapest to do on 0.x. Then P3 → P4 → P6 → P8 → P9.

## 2026-09-20 — Conversion/augmentation audit + DLPack

A full audit of every conversion the library advertises (all 6×6 format pairs,
driven through the CLI with `lytk diff` as the oracle) and every augmentation,
plus the DLPack question. **1025 Rust + 141 Python tests, 0 failures**; clippy
clean.

### Conversion bugs found and fixed

The three defects below were all **silent** — nothing errored, note counts often
stayed plausible, and no test covered the path.

- **ABC dropped tuplets in both directions.** The writer flattened
  `Music::Tuplet` away and printed the *sounding* duration as a raw fraction
  (`c4/3`); the reader's token loop sent `(` to the catch-all `_ => i += 1`, so
  `(3cde` was read as four plain notes with **wrong durations** and no error.
  The writer now emits `(p:q:r` (one group per `p` notes, so a run never crosses
  a bar or a wrapped line) and prints the notated duration; the reader parses
  `(p`, `(p:q` and `(p:q:r` with the ABC default ratios and stamps the ratio onto
  both the `Music::Tuplet` wrapper and the note durations.
- **ABC emitted almost no bar lines.** The IR only stores *explicit* barlines
  (`||`, `|.`, repeats), so a 28-measure MusicXML score came out with **one** `|`
  and a MIDI import with **zero** — one giant ABC measure that any external ABC
  tool renders wrong. lytk's own readback hid it by re-barring from the meter.
  `emit_body` now tracks the running meter and closes each bar, wrapping the body
  every 4 bars (ABC convention).
- **Humdrum silently dropped grace notes.** `ir_to_humdrum` collected a measure
  into `BTreeMap<Frac, String>` keyed by onset; a grace note has zero duration,
  so it shared an onset with the note it decorates and `insert` **overwrote** it.
  `24a-GraceNotes` went 28 → 14 notes. The map is now `BTreeMap<Frac, Vec<String>>`
  and each grace gets its own kern data record (`.` in the other spines), bottom-
  aligned so the metrical event still shares one row across spines. Round-trips
  28 → 28.
- **ABC had no grace notes at all** — the writer ignored `Music::Grace`, the
  reader skipped `{…}`. Both sides implemented (`{ab}`, `{/a}`), graces carry no
  metrical time.

### Fidelity gate extended to the cross-format directions

The scoreboard measured `ABC → IR → ABC` over the four hand-written ABC fixtures
and **nothing at all for Humdrum** — a vacuous gate: those tunes are written in
the formats' own narrow idiom and never exercise what a real score throws at
those writers. Added two boards over the full 152-fixture MusicXML corpus:

| Direction | note-count | pitch | onset+dur |
|---|---|---|---|
| XML → IR → ABC → IR | 124/152 | 124/152 | 122/152 |
| XML → IR → KRN → IR | 131/152 | 129/152 | 130/152 |

(from 119/119/117 and 126/124/124 before the grace fixes), committed as
non-decreasing baselines. The remaining drift is understood, not mysterious:
un-notatable durations (a 31/8-bar note has no single spelling, so the Layer-1
lowering splits it into tied notes) and inner polyphony (both writers emit one
stream per staff). Also added `tests/humdrum_roundtrip.rs` — `.krn` previously
had no round-trip file of its own — and 7 ABC regression tests.

### DLPack

**Already supported, transitively — no dependency needed.** Every representation
is returned by `into_pyarray_bound` as a real NumPy array, and NumPy implements
`__dlpack__`/`__dlpack_device__`, so `torch.from_dlpack(lytk.to_piano_roll(doc))`
shares the buffer today (verified: writing through the DLPack view mutates the
original, device reports `kDLCPU`). Adding a Rust `dlpack` crate and hand-rolling
a `PyCapsule` would buy nothing. What was actually missing:

- the `numpy>=1.21` floor sat below the protocol (`__dlpack__` landed in 1.22,
  `np.from_dlpack` in 1.23) — bumped to `numpy>=1.23`;
- no test pinned the guarantee — added two in `tests/test_representations.py`;
- it was undocumented — now in the README.

### Verified, no change needed

- **All 36 format pairs convert** (ly · xml · mxl · mid · abc · krn, every
  direction) without error.
- **Augmentations are semantically correct**, not just involutive: `transpose`
  shifts every pitch exactly and keeps onsets; `--interval M3` == +4 semitones;
  `--to-key D` applies one uniform shift; `invert` keeps every pitch *sum*
  constant (a true mirror); `retrograde` exactly reverses the pitch order; all
  three preserve the duration multiset; transpose ∘ transpose⁻¹, invert², and
  retrograde² are identities.
- **All three ML representations are exact inverses** (`to_*` → `from_*` → `to_*`
  is bit-identical for note-array, event-sequence and piano-roll).

### Documentation drift corrected

`docs/import-export.md` still listed Humdrum as *"planned — not yet implemented"*
two commits after it shipped; it now has a full import/export table. The CLI's
`convert` help omitted Humdrum. The `Duration` doc comment had `tuplet_normal`
and `tuplet_actual` swapped in its example (the code follows MusicXML: actual=3,
normal=2 for a triplet).

### Known gap, not fixed

`français` is absent from `PitchLanguage` (11 languages, LilyPond ships 12). The
README's "all 11 languages" is self-consistent, so no promise is broken, but a
French LilyPond source is rejected. Not added here: the reference projects the
pitch tables were ported from are not present in this checkout, and guessing the
accidental suffixes would risk silently mis-spelling pitches.

## 2026-09-20 — PyPI release preparation + DL data loaders

lytk ships as a **Python package only**; the Rust crate is the implementation,
not a published artifact. **1014 Rust + 150 Python tests, 0 failures** (torch and
TensorFlow both installed, so the framework adapters actually run); clippy clean.

### Deep-learning integration

- **Padded data loaders — `to_pytorch_dataloader()` / `to_tensorflow_dataloader()`.**
  Previously `to_pytorch_dataset()` was the end of the road: every representation
  is ragged along its first axis (note arrays `(n_notes, 4)`, event sequences
  `(n_events,)`, piano rolls `(n_frames, 128)`) and no two scores agree, so the
  obvious next line — `DataLoader(ds, batch_size=8)` — died with *"stack expects
  each tensor to be equal size, but got [947] at entry 0 and [915] at entry 1"*.
  Both loaders now pad the batch and return `(padded, lengths)`. The lengths are
  explicit rather than inferred from the padding, because the pad value is not
  reserved: `0` is a legitimate event code, pitch and velocity, so trailing zeros
  are ambiguous. Verified on all three representations, that padded content
  matches the unbatched items exactly, and that the torch and TensorFlow loaders
  produce byte-identical batches.
- **`pad_collate` is exported** for use as a `collate_fn` with a hand-rolled
  `DataLoader`. `to_pytorch_dataloader` forwards `**kwargs` to the `DataLoader`
  (`num_workers`, `pin_memory`, `drop_last`, …) with converter arguments in
  `representation_kwargs`.
- **Fixed: a TensorFlow-only dtype bug.** `pad_value` defaults to `0.0`, and every
  representation is an integer dtype, so `padded_batch` raised *"Cannot convert
  0.0 to EagerTensor of dtype int64"* — torch silently casts, TensorFlow does not.
  The pad scalar is now cast through numpy. This class of bug could not have been
  caught before: CI installed torch but never TensorFlow, so **every** tf test
  silently skipped. CI now installs `tensorflow-cpu`.
- **Fixed: `FolderDataset` could not see ABC or `**kern` files.** The CLI has read
  both for some time, but `SUPPORTED_EXTENSIONS` in `datasets/base.py` listed only
  LilyPond/MusicXML/MIDI, so a dataset over an ABC or kern corpus — including the
  1328-file music21 kern corpus this project validates against — silently
  discovered *zero* files and reported `len(ds) == 0` rather than an error.
  `.abc`, `.krn` and `.kern` now load, and `test_extensions_match_the_cli` asserts
  the CLI and dataset format lists agree so they cannot drift apart again.
- **Install API**: `torch>=2.0` / `tensorflow>=2.12` / `eval` extras gained
  version floors, plus a new `all` extra — `pip install "lytk[torch]"`,
  `"lytk[tensorflow]"`, `"lytk[eval]"`, `"lytk[all]"`. Both frameworks stay
  lazily imported, so `import lytk` needs neither.

### Release readiness

- **Licensing gap closed (MIT compliance).** `src/tree-sitter/` vendors the
  generated tree-sitter-lilypond parser (~1.1 MB of `parser.c` and friends) and
  was shipping inside the wheel with no licence text or attribution — MIT requires
  the notice to travel with the code. Added the verbatim upstream notice at
  `src/tree-sitter/LICENSE` (© Nathan Whetsell) plus a provenance `README.md`, and
  listed it in `license-files`, so the wheel and sdist now carry it
  (`dist-info/licenses/src/tree-sitter/LICENSE`).
- **4 real clippy findings fixed.** The crate-level
  `#![allow(clippy::useless_conversion)]` existed only for pyo3's macro expansion
  but silenced the whole crate; scoping it to `mod python` exposed four genuine
  `useless_conversion` hits on `ts.beats_fraction().into()` in
  `ly_to_ir/chord_mode.rs` and `ly_to_ir/figured_bass.rs`. The bindings also moved
  out of `src/lib.rs` (1002 lines → a 52-line root) into `src/python.rs`.
- **Fixed a latent release-blocking bug**: the `test` gate in `release.yml` ran
  `maturin develop` without the `pip install --upgrade pip` that the CI job
  documents as required for PEP 735 dependency-groups, so a tagged release would
  have failed before publishing anything.
- **`rust-version = "1.85"`** declared (clap 4.6 is the floor) with a CI job
  pinned to exactly that toolchain — it matters because the sdist is compiled on
  the user's own machine.
- **README/docs corrected for a public audience**: the reference table pointed at
  a dozen vendored directories absent from the repo (replaced with an
  acknowledgements list); Humdrum was still listed as unimplemented; test counts
  were ~140 commits stale; `ruff` was documented but configured nowhere. Added
  PyPI/Python/CI/licence badges, the extras matrix, a data-loader section,
  `CONTRIBUTING.md` and `SECURITY.md` (parser threat model: untrusted input files,
  `\include` following, MXL decompression caps). All three README Python examples
  were executed verbatim as a check.
- **Verified the artifacts, not just the build**: the wheel contains
  `lytk/_core.abi3.so` and both licences, and the sdist was installed from source
  into a clean venv (`--no-binary lytk`) where parsing, transposing,
  note-array/piano-roll encoding, the `lytk` console script and CLI conversion all
  work.

### Still the maintainer's call
- Version stays **1.0.0** and no tag was pushed.
- PyPI Trusted Publishing must be configured for the repo before a `v*` tag will
  publish.
- `GPL-2.0-only` is restrictive for a library (it prevents use from non-GPL
  projects). `Cargo.toml` and `pyproject.toml` agree, so only the intent needs
  confirming — note Epic G's EGT4 row describes it as `GPL-2.0-or-later`, which
  never matched the manifests.

## 2026-07-10 — Humdrum (`**kern`) format support

New adapter pair `humdrum_to_ir` / `ir_to_humdrum` (Layer-1 Music tree, same
shape as the ABC adapter), wired into every surface. **1014 Rust + 135 Python
tests, 0 failures**; clippy clean.

- **Parser**: kern pitches (letter runs, `#`/`-`/`n`), recip durations (dots,
  non-power-of-two recips decomposed into base + tuplet ratio, rational `N%M`),
  rests, chords, ties (`[` `_` `]`), slurs, grace notes (`q`/`Q`), fermatas,
  barlines incl. repeats, tandem interpretations (`*clef…`, `*k[…]`, `*X:`
  mode, `*M`, `*MM`, `*I"`), `!!!COM`/`!!!OTL` metadata, Latin-1 fallback for
  old corpora. Spines map to parts (reversed: kern is low→high). Anacrusis
  detected from a short pre-barline lead-in → `metadata.partial_duration`.
  Spine rearrangement (`*^`/`*v`/`*x`) is a clear error — no silent loss.
  Non-kern spines (`**dynam`, `**text`) are skipped.
- **Emitter**: one spine per (part, voice), time slices aligned with `.`
  nulls, numbered barlines, headers/trailer, ties/slurs/graces, rational
  recips for irregular durations.
- **Lower-layer fix**: `lower_to_score` now honours
  `metadata.partial_duration` — the first measure ends at the pickup length
  and is marked implicit (previously the anacrusis shifted the whole bar
  grid, tie-splitting notes; this also benefits future Layer-1 producers).
- **Surfaces**: CLI `convert`/`transpose`/… accept and emit `.krn`/`.kern`
  (`--format krn`), batch mode walks kern files; Python `from_humdrum`,
  `from_humdrum_string`, `to_humdrum` (+ stubs, `lytk.cli`).
- **Verification**: the full music21 kern corpus (1328 files: chorales,
  Chopin, Palestrina, …) — **1325 convert + round-trip cleanly** (≤5% note
  drift tolerance), 2 files rejected for spine ops, 1 known divergence
  (Missa_Sine_nomine Kyrie, +5.8% from irregular mensural barring). Proptest
  fuzz net extended (arbitrary text + kern-shaped soup).


## 2026-07-10 — Review fixes R8–R10: MXL output, Layer-1 Python transforms, GIL release

Third fix wave from the review backlog. **1000 Rust + 134 Python tests, 0
failures**; clippy clean.

- **R8 — compressed MXL is a real output everywhere**: new
  `IrToMxmlAdapter::convert_mxl_bytes` zips the *decoded* XML (microtones
  intact — the direct-`.mxl` ceiling from the R7 note is gone) with a proper
  `META-INF/container.xml`. Wired into: CLI (`convert x.ly -o y.mxl`,
  `--format mxl`), batch jobs (`"format": "mxl"` no longer writes plain XML
  into a `.mxl` name), Python (`to_musicxml(score, "out.mxl")` writes
  compressed; `lytk.cli` accepts `.mxl` outputs).
- **R9 — Layer-1 transforms are first-class on every surface**:
  - Python: `transpose`/`transpose_interval`/`invert`/`retrograde`/
    `change_language` now accept a `Score` *or* a `MusicDocument` and return
    the same type — augmentation pipelines stay on the Music tree with no
    lossy Score round-trip. New `transpose_to_key(music, "Bb")` binding
    (Score and MusicDocument; new `transpose_to_key_music` in Rust). Stubs
    updated with a `TypeVar`.
  - CLI: LY→LY `transpose`/`invert`/`retrograde`/`change-language` now go
    through the Music tree like `convert` — output keeps the document's own
    context structure instead of flattened `pB = { … }` variables.
  - `parse_tonic` moved to `ir::pitch` (shared by CLI and bindings).
- **R10 — perf**: PyO3 bindings release the GIL around parse/convert/transform
  calls (`py.allow_threads`) — Python threads can now overlap conversions;
  dropped the borrow-appeasing deep clone in `resplit_measures_to_match`
  call site; event-sequence decoding uses a `VecDeque` FIFO instead of
  `Vec::remove(0)`. Deferred: `resolve_variable`/merge.rs clone reduction and
  `ir_to_mxml` emission cost (architectural; benchmark first).


## 2026-07-10 — Review fixes R4–R7: retrograde, MIDI pairing, output contract, ingestion gaps

Second fix wave from the 2026-07-10 review backlog, all TDD with corpus
verification. Round-trip scores vs the audit baseline: `tests/fixtures/xml`
**131/142 → 142/142**, acid corpus **143/159 → 158/159** (only
45i-Repeats-Nested remains, off by one note), LilyPond fixtures **31/35 →
34/35** (only pedal.ly's known ~0.5% tie drift). MIDI fidelity gate raised
3/3/2 → 4/4/3. Full suite: **998 Rust tests, 0 failures**; clippy clean.

- **R4 — retrograde is structurally sound** (`transforms/retrograde.rs`):
  positional properties (attributes, barlines, numbering) stay on their
  measure shells — the reversed score declares key/meter up front; only
  musical content travels. Tie/slur/tuplet Start↔Stop pairing is swapped so
  every pair still opens before it closes; grace runs are re-anchored before
  their principal (both layers, `Music::Grace` included); Music-layer
  leading attribute events (`\key`/`\time`/clef/tempo) no longer migrate to
  the end. Still self-inverse. 7 new tests. Known ceiling: content swapped
  across unequal-length measures under mid-piece meter changes; in-measure
  direction offsets keep forward time.
- **R5 — MIDI unison collision** (`midi_to_ir.rs`, `ir_to_midi.rs`): the
  reader now keeps a FIFO of open notes per (key, channel) instead of a
  single slot — overlapping same-pitch notes across voices no longer vanish
  (`<< { c'1 } \\ { c'2 c'2 } >>` keeps 3 notes; piano fixtures had lost up
  to 98 sounding notes). The writer orders note-offs before note-ons at equal
  ticks for other consumers. Grace-note timing was already fixed in Wave 1;
  the residual note-count growth on re-import (grace → real note) is inherent
  to MIDI. Fidelity baselines raised to lock the gain.
- **R6 — multi-`\score` output contract** (`main.rs`): the first movement now
  lands at the requested path (pipelines read what they asked for), movements
  2+ at `_02`/`_03` siblings, with a stderr note. Fixed the example2 /
  key-signature-left-edge round-trip failures.
- **R7 — MusicXML ingestion gaps** (all worked around the `musicxml` crate's
  strictness in the raw-XML pre-pass, `adapters/mod.rs`):
  - *Microtones*: the crate types `<alter>` as i16, silently dropping any
    note with a fractional alter (01d/01f lost 100%/75%). Fractional alters
    are now bridged as encoded integers (`0.5 → 1050`) past the crate and
    decoded to exact `Ratio` alters; symmetric on emission, so
    `<alter>0.5</alter>` round-trips and `.ly` gets real quarter-tone names
    (`cih`). Ceiling: direct compressed-`.mxl` writes keep the encoded form.
  - *Orphan/id-less parts*: `<part>` elements missing from `<part-list>` (or
    with no id — the crate requires one; a sentinel id is injected) are kept
    in document order with distinct ids instead of dropped (41g/41h).
  - *Single-quoted attributes*: the crate's tokenizer drops notes carrying
    `dynamics='68'` / `beam number='1'` style attributes (Sibelius exports,
    99a lost 6/6). A tag-aware normalizer rewrites attribute quotes without
    touching text content.
  - *`<words>` quoting*: direction text, `\mark`, and `\tempo` strings now
    go through `escape_ly_string` — a quote in `<words>` no longer emits an
    unterminated LilyPond string (31a lost 57/57).


## 2026-07-10 — Review fixes R1–R3: repeats round-trip, rest-only parts, transpose spelling

The three top items from the 2026-07-10 review backlog, all TDD (failing test
first) with empirical corpus verification. Acid-corpus round-trip: **143/159 →
153/159**; `tests/fixtures/xml`: **131/143 → 137/143**; LilyPond fixtures:
**31/35 → 32/35** (repeats.ly went from 2030 → 0 notes to 2030 → 2030 exact).
Full suite: **985 Rust tests, 0 failures**; clippy clean.

- **R1 — `\repeat`/`\alternative` round-trip** (two coordinated fixes):
  - *Producer* (`ly_to_ir/modifiers.rs`): the backward repeat now lands on the
    right barline of every alternative except the last (MusicXML-correct:
    `<ending type="stop"/>` + `<repeat direction="backward"/>` together),
    instead of on a spurious empty measure after the alternatives.
  - *Emitter* (`ir_to_ly/emit.rs`): tracks `\alternative`/ending state
    explicitly — closes the block by lookahead (not on ending‑1's backward
    repeat, which is implicit in LilyPond), retro-wraps `\repeat volta N {`
    when endings appear with no forward repeat (acid 45b: repeat from score
    start), supports multi-measure endings, `discontinue` stops, and the
    redundant per-measure ending markers that merged multi-voice piano parts
    carry. Tests: `repeat_alternative_roundtrip` (3 cases incl. the 45b
    fixture).
- **R2 — rest-only parts no longer deleted** (`ly_to_ir/merge.rs`):
  `part_is_dynamics_only` now requires the part to be all spacers *or* to
  carry directions; a part of real rests with no directions is a resting
  instrument, not a Dynamics lane. Acid tests 41b/41f/41g‑NestingOrder/43g
  (previously 100% content loss) all round-trip exactly. Tests:
  `rest_only_parts` (2 cases).
- **R3 — transpose is now key-aware** (`transforms/transpose.rs`):
  - Chromatic mode respells results into the *target* key via the previously
    unused `pitch::respell()` — C major +3 now yields `\key ees` with
    `ees g bes`, not `dis/ais` under an E-flat signature. Threads the current
    (transposed) key through measures (Score) and the Music tree (branch-local
    across `Simultaneous`); microtonal pitches are exempt from respelling.
  - Diatonic mode derives the key signature from the interval's
    line-of-fifths delta (`7·semitones − 12·steps`): `--interval d5` from C
    now gives G-flat (6♭), `A4` gives F-sharp (6♯); ±12-fifths results
    normalize enharmonically. `--to-key` inherits the fix.
  - Chord symbols (`Harmony` root + bass) transpose with the notes, in both
    modes and both layers (`Music::Harmony` included).
  - Tests: `spelling_tests` (4 cases). CLI verified end-to-end.
- Remaining known ly-fixture failures are the R6 multi-`\score` output-path
  contract (example2, key-signature-left-edge) and pedal.ly's small +8-note
  drift; remaining corpus failures are R7 ingestion gaps (microtone alters,
  orphan/id-less parts, 31a `<words>` quote escaping, 99a) and a 1-note drift
  on 45i-Repeats-Nested. R4 (retrograde) and R5 (MIDI) untouched.


## 2026-07-10 — Full-codebase review: panic fixes, lint zero, doc corrections

A multi-agent audit (coherence, code quality, performance, transform semantics,
Python surface + empirical round-trip testing of every fixture and the 159-file
LilyPond MusicXML acid corpus). Fixes landed in this pass:

- **Trust-boundary panic fixes** (malformed input files could abort the process,
  reproduced via CLI; all now degrade gracefully, with regression tests):
  - MusicXML `<actual-notes>0</actual-notes>` (or 256, wrapping through `as u8`)
    panicked in `Frac::new` — invalid tuplet ratios are now ignored
    (`src/adapters/mxml_to_ir/note.rs`).
  - MusicXML `<divisions>65536</divisions>` truncated through `as u16` to 0 and
    panicked; ≥65536 silently corrupted durations — now clamped to 1..=65535
    (`src/adapters/mxml_to_ir/part.rs`).
  - LilyPond `c4*1/0` duration scale panicked — zero denominators now ignored
    (`src/adapters/ly_to_ir/consume.rs`).
  - ABC duration digit-runs overflowed i64 (panic in debug, wrap in release) and
    80 slashes overflowed the shift — now saturating + capped
    (`src/adapters/abc_to_ir.rs`).
  - The mxml panic firewall (`catch_read`) now also wraps `convert_mxml_score`,
    so conversion panics surface as `AdapterError::Parse` at the PyO3 boundary.
- **Lint zero:** fixed all 7 compiler warnings (dead test code, unused imports)
  and all 26 clippy warnings (test-only); `cargo clippy --all-targets` is clean.
- **Doc corrections:** CLAUDE.md no longer claims Python is "not yet fully
  wired", lists all 13 CLI subcommands, documents the representations/navigation
  layer, and drops the aspirational bumpalo/`Arc<Node>` guidance (neither was
  ever used); README/roadmap IR hierarchy fixed to Score → Part → Measure →
  Voice.
- Full suite: **976 Rust tests, 0 failures**; clippy clean.

### Review findings — prioritized backlog (not yet fixed)

Empirical status: 939 CLI invocations over 313 MusicXML files — zero crashes;
285/313 round-trip cleanly; 31/35 LilyPond fixtures round-trip with exact note
counts; MIDI 42/52. Every remaining failure is *silent* data loss (exit 0).

1. **Repeats/alternatives re-emission (HIGH):** `ir_to_ly` emits duplicated /
   unbalanced `\alternative` blocks; re-parse silently drops nearly everything
   (repeats.ly round-trip 2030 → 0 notes; 45b/45d acid tests).
2. **Rest-only parts collapse (HIGH):** `part_is_dynamics_only`
   (`ly_to_ir/merge.rs`) treats any all-rest part as a Dynamics lane — 8 acid
   files lose 100% of content (41b, 41f, 41g-NestingOrder, 43g …).
3. **Transpose spelling (HIGH):** chromatic mode spells all black keys as
   sharps while correctly moving the key signature to flats (C→Eb yields d♯/a♯
   under `\key ees`); diatonic mode computes the new key from semitones only
   (F♯-major signature with G♭ notes); chord symbols/harmonies are not
   transposed at all. `respell()` exists in `pitch.rs` but is never called.
4. **Retrograde is structurally naive (HIGH):** measure attributes stay on the
   (now-last) first measure; tuplets/ties/slurs/wedges keep forward-time
   pairing (dangling ties, inside-out slurs); grace notes end up after their
   principal; sounding durations change.
5. **MIDI cross-voice unison collision (HIGH):** same-tick note-on ordered
   before another voice's same-pitch note-off; reader mis-pairs and drops both
   (piano fixtures lose 3-5% of sounding notes). Grace notes exported with real
   duration shift all later onsets (+12.5% on tablature-grace-notes.ly).
6. **Multi-`\score` output path (MEDIUM):** converting a multi-score .ly writes
   `out_01.xml`/`out_02.xml` instead of the requested path, exit 0, no notice.
7. **MusicXML ingestion gaps (MEDIUM):** microtone `<alter>0.5</alter>` notes
   silently dropped (01d/01f); `<part>` without id or without a matching
   `<score-part>` dropped; untyped whole-measure rests lost on ly emit (02e).
8. **MXL output unreachable (MEDIUM):** the adapter can write compressed MXL
   but no surface exposes it; `batch` with `"format": "mxl"` writes plain XML
   into a `.mxl` file. CLI `--format` has no mxl; Python has no compressed
   write.
9. **Layer-1 transforms unbound (MEDIUM):** Python and the transform
   subcommands use only Score-based transforms; `apply_music` exists but is
   unreachable, so ly→ly augmentation flattens structure. `transpose_to_key`
   has no Python binding.
10. **Perf (MEDIUM, measured):** `ir_to_mxml` typed-struct emission dominates
    time+memory; `resolve_variable` deep-clones every reference;
    `ly_to_ir/merge.rs` is the clone hotspot (79 clones). No GIL release
    around long conversions in PyO3. Piano-roll default resolution 480 → 11 MB
    arrays per short piece.
11. **Minor:** invert not self-inverse (doc claim); microtones destroyed by
    chromatic transpose but preserved by diatonic; datasets are folder-only
    (JSB Chorales remote loader still pending); Python CLI is a 4-subcommand
    subset advertised as a mirror; `benches/bench_python.py` fixture paths are
    dead; FolderDataset .npy cache never invalidates.


## 2026-06-19 — Wave 2 / P4.8–P4.9: measure-repeat (the "%" sign)

Closes a cited gap vs MuseScore: measure-repeat now survives MusicXML round-trip.

- **IR:** `Measure.measure_repeat: Option<u8>` (`src/ir/measure.rs`) — repeat the
  previous N measures; `#[serde(default, skip_serializing_if)]` keeps measure JSON
  byte-for-byte unchanged.
- **MusicXML round-trip:** `mxml_to_ir` reads `<measure-style><measure-repeat
  type="start">N</measure-repeat>` (ignoring `type="stop"`); `ir_to_mxml` emits it
  (and the attributes block is now emitted when a measure carries only a
  measure-repeat). Test `measure_repeat_round_trips` (emit → re-parse → `Some(1)`).
- **Deferred:** LilyPond `\repeat percent N` emit/parse (Score→LY path).
- ~23 `Measure` struct literals across the codebase gained `measure_repeat: None`.
  Full suite: **973 Rust tests, 0 failures**; clippy clean; fidelity unchanged.

## 2026-06-19 — Wave 2 / P4.7: MusicXML default vertical positions

MusicXML export now emits conservative `default-y` on directions so output
renders cleanly in editors that honor it (`src/adapters/ir_to_mxml/direction.rs`):

- A `placement_default_y` helper maps placement → tenths (below → −80, above →
  +30, unspecified → none); applied to dynamics, hairpins (wedge), pedal, and
  words/tempo. `placement` (above/below) and `<print>` page/system breaks were
  already emitted — this fills in the vertical offset.
- `default-x` is intentionally left unset (lytk has no layout engine; the
  consuming app spaces horizontally). `<staff-layout>` staff-distance deferred —
  editors default it and it needs multi-staff threading for marginal value.
- Adjusted two assertions that matched bare `<dynamics>` to tolerate the new
  attribute (`<dynamics default-y=…>`); output is unchanged semantically.
- New test `direction_emits_default_y_by_placement`. Full suite: **972 Rust
  tests, 0 failures**; clippy clean; fidelity baselines unchanged (positions are
  not part of the semantic signature).

## 2026-06-19 — Wave 2 / Epic P7 (partial): harmony functional Roman numerals

`Harmony` can now carry a functional-harmony Roman numeral and round-trip it
through MusicXML:

- **`Harmony.function: Option<String>`** (`src/ir/harmony.rs`) — the MusicXML
  `<function>` element (e.g. `"V"`, `"ii"`), supplementing the chord symbol.
  `#[serde(default, skip_serializing_if = "Option::is_none")]` keeps existing
  harmony JSON byte-for-byte unchanged.
- **Round-trip:** `mxml_to_ir` reads `<function>` into the field; `ir_to_mxml`
  emits it. Test `test_emit_harmony_with_function` asserts `<function>V</function>`.
- **Deferred (T7.3 + scope):** figured-bass accidental typing / extension lines
  (low value); the MusicXML 4.0 `<numeral>` element and root-optional functional
  harmony (would ripple `Harmony.root` to `Option`).
- Full suite: **971 Rust tests, 0 failures**; clippy clean; fidelity unchanged.

## 2026-06-18 (cont.) — Wave 1 / Epic P11: JSON batch-job API

A `batch` subcommand driven by a JSON job file (`src/main.rs`):

- **Schema:** an array of `{in/input, out/output, format?, from?, transpose?,
  interval?}` (serde, with `in`/`out` aliases). Each job can convert between any
  supported formats and optionally transpose (chromatic `transpose` or diatonic
  `interval`, the latter winning if both are set).
- **Executor:** reuses the existing rayon pool + per-job `catch_unwind` isolation
  (one bad job fails only itself) and `-j` thread control; exits non-zero if any
  job failed, so it gates automation.
- **`--report <path>`** (`-` for stdout): a JSON array of per-job results
  (`input`/`output`/`ok`/`error`) — the diagnostic sidecar.
- **Deferred (T11.3 remainder):** visible-parts filtering and filename templates
  inside batch jobs overlap excerpt-selection (P1/P8) and are deferred; the
  `bundle` command already covers per-part splitting.
- **Tests:** 2 new CLI tests (multi-job success incl. per-job interval transform;
  partial-failure exit code + report contents). Full suite: **970 Rust tests, 0
  failures**; 136 Python; fidelity baselines unchanged.

## 2026-06-18 (cont.) — Wave 1 / Epic P10: machine-readable automation outputs

Four non-notation CLI outputs for automation/CI (`src/main.rs`):

- **`info --json`** — a curated metadata summary (title/composer/…, language,
  per-part id/name/measures/staves/midi_program, total note count). Stable
  human-meaningful subset, not the full serialized score.
- **`positions`** — per-part measure positions (`start`/`duration` in quarter
  notes) as JSON. *Temporal/structural* positions derived from notated durations
  (measure length = longest voice), explicitly NOT graphical coordinates.
- **`bundle -o <dir>`** — exports each part to its own single-part file
  `<stem>_<part>.<ext>` (metadata/layout carried over); `--format` picks the
  output format (default xml).
- **`diff a b [--json]`** — semantic comparison on sounding content (part count,
  note count, sorted MIDI pitch multiset). Exits non-zero when scores differ, so
  it can gate CI; the comparison ignores source-text formatting.
- **Tests:** 5 new CLI tests (info-json, positions, bundle, diff equal/differ).
  Full suite green: **559 lib + 42 CLI + 208 fixture + … Rust; 136 Python**;
  fidelity baselines unchanged.

## 2026-06-18 (cont.) — Wave 1 / Epic P2: transpose modes & enharmonic spelling

The highest-value backlog item: transposition is no longer semitone-only.

- **`Interval` type** (`src/ir/interval.rs`): a signed `(diatonic, chromatic)` pair
  with a name parser (`M3`, `m3`, `P5`, `A4`, `d5`, `-m2`, `P8`, `M10`, `AA4`,
  `dd5`) and `Interval::between(a, b)`. Invalid qualities for a class (`P3`, `M5`)
  are rejected.
- **Spelling-correct transposition** (`Pitch::transpose_diatonic`): "up a major
  third" (C→E) and "up a diminished fourth" (C→F♭) are now distinct, with correct
  letters and accidentals; microtonal `alter` carries through. Chromatic
  (semitone) transposition keeps its existing nearest-natural behavior.
- **Key-aware enharmonic spelling** (`pitch::respell(pitch, fifths)`): chooses the
  spelling that fits a key signature (F♯ in a sharp key, G♭ in a flat key),
  preserving the sounding pitch. Mode-independent (depends only on `fifths`).
- **`Transpose` now carries a `TransposeMode { Chromatic, Diatonic }`**;
  back-compatible `Transpose::new(semitones)` and `transpose()` kept. New
  `transpose_interval`, `transpose_interval_music`, and `transpose_to_key`
  (nearest-direction, ≤ tritone; key signatures shift with the music).
- **CLI:** `transpose` gains `--interval <name>` and `--to-key <tonic>` (e.g.
  `--to-key Bb`/`F#`/`ef`), mutually exclusive with `--semitones` (exactly one
  required). **Python:** `lytk.transpose_interval(score, "M3")` (+ `.pyi` stub and
  `__init__` re-export).
- **ABC (T2.5):** the emitter now respells pitches against the active `K:` so
  output follows the key's enharmonic spelling (sounding pitch preserved, so
  round-trip stays faithful). Remaining: omit key/within-bar-implied accidentals
  (needs matching parser support) — tracked.
- **Tests:** `interval` (3), `pitch` (5: diatonic transpose, octave carry,
  respell, `major_tonic`), `transpose` (2: by-interval spelling + to-key), ABC
  respelling (1), and 4 CLI tests. Full suite green: **559 lib + 37 CLI + 208
  fixture + … Rust; 136 Python**; fidelity baselines unchanged.

## 2026-06-18 (cont.) — Wave 1 / Epic P1: CLI & I/O ergonomics

Expanded the CLI surface and added stream I/O (6 of 7 P1 tasks; `src/main.rs`):

- **stdin/stdout streaming (T1.1):** `-` is now a valid input/output path. Reading
  stdin requires `--from <ly|xml|midi|abc>` (no extension to infer from); writing
  stdout requires `--format`. Wired through new `parse_source`/`parse_bytes`/
  `render_output`/`write_bytes` helpers; `convert_ly_to_ly` is stdin/stdout-aware.
  Multi-movement to stdout is a clear error. MXML-over-stdin uses the bounded
  `convert_bytes` (handles plain XML and zipped MXL).
- **New transform subcommands (T1.2–T1.4):** `invert` (`--axis c4`/`fs3`/`bf5`,
  default middle C), `retrograde`, and `change-language` (`-l <lang>`) now expose
  the existing `transforms::{invert,retrograde,change_language}` — previously
  library/Python-only. All single-file subcommands gained `--from` for stdin.
- **`abs2rel` / `rel2abs` (T1.5–T1.6):** LilyPond-only re-emission of pitch entry
  in `\relative` vs absolute form, via the Score path (which honors
  `metadata.pitch_mode` and wraps reliable single-staff/voice parts in `\relative`;
  complex parts fall back to absolute, as documented). The IR is always
  absolute internally, so both commands are parse → set mode → re-emit.
- **Tests:** 12 new CLI integration tests in `tests/cli.rs` (streaming round-trip,
  `--from`/`--format` requirement errors, each subcommand, bad-axis and
  unknown-language errors, `abs2rel` emits `\relative`, `rel2abs` drops it,
  non-LilyPond rejection). Full suite green (547 lib + 33 CLI + 208 fixture + …);
  fidelity baselines unchanged.
- **Deferred (T1.7):** LilyPond `indent`/`reformat` — needs a source-preserving
  (whitespace-only, comment-preserving) reindenter over the tree-sitter parse;
  a parse→emit shortcut would be lossy and is redundant with `convert in.ly -o out.ly`.

## 2026-06-18 (cont.) — Pre-1.0.0 expansion roadmap landed

A lytk-vs-MuseScore CLI/converter & import-export comparison (multi-agent review)
produced a 24-item backlog of features where MuseScore leads inside lytk's own
scope. The backlog is now organized into **12 epics (P1–P12)** of atomic tasks and
added to [`docs/roadmap.md`](roadmap.md) as the **Pre-1.0.0 Expansion** section;
full task detail + file anchors live in the plan file
`~/.claude/plans/add-as-a-next-mighty-matsumoto.md`.

- **Scope decisions:** lytk stays **GPL-2.0-only** (MuseScore read for approach,
  not copied); "full geometry/style" scoped down to **sensible default positions /
  placement hints** (no layout engine); the `tests/fidelity.rs` semantic scoreboard
  stays a **hard, non-decreasing CI gate** (visual regression deferred — no rendered
  output). All 12 epics gate the 1.0.0 release (owner decision).
- **Build-order waves:** cheap/high-value first (CLI/IO, automation outputs, batch
  API, transpose modes, XSD validation) → IR extensions (options, keys/time/
  positions/measure-repeat, enum typing, harmony) → heavy modeling (beams/tuplets,
  instruments/tab/perc/fretboard) → MIDI reconstruction (depends on tuplets +
  percussion).
- No code behavior changed in this landing — docs only.

## 2026-06-18 (cont.) — MIDI instrument preservation across formats

Instrument identity now survives every conversion between LilyPond, MusicXML and
MIDI. Each format carries it differently — **LilyPond** a name
(`\set Staff.midiInstrument = "violin"`), **MIDI** a program number (a 0-indexed
Program Change), **MusicXML** both a `<midi-name>` and a (1-indexed)
`<midi-program>` — and the IR previously bridged them inconsistently, so e.g.
**ly → musicxml dropped the program entirely** (it emitted `<midi-name>` but no
`<midi-program>`, and a generic `<instrument-name>Instrument</instrument-name>`)
and **midi → musicxml was off by one** (it wrote the raw 0-indexed program as the
1-indexed MusicXML value, turning a violin into a viola).

- **New shared GM table** (`src/adapters/gm.rs`): the 128 General MIDI
  instruments as the canonical bridge, with `gm_program_from_name` (name →
  0-indexed program, canonical names + aliases) and `gm_name_from_program`
  (program → canonical LilyPond name). `ir_to_midi`'s private table was folded
  into it.
- **Canonical convention:** `Part.midi_program` is now consistently the
  **0-indexed MIDI program** (violin = 40) everywhere. The MusicXML adapters
  convert at the boundary: the reader stores `<midi-program> − 1`, the writer
  emits `program + 1`.
- **Cross-fill at every boundary** so a one-sided source becomes two-sided:
  - `ir_to_mxml` derives the program from the name (and vice-versa), emits a real
    `<midi-program>` and a title-cased `<instrument-name>` (e.g. `Violin`).
  - `mxml_to_ir` recovers the GM name from the program when `<midi-name>` is
    absent.
  - `midi_to_ir` recovers the GM name from the Program Change (threaded as
    `Option<u8>` so a track with *no* program change stays unset rather than
    defaulting to piano).
  - `ir_to_ly` already emitted from the name, which is now always populated.
- **Tests:** `src/adapters/gm.rs` unit tests (128-entry round-trip, aliases);
  `tests/instrument_preservation.rs` (5 cross-format chains: ly→xml, ly→xml→ly,
  ly→midi→ir, ly→midi→xml, xml-program-only→ly); a new `mxml_to_ir` test for
  program-only → name recovery. The existing `test_parse_midi_instrument_info`
  now asserts the 0-indexed value (40, not the raw 41).
- **Known gap:** ABC has no standard instrument field (the `%%MIDI program`
  directive is non-standard/abc2midi-specific), so →ABC still drops the
  instrument — a format limitation, not a bug.

Counts: **936 Rust tests** green; clippy (lib+bin) clean; **136 Python** green.

## 2026-06-18 — ABC multi-voice, structured note navigation, MIDI carried-meter fix

Three feature/fix landings plus a roadmap reconciliation (several items the
roadmap still listed as open were already fixed on the hardening branch — see
below).

### ABC multi-voice (`V:`) — ABC 2.1 §4.1 (parser + emitter)

ABC is no longer single-line-only: multi-voice tunes now round-trip.
- **Parser** (`abc_to_ir.rs`): `V:id [name=…]` voice fields are recognised in
  the header (declaration) and the body (a line-start `V:id` field, or an inline
  `[V:id]` marker, switches the active voice). Voice streams accumulate across
  interleaved `V:` blocks. The shared header `M:`/`K:` are prepended to every
  voice so each lowers to a Part with correct attributes. A tune with **one**
  voice still produces a single Staff (byte-compatible with v1). Multi-voice →
  `Simultaneous` of named Staves → multiple Parts on lowering.
- **Emitter** (`ir_to_abc.rs`): a score with ≥2 top-level voices/staves (a
  `Simultaneous` of Staff/PianoStaff contexts, from the parser OR from lifting a
  multi-part XML/MIDI score) now emits `V:1`/`V:2` blocks with `name="…"`; the
  shared `M:`/`K:` stay in the header. Single-staff scores keep the original
  single-line output. Grouping contexts (PianoStaff, …) are flattened so a piano
  grand staff becomes two voices.
- Tests: 6 parser + 2 emitter unit tests; `tests/fixtures/abc/multivoice.abc` +
  `abc_multivoice_roundtrips` / `abc_multivoice_lowers_to_two_parts` integration
  tests (voices, names and per-voice pitch/duration all survive). The fidelity
  ABC baseline rises **3/3/3 → 4/4/4**.

### Structured note navigation (typed Python objects)

The long-standing gap (only the flat `Score.notes()` tuples existed; the typed
Part/Measure tree was never exposed) is now closed. New read-only PyO3 wrappers
in `src/navigation.rs`: **`Part`, `Measure`, `Voice`, `Note`, `Rest`, `Chord`,
`Pitch`**, reachable via `score.iter_parts()` and walkable as
`part.measures → measure.voices → voice.elements`. Each element exposes pitch
(`step`/`alter`/`octave`/`midi`/`name`), duration (quarter-lengths +
exact fraction), ties, articulations, lyrics; measures expose
time/key signature and `senza_misura`; convenience `.notes` flatteners at every
level (chord members included). Each wrapper owns a clone of its IR node, so the
view is strictly read-only and outlives its parent. `.pyi` stubs +
`__init__` re-exports + `tests/test_navigation.py` (7 cases incl. ABC
multi-voice → two navigable parts). Complements `notes()` and `to_dict()`.

### MIDI round-trip: export-side carried-meter fix

`ir_to_midi::build_part_track` padded every bar that didn't itself re-declare
`<time>` to the 4/4 default (`measure_ticks` returned 1536 ticks regardless of
the running meter). For a piece that declares e.g. `\time 3/8` once and then
relies on it, every later bar was over-sized, shifting every subsequent onset
forward on export and breaking the MIDI round-trip for all non-4/4 fixtures. The
loop now carries `current_ts` (mirroring the conductor track) and sizes each bar
by the running meter via the new `ticks_for_ts`; senza-misura bars are never
padded. Regression test `test_running_time_signature_sizes_later_bars` (three
3/8 bars, meter declared once → onsets at 0/576/1152, not 0/1536/3072).
- **Effect:** MIDI fidelity **2/2/2 → 3/3/2** (note-count + pitch). `example2_1`
  now matches on note-count AND pitch multiset; `example.midi`/`example2_0.midi`
  stay fully stable. New `tests/midi_roundtrip.rs` pins the per-fixture guarantee.
- **Remaining (gated, documented limitations):** `example2_1` still drifts on
  one note's *duration* (a tie re-fuses across a meter boundary on re-import);
  `pedal` loses one note to a per-voice quantized-budget cascade; **`chopin_n`
  is INHERENT** — its source MIDI places time-signature changes on non-bar-
  aligned ticks (6/8→4/4 at tick 100416, not a 6/8-bar multiple), so notated
  meter and actual bar lengths disagree and an exact round-trip would need
  arbitrary mid-bar re-gridding that itself breaks fidelity. These were
  investigated (a dedicated root-cause pass) and left gated rather than chased
  with a fragile requantization that would risk the stable fixtures.

### Roadmap reconciliation (stale "open" items already fixed)

The roadmap's **EBT7 follow-up** list marked five bugs ⬜; four had already been
fixed on the hardening branch and are now marked done in `roadmap.md`:
ly→ly relative multi-staff octave (Phase 3a), midi→ly tuplet loss (Phase 3c),
xml→ly repeat-from-top note loss (Phase 3b), ly→midi grace timing (Phase 3d).
The fifth, **ABC multi-voice `V:`**, is the feature shipped above — so the whole
EBT7 list is now resolved.

Counts: **926 Rust tests** (542 lib + 384 integration) green; `cargo clippy
-D warnings` (lib+bin) clean; **136 Python tests** green (3 skipped: optional
torch/scipy deps).

## 2026-06-16 (cont.) — Generation-evaluation metrics (JS-similarity, FMD)

New `lytk.metrics` (ported from `lilybench/`), behind the optional `lytk[eval]`
extra (deps lazy-imported, so the module loads without them):
- **JS-similarity** (`js_similarity`, `js_descriptor_similarity`,
  `aggregate_descriptor_stats`) — `100·exp(-2·mean(JS div))` over Gaussians fit to
  the three MusPy descriptors (`polyphony_rate`, `groove_consistency`,
  `scale_consistency`), taken from lytk's own `compute_metrics` (no muspy needed;
  scipy only).
- **Fréchet Music Distance** (`frechet_music_distance`, `lilybert_embed`,
  `load_documents`) — `||μx−μy||² + Tr(Σx+Σy−2√(ΣxΣy))` over LilyBERT layer-6
  embeddings of raw `.ly` (torch + transformers + a checkpoint).
Added the `eval` extra (`scipy`, `torch`, `transformers`). Tests in
`tests/test_metrics_eval.py` (5: JS math + end-to-end, FMD identity/separation/arity).

## 2026-06-16 (cont.) — Phase 6: CI & quality gates

- `[tool.pytest.ini_options] testpaths = ["tests"]` — a bare `pytest` no longer
  tries to collect the vendored reference projects and die.
- CI now installs `scipy` + CPU-only `torch`, so the torch dataset-adapter tests
  and the eval-metric tests **run** instead of being skipped.
- `release.yml`: a **Test gate** job (cargo test + maturin develop + pytest) now
  gates the PyPI publish (`publish` `needs: […, test]`) — a regression merged
  after the last CI run can no longer ship to PyPI untested.
- **Skipped:** `clippy --all-targets` in CI (36 warnings, all in test code; the
  shipped lib+bin are already gated by default-target `clippy -D warnings`) and a
  pinned MSRV (`rust-version`) — pinning it correctly needs testing old
  toolchains; deferred rather than guessed.

## 2026-06-16 (cont.) — Phase 5: packaging & release metadata (1.0.0)

- **Version → 1.0.0** in `Cargo.toml` + `pyproject.toml`; classifier
  `Development Status :: 5 - Production/Stable`.
- **abi3-py39 → abi3-py310** so the wheel tag (`cp310-abi3`) matches
  `requires-python = ">=3.10"` and the classifiers (built against Python 3.10.19).
- **License reconciled to `GPL-2.0-only`** in both manifests — matches the actual
  bundled `LICENSE` (bare GPLv2) and the README, which the `GPL-2.0-or-later`
  metadata previously contradicted. (If "or later" was intended, add the per-file
  grant and flip these back.)
- Added `src/lytk/py.typed` (PEP 561) so downstream type checkers see the stubs;
  deleted the empty `src/lytk/transforms.py`; `cli.py` no longer probes for MIDI
  (`_has_midi = True`, always built in).
- `release.yml`: added **musllinux** wheels (x86_64 + aarch64) and disambiguated
  the artifact names by `manylinux` tag (the new rows reused runner+target).
- **Skipped:** making `pyo3/extension-module` an opt-in crate feature. It only
  matters for `cargo add lytk` from crates.io, which is not a publish target
  (release is PyPI-only); the refactor risks the working `cargo test` libpython
  linking for no real consumer. lytk stays PyPI-only; the `.cargo/config.toml`
  macOS hack keeps local `cargo test` green.

## 2026-06-16 (cont.) — 1.0.0 hardening (audit remediation)

Working through the [1.0.0 hardening plan](roadmap.md) from the release-readiness
audit. Release-critical spine first (blocker → robustness → fidelity).

**Phase 4 — Public API stabilization.**
- **Typed errors:** a new `adapter_err` helper maps `AdapterError::Io` →
  `IOError` and every parse/validation failure → `ValueError`. The file/bytes
  readers previously raised *all* errors as `IOError`, so a malformed-but-readable
  file looked like a missing one (a batch loader's `except IOError` swallowed
  corrupt input). `AdapterError` is now `#[non_exhaustive]`.
- **Bytes I/O** (no temp files for archives/HTTP/dataset buffers):
  `from_midi_bytes`, `to_midi_bytes` (returns `bytes`), and `from_musicxml_bytes`
  (auto-detects `.mxl` vs plain XML via the bounded reader). Added
  `MxmlToIrAdapter::convert_bytes`.
- **Read-only note navigation:** `Score.notes()` and `MusicDocument.notes()`
  return `(onset, duration, pitch, velocity)` tuples in time steps — a
  lightweight, numpy-free way to iterate notes without `to_note_array` or
  hand-walking `to_dict`. `MusicDocument` also gained the `subtitle`/`arranger`/
  `language` getters for parity with `Score`.
- **Rust crate identity:** added a crate-level `//!` doc (so docs.rs renders a
  landing page) and re-exported `Score` and `MusicDocument` at the crate root.
- Removed the stale "MIDI only with the midi feature" `try/except` in
  `__init__.py` (MIDI is always built); `.pyi` stubs updated for all of the above.
- Tests: bytes round-trip (MIDI + MXL), `notes()` shape, and that a malformed
  string raises `ValueError` not `IOError`. 124 Python + full Rust suite green.

**Phase 3a — Conversion fidelity: relative multi-staff octave shift.** The
Score-path LilyPond emitter (used by Python `to_lilypond(score)`) octave-shifted
whole staves/voices on a relative multi-staff or multi-voice score, because a
single linear `prev_pitch` can't reproduce LilyPond's `\relative` octave
resolution across `<< \\ >>` voices and separate piano staves — pedal.ly drifted
on ~1450 of 1479 notes on round-trip. Fix (`ir_to_ly/mod.rs`): emit a part with
**absolute** octaves whenever relative threading is unreliable (multi-staff, or
any multi-voice measure), matching the always-absolute Music/CLI emit path;
simple single-staff single-voice parts keep the tidy `\relative` form. New
tracked test `tests/relative_multistaff_roundtrip.rs` asserts pedal.ly and
chopin_n.ly preserve their pitch multiset through a Score-path round-trip, and
that a simple part still emits `\relative`. (Replaces a throwaway scratch test.)

**Phase 3b — Conversion fidelity: repeat brace balancing.** A MusicXML bare
backward repeat (repeat-to-top, no forward `|:`) made the Score-path emitter
write a closing `}` with no matching open, truncating the part variable and
silently dropping every later measure's notes (`xml→ly`); a forward repeat with
no end left an unmatched open `{`. `ir_to_ly/emit.rs` now tracks repeat-brace
depth: a backward repeat at depth 0 wraps the section retroactively in
`\repeat volta N { … }` instead of emitting a stray `}`, and any forward repeat
left open is closed at the end of the variable. New test
`tests/repeat_braces_roundtrip.rs` asserts balanced braces and no content loss
for fixtures 45a (backward-only) and 45g (forward-not-ended).

**Phase 3c — Conversion fidelity: tuplet emission to LilyPond.** Elements that
carry a tuplet ratio in their `Duration` but no explicit `TupletDisplay` —
produced by MIDI import and by MusicXML `<time-modification>` without a
`<tuplet>` bracket — emitted as plain notes, so three triplet eighths printed as
three plain eighths and overfilled the bar (invalid LilyPond) on `midi→ly` and
`xml→ly`. `ir_to_ly/emit.rs` now wraps consecutive same-ratio elements in
`\tuplet a/b { … }` (grouped, closed on ratio change/end), skipped while an
explicit `TupletDisplay` tuplet is open so the two never nest. The inner notes
keep their base durations (already correct inside `\tuplet`). Test: a
duration-ratio triplet emits a single balanced `\tuplet 3/2 { … }` wrapper.

**Phase 3d — Conversion fidelity: grace-note MIDI timing.** `ir_to_midi` treated
a grace note like a metrical note, advancing the voice clock by its notated
duration — so every later onset shifted, the bar overflowed, and a round trip
split/duplicated the displaced notes (`ly→midi`). `build_part_track` now emits a
short grace note at the current tick **without advancing** it (mirroring the
note-array, where grace notes consume no time). Test (emit-level): with a grace
eighth before four quarters, D4 stays at tick 0 and G4 at three quarters. (The
re-import side can't recover grace-ness from MIDI — inherent and out of scope.)
This completes Phase 3 conversion fidelity (H6–H9).

**Phase 1 — BLOCKER fixed: tie chains now collapse in the ML representations.**
`to_note_array` (and therefore the event-sequence, piano-roll and metric paths
that build on it) re-articulated every tied note as separate notes: a half tied
to a quarter became two rows instead of one 3-beat note, and every note the
MusicXML/MIDI importers split at a barline (`README`: "notes crossing a barline
are split and tied") became a spurious re-onset — silently corrupting training
data on the library's core path (`from_musicxml`/`from_midi` →
`to_music_document` → `to_note_array`).
- `src/representations/note_array.rs`: the tree walk now fuses tie chains via a
  per-voice `open: HashMap<pitch, idx>` map. Fusion is *forward-looking* on
  `TieStart` (extends the held note with the next same-pitch note) so it works
  for all three tie conventions — the lift path's `TieStart`+`TieStop`
  (MusicXML/MIDI), ABC's `TieStart`-only, and middle-of-chain `TieStop`+`TieStart`
  — mirroring the chain-collapse rule already in `ir_to_midi`'s `tie_flags`. Ties
  are scoped per simultaneous branch so they never cross voices.
- Tests: half-tied-to-quarter → one 1440-step row; 3-segment chain → one row;
  ABC forward-only tie collapses; un-tied repeated pitches stay separate
  (anti-over-fusion); ties do not cross parallel voices. 519 lib tests green.

**Phase 2a — Untrusted-input clamping / checked arithmetic.** Several reachable
crashes/hangs on crafted-but-parseable input are now bounded (debug panics /
release corruption / infinite loops / multi-GB allocations → safe behavior):
- `ir/duration.rs`: new shared `dot_multiplier(dots)` clamps the `1 << dots`
  shift to `MAX_DOTS` (was UB/panic at dots ≥ 63 from a crafted .ly/.xml/.json).
  Reused by `midi_to_ir`'s tick math; dot counting saturates in `consume_dots`
  and is capped in `mxml_to_ir/note.rs`.
- `representations/event_sequence.rs`: clamp `max_time_shift`/`velocity_bins` to
  ≥ 1 (was an infinite loop + OOM for `max_time_shift=0`, and an underflow for
  `velocity_bins=0`); `velocity_to_bin`/`bin_to_velocity` guard `bins=0`.
- `adapters/midi_to_ir.rs`: clamp header `divisions` to ≥ 1, clamp time-sig
  denominator-power shifts, and break on a zero-length measure (a `0/N` time sig
  or `0`-tpq header was an infinite loop → OOM in `compute_measure_boundaries`).
- `representations/{note_array,piano_roll,metrics}.rs`: saturating `onset+duration`,
  a `MAX_STEPS` cap on the piano-roll allocation, and `MAX_METRIC_SLOTS` caps on
  the beat/groove scratch buffers; `groove_consistency`'s `assert!(measure_resolution≥1)`
  panic is now a `NaN` return.
- Tests: extreme dots stay finite; `max_time_shift=0`/`velocity_bins=0` terminate;
  `0/4` time sig and `0`-tpq MIDI header don't hang; metrics bounded on huge
  durations. 526 lib tests green; default-target `clippy -D warnings` clean.

**Phase 2b — Recursion-depth guard (stack-overflow DoS).** Deeply nested
LilyPond (`{{{ … }}}` thousands deep) overflowed the IR walk's stack and
*aborted the process* (a stack overflow is not a catchable panic; via PyO3 it
killed the host interpreter). `parser.rs` now rejects input whose tree-sitter
tree exceeds `MAX_NESTING_DEPTH` (2000 — astronomically above any real score,
well below the recursion ceiling) with `ParseError::TooDeeplyNested`, checked
iteratively (no recursion) at the single parse chokepoint so both the Score and
Music paths are covered. The other recursive consumers are bounded transitively:
the post-parse lift inherits this guard, and `from_json` is capped by
serde_json's built-in recursion limit. Test: 8000-deep braces → clean error
(no overflow); normal nesting unaffected. 527 lib tests green.

**Phase 2c — Panic firewall + overflow checks + batch isolation.**
- `mxml_to_ir`: the vendored `musicxml` 1.1.2 ZIP reader can panic (an
  out-of-bounds slice) on a crafted `.mxl` central directory; both read paths are
  now wrapped in `catch_unwind` so that panic becomes an `AdapterError::Parse`
  instead of crossing the PyO3 boundary as an opaque `PanicException`.
- `Cargo.toml`: `overflow-checks = true` for `[profile.release]` — a missed
  integer overflow on attacker-controlled durations/ticks now panics (caught at
  the FFI boundary) rather than silently wrapping into corrupt output in wheels.
- `main.rs`: batch conversion wraps each file in `catch_unwind`
  (`process_one_file_caught`) so one pathological file fails only itself instead
  of unwinding out of the rayon worker and aborting the whole run.
- **Phase 2c residual now fixed (bounded unzip).** `.mxl` is a ZIP archive;
  previously lytk handed the raw bytes to the `musicxml` crate, whose ZIP reader
  has no decompressed-size cap (decompression-bomb OOM) and an out-of-bounds read
  on crafted offsets. lytk now decompresses `.mxl` itself via the `zip` crate
  (deflate only) with a **256 MiB decompressed cap** and a 64 MiB input cap,
  selecting the root part from `META-INF/container.xml` (fallback: first
  non-`META-INF` `.xml`), then hands plain XML to the `musicxml` crate — bypassing
  its ZIP path entirely, so both the bomb and the OOB read are gone. Tests build a
  small `.mxl` and assert it's rejected under a tiny cap and decompresses under an
  ample one; plain XML passes through untouched. All 10 `.mxl` fixtures still
  parse.

**Phase 0 — Parser fuzz net.** New `tests/fuzz_inputs.rs`: proptest feeds random
text/bytes to every `ToIr` parser (LilyPond, MusicXML/MXL incl. the zip path,
MIDI, ABC) asserting they never panic/hang/OOM, plus targeted regression cases
for the audit's crafted crashers (deep nesting, `M:4/0`, `0`-tpq MIDI, garbage).
The net immediately caught a residual panic Phase 2a had missed: ABC `M:4/0`
still reached `Frac::new(_, 0)` — `parse_meter` now rejects a zero numerator or
denominator (mirroring `parse_fraction`), so the meter is dropped instead of
crashing.

**Phase 0/3 — Fidelity scoreboard extended.** The gate was duration/onset-blind
(pitch-multiset only) — exactly why the tuplet/grace bugs hid behind an unchanged
pitch set. Added a `(onset, duration, pitch)` **note signature** (via the
note-array) and two new round-trip directions (**ABC** and **MIDI**) alongside
LY and XML, each gated on a committed non-decreasing baseline. Current floors:
LY 35/35 note+pitch, 27/35 onset+dur (8 complex multi-voice fixtures still drift,
now gated so they can't get worse); XML **152/152/152** (full onset+duration
fidelity); ABC 3/3/3; MIDI measured & reported but not yet gated above 0
(midi→midi re-imports with different bar-splitting, so note counts shift — the
sound is preserved, the notation isn't note-for-note stable; a known limitation).

**MIDI → IR → MIDI round-trip: multi-voice reconstruction.** `midi_to_ir`
previously flattened a part's overlapping notes into a single voice (lossy and
non-idempotent for polyphony), and independent per-note quantization over-filled
bars (durations summed past the time signature), so the round-trip drifted. Now:
- `assign_voices` reconstructs independent **monophonic voices** by greedy
  interval colouring (chords — identical onset+offset — stay in one voice), so
  overlapping notes keep their true onsets and each voice fills its own bar. A
  monophonic part still collapses to one voice (no behavior change).
- the extracted `build_voice_elements` caps each note/rest to the bar's remaining
  **quantized budget** (`quantize_capped`) so rounding can't over-fill a bar; the
  trailing rest is decided in quantized ticks and sub-grid remainders are dropped
  consistently — making the import a fixed point of export∘import for the simple
  fixtures. (The IR→MIDI exporter already overlays voices correctly.)

Result: the MIDI scoreboard goes **0/5 → 2/5 on all three metrics**; the 3
hardest multi-voice piano fixtures (example2_1, chopin_n, pedal) still drift on
cross-measure tie/tuplet interactions and remain a documented limitation, gated
at the 2/5 floor. Tests: `assign_voices` splits overlap / keeps chords / keeps
sequential; a multi-part MIDI round-trips with an identical sounding note-array.
No regressions.

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
