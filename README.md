# lytk

A fast Rust library (with Python bindings) for symbolic music notation processing,
format conversion, and large-scale data augmentation. It processes LilyPond, MusicXML,
and MIDI through a shared Internal Representation (IR), applying composable transforms
for dataset creation and research.

> **Alternative to music21** — lytk aims to provide the analytical capabilities of
> [music21](https://web.mit.edu/music21/) in a fast, correct, well-designed package.
> music21 is the standard Python toolkit for Music Information Retrieval but suffers
> from slow performance, inconsistent APIs, and frequent bugs. lytk is built in Rust for
> speed and correctness, exposed to Python via zero-overhead bindings.

## Quick Start

```bash
# Rust CLI
cargo build --release
./target/release/lytk convert input.xml -o output.ly      # ly · xml · mxl · abc · midi · krn
./target/release/lytk flatten score.ly -o flat.ly
./target/release/lytk transpose input.ly --semitones 3 -o transposed.ly
./target/release/lytk info input.xml

# Python
pip install lytk            # (once published)
pip install lytk[torch]     # optional: torch dataset adapters (or lytk[tensorflow])
```

```python
import lytk

# Convert — any format to any format, in-memory or to a file.
score = lytk.from_musicxml("input.xml")        # also: from_lilypond, from_midi, from_abc
score = lytk.transpose(score, semitones=3)     # also: invert, retrograde, change_language
lytk.to_lilypond(score, "output.ly")           # also: to_musicxml, to_midi, to_abc
abc_text = lytk.to_abc(score)                   # any emitter returns the string too

# ML representations (NumPy in/out) via the Layer-1 Music tree.
doc = score.to_music_document()
notes = lytk.to_note_array(doc)                 # (N, 4): onset, duration, pitch, velocity
roll = lytk.to_piano_roll(doc)                  # (T, 128)
events = lytk.to_event_sequence(doc)            # 1-D Performance-RNN event codes
metrics = lytk.compute_metrics(doc)             # pitch-class entropy, polyphony, …
```

The Python package ships prebuilt **abi3** wheels (one wheel per platform covers
CPython 3.10+) and full type stubs (`lytk/_core.pyi`).

## Architecture

```
1. Parser        tree-sitter-lilypond grammar → parse tree
2. IR            Score → Part(Group) → Measure → Voice → Note/Rest/…
3. Transforms    Idempotent, composable passes on the IR
4. Adapters      from_ir / to_ir per format (ly, mxml, midi, …)
5. CLI + lib     rayon-parallel batch CLI; Rust crate + PyO3 bindings
```

### Conversion Pipeline

Every format converts to every other through the shared IR:

```
            ┌─ LilyPond (.ly/.ily)
MusicXML ──┤   MusicXML (.xml/.mxl)
(.xml/.mxl) │
LilyPond ──┼─→  IR (Score / Music tree)  ──→  LilyPond · MusicXML · MIDI · ABC · kern
MIDI ──────┤
ABC ───────┤
Humdrum ───┘
```

See [`docs/import-export.md`](docs/import-export.md) for the full per-format
support matrix (what each reader/writer preserves).

### Module Layout

| Path | Purpose |
|---|---|
| `src/ir/` | Internal Representation — `Score`, `Part`, `Voice`, `Measure`, `Note`, `Pitch`, `Duration`, etc. |
| `src/parser.rs` | tree-sitter LilyPond parser wrapper |
| `src/adapters/` | Format adapters: `MxmlToIr`, `IrToMxml`, `LyToIr`, `IrToLy`, `MidiToIr`, `IrToMidi`, `LyFlatten` |
| `src/transforms/` | `Transform` trait and composable passes (transpose, invert, retrograde, language change) |
| `src/lib.rs` | Library root and PyO3 module |
| `src/main.rs` | CLI entry point |
| `src/lytk/` | Python package installed as `import lytk` |

## Current Status

### Implemented

- **IR layer** — complete `Score → Part → Measure → Voice → Note/Rest/Chord` tree with `Pitch`,
  `Duration`, `Articulation`, `Direction`, `Barline`, `Clef`, `KeySignature`, `TimeSignature`,
  `Lyric`, and pitch language data for all 11 LilyPond languages
- **MusicXML → IR** — parses all 143 MusicXML test suite fixtures; supports MusicXML 4.0
  including harmonies, figured bass, page layout, coda/segno, da capo/dal segno
- **LilyPond → IR** — full parser via tree-sitter: notes, rests, chords, tuplets, grace notes,
  ties, slurs, beams, articulations, dynamics, wedges, ornaments, lyrics (`\lyricsto`,
  `\lyricmode`, `\addlyrics`), clef/key/time (incl. compound meters like `3+2/8`),
  `\breve`/`\longa`, repeats with voltas, variables, multi-staff piano scores with shared
  time-signature re-barring, `\autoBeamOff`, glissando, arpeggio, tremolo, after-grace,
  `\markup` text, shorthand symbols, figured bass (`\figuremode`), chord names (`\chordmode`)
- **IR → MusicXML** — full MusicXML 4.0 emitter with all notation elements
- **IR → LilyPond** — emitter with relative pitch, all notation, multi-staff, lyrics, voltas
- **LilyPond flatten** — recursive `\include` expander with circular dependency detection,
  extension fallback, extra search paths (`-I`), and `\version`/`\language`/`\header` normalization
- **Transforms** — `Transpose`, `ChangeLanguage`, `Invert`, `Retrograde`; composable via
  `apply_all`; dual OOP + functional API
- **MIDI adapter** — `midly`-based MIDI → IR and IR → MIDI (always included); simultaneous
  note-ons import as chords, notes crossing a barline are split and tied
- **CLI** — `convert`, `transpose`, `info`, `flatten` subcommands; batch mode with rayon
- **ABC notation adapter** — `from_abc` / `to_abc` reader and writer through the IR,
  exposed in both the Rust CLI and the Python API
- **ML representations** — note-array `(N, 4)`, Performance-RNN event sequences, and
  piano-roll `(T, 128)` encoders, plus objective metrics (pitch-class entropy, polyphony,
  scale consistency, …), all NumPy in/out via the Layer-1 Music tree
- **Dataset loaders** — `lytk.datasets` with a lazy `Dataset`/`FolderDataset`, deterministic
  train/val/test splits, on-disk representation caching, and lazy PyTorch / TensorFlow
  adapters (optional `lytk[torch]` / `lytk[tensorflow]` extras)
- **Python bindings** — full PyO3 API: `from_musicxml`, `from_lilypond`, `from_abc`,
  `to_musicxml`, `to_lilypond`, `to_abc`, `from_midi`, `to_midi`, `flatten`, `transpose`,
  `change_language`, `invert`, `retrograde`, `Score.to_json/dict`, representation
  encoders and metrics
- **Python CLI** — the shipped `lytk` console script mirrors the Rust binary
  (`convert`, `transpose`, `info`, `flatten`) with process-parallel batch conversion
  honoring `--jobs`; batch mode exits non-zero if any file fails
- **876 Rust tests** (514 unit + 362 integration: CLI, fixture regression, property-based,
  round-trip, semantic round-trip, fidelity scoreboard) + 117 Python tests, all passing
- **Semantic fidelity gate** — committed non-decreasing baselines: LilyPond 35/35 and
  MusicXML 152/152 fixtures preserve note counts and pitch multisets on round-trip
- **Criterion benchmarks** — ~52× faster than python-ly for transpose; ~40× for language change

### Not yet implemented

- music21-parity MIR features (see roadmap)
- MEI adapter (deferred past v1.0)
- Humdrum adapter (deferred past v1.0)

### Known limitations

- A cross-staff voice is emitted entirely in its primary staff (no `\change Staff`
  cross-staff beaming yet) — notes are preserved, not duplicated

## Build & Test

See [docs/development.md](docs/development.md) for the full guide.

```bash
# Rust — compile and test
cargo build
cargo test

# Python extension — dev install
uv sync && maturin develop

# Python tests
pytest tests/ -v

# Lint / format
cargo fmt && cargo clippy -- -D warnings
ruff check . && ruff format .
```

## References

| Folder | Description |
|---|---|
| `python-ly/` | LilyPond library from Frescobaldi (lexer, tokenizer, transposer) |
| `quickly/` | Successor of python-ly with improved API |
| `lilypond/` | The LilyPond compiler source |
| `tree-sitter-lilypond/` | tree-sitter grammar for LilyPond |
| `lily/` | Reference code from LilyPond's `musicxml2ly` plugin |
| `lilybert/` | BERT model with LilyPond tokenizer and data augmentation pipeline |
| `MEILER/` | MEI to LilyPond converter |
| `abjad/` | Python package to programmatically create LilyPond scores |
| `lilypond-rs/` | Rust crate inspired by abjad |
| `symusic/` | Fast MIDI parsing in C++ with Python bindings; MIDI IR reference |
| `music21/` | Python musicology toolkit; feature reference for MIR roadmap |
| `lyp/` | LilyPond package manager; reference for include flattening |

## License

GNU General Public License v2.0
