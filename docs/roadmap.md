# Roadmap

Items are grouped by status. Completed items are kept for reference.

---

## Completed ✅

### IR Layer
Complete `Score → Part → Voice → Measure → Note/Rest/Chord` tree with `Pitch`, `Duration`,
`Articulation`, `Direction`, `Barline`, `Clef`, `KeySignature`, `TimeSignature`, `Lyric`,
`PartGroup`, `PageLayout`, `Harmony`, `FiguredBass`. Pitch language data for all 11 LilyPond
languages. Full serde serialisation.

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
Dual OOP + functional API.

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

---

## In Progress / Near-term

### 1. MIDI — First-Class Dependency

Currently MIDI support is behind `--features midi`. Plan:
- Move `midly` from optional to a regular dependency in `Cargo.toml`
- Remove the `#[cfg(feature = "midi")]` guards from `src/lib.rs` and `src/main.rs`
- Always compile and expose `from_midi`/`to_midi` in Python bindings
- Update `pyproject.toml` to document MIDI support
- Add MIDI to the default CLI format detection (`"mid" | "midi"` in `invert_ext`)
- Add MIDI round-trip tests to the test suite

Motivation: MIDI is fundamental to symbolic music research. Having it as an opt-in
feature is an unnecessary barrier for users.

### 2. music21 Feature Parity

[music21](https://web.mit.edu/music21/) is the standard Python toolkit for Music
Information Retrieval (MIR) but is slow, poorly designed, and frequently buggy. lytk
aims to provide equivalent or superior analytical capabilities with a clean API and
Rust performance.

Planned features (roughly in priority order):

#### 2a. Pitch & Interval Analysis
- `Interval::from_pitches(p1, p2)` — compute named intervals (m3, P5, …)
- `Interval::semitones()` / `Interval::diatonic_steps()`
- Enharmonic equivalence: `Pitch::is_enharmonic(other)`
- `Pitch::from_name("C#4")` / `Pitch::to_name()` with accidentals
- `Scale::from_key(key, mode)` — major, minor, modes (dorian, etc.)
- `Scale::contains(pitch)` / `Scale::degree_of(pitch)`
- `Chord::from_pitches(pitches)` — stack + identify chord quality
- `Chord::inversion()` / `Chord::root()` / `Chord::quality()` (major, minor, dim, aug, …)

#### 2b. Score Analysis
- `Score::key_analysis()` — Krumhansl-Schmuckler key-finding algorithm
- `Score::ambitus()` — pitch range (min, max) per part
- `Score::pitch_histogram()` — pitch class distribution
- `Part::note_density(measure_range)` — notes per beat
- `Measure::beat_strength(note)` — metric weight of each note's onset
- `Score::find_motif(pattern)` — melodic pattern search
- `Score::chords_to_roman_numerals(key)` — harmonic analysis with Roman numerals

#### 2c. Rhythm & Meter
- `Duration::to_quarter_length()` — float representation
- `Duration::from_quarter_length(f64)` — quantise to nearest notatable value
- `TimeSignature::beat_duration()` / `TimeSignature::compound()`
- `Score::tempo_map()` — ordered list of `(measure, beat, bpm)` changes
- `Score::to_offset_seconds(note, tempo_map)` — absolute time of a note

#### 2d. Harmony & Voice Leading
- `Score::soprano_alto_tenor_bass()` — extract SATB voices
- `VoiceLeadingChecker::parallel_fifths(voice1, voice2)` — detect parallel motion
- `VoiceLeadingChecker::parallel_octaves(voice1, voice2)`
- `Score::figured_bass_to_harmony()` — realise figured bass as chord symbols

#### 2e. Melodic Analysis
- `Part::contour()` — refined contour string (Marvin-Laprade)
- `Part::intervals()` → `Vec<Interval>` — melodic interval sequence
- `Part::melodic_profile()` — stepwise vs leap ratio
- `Part::n_grams(n)` — pitch-class n-grams for pattern mining

#### 2f. Data Augmentation Transforms
- `Augment` transform — stretch/compress rhythms by a ratio
- `AddNoise` — randomly alter pitch/rhythm within tolerance (for ML dataset generation)
- `Harmonise` — add a harmonised voice at a given interval
- `Reduce` — remove ornaments and grace notes, keeping only structural notes
- `NormaliseRhythm` — quantise all durations to the nearest grid point
- `SplitMeasures` / `MergeMeasures` — restructure barring

### 3. LilyPond Parser — Remaining Features

| Feature | LilyPond syntax | IR target | Priority |
|---|---|---|---|
| Chord names | `\chordmode { c1 f g }` | `Measure.harmonies` | MEDIUM |
| Figured bass parse | `\figuremode { <6 4> }` | `Measure.figured_bass` | LOW |
| `\partial` in multi-movement | Anacrusis per movement | `ScoreMetadata.partial_duration` | LOW |

### 4. MusicXML Parser — Remaining Gaps

| Feature | Notes | Priority |
|---|---|---|
| `<measure-style>` (multi-rest, slash) | Needs `MeasureStyle` IR type | MEDIUM |
| `<print>` element (new-system, new-page) | Needs `PrintDirective` IR type | MEDIUM |
| `<dashes>` / `<bracket>` spanners | Needs spanner tracking | LOW |
| Non-traditional key signatures | `<key-step>` / `<key-alter>` | LOW |
| `<interchangeable>` time variants | Rare edge case | LOW |

---

## Planned (lower priority)

### 5. Further Format Adapters

- **ABC notation** — `src/adapters/abc_to_ir.rs`, `src/adapters/ir_to_abc.rs`
  (Reference: abc2lilypond, abcmidi)
- **MEI** — `src/adapters/mei_to_ir.rs` (Reference: `MEILER/`)
- **Humdrum** — `src/adapters/hum_to_ir.rs` (Reference: `hum2ly/`)

### 6. Round-Trip Testing

Systematic round-trip tests: `MusicXML → IR → LilyPond → IR → MusicXML`, comparing
semantic equivalence (not byte equality). Use the 143-fixture test suite as input.
Detect regressions in both adapter directions.

### 7. Python Package Distribution

- Publish to PyPI via maturin + GitHub Actions
- Multi-platform wheels (Linux x86_64/aarch64, macOS arm64/x86_64, Windows x86_64)
- `pip install lytk` with no Rust toolchain required
- Stable API with semantic versioning

### 8. LilyPond Toolchain Integration

- `lytk compile` — invoke `lilypond` to render `.ly` → PDF/PNG/SVG/MIDI
- `lytk check` — validate a LilyPond file by attempting compilation
- `lytk convert-ly` — wrap LilyPond's `convert-ly` syntax updater

---

## Out of Scope (v1)

These MusicXML elements have no planned IR representation:
- `<harp-pedals>` / `<accordion-registration>` — hardware-specific notation
- `<scordatura>` — tuning override notation
- `<percussion>` pictogram elements — complex symbol table
- `<image>` — embedded raster images
- `<listen>` / `<listening>` — performance instructions (MusicXML 4.0 new)
- `<staff-divide>` arrow — orchestral condensed score notation
