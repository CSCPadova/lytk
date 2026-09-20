# lytk

[![CI](https://github.com/CSCPadova/lytk/actions/workflows/ci.yml/badge.svg)](https://github.com/CSCPadova/lytk/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/lytk.svg)](https://pypi.org/project/lytk/)
[![Python](https://img.shields.io/pypi/pyversions/lytk.svg)](https://pypi.org/project/lytk/)
[![License: GPL-2.0](https://img.shields.io/badge/license-GPL--2.0-blue.svg)](LICENSE)

A fast Python library (Rust core) for symbolic music notation processing,
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
pip install lytk                  # core: every format, transform and representation
pip install "lytk[torch]"         # + PyTorch datasets and data loaders
pip install "lytk[tensorflow]"    # + tf.data datasets and data loaders
pip install "lytk[eval]"          # + generation-evaluation metrics (scipy, FMD)
pip install "lytk[all]"           # everything above
```

Prebuilt **abi3** wheels mean no Rust toolchain and no compile step — one wheel
per platform covers CPython 3.10+. Nothing in the core requires a DL framework;
torch and TensorFlow are imported lazily, only when you call an adapter that
needs them.

Installing also puts a `lytk` command on your PATH:

```bash
lytk convert input.xml -o output.ly      # ly · xml · mxl · abc · midi · krn
lytk flatten score.ly -o flat.ly
lytk transpose input.ly --semitones 3 -o transposed.ly
lytk info input.xml
lytk batch in_dir/ -o out_dir/ -f xml --jobs 8
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

Full type stubs ship too (`lytk/_core.pyi`, plus `py.typed`).

### Datasets and data loaders

`lytk.datasets` turns a directory of scores — `.ly`, `.xml`/`.mxl`, `.mid`,
`.abc`, `.krn` — into a lazy dataset with deterministic splits, on-disk caching
of converted representations, and data loaders for either framework:

```python
from lytk.datasets import FolderDataset

data = FolderDataset("corpus/", cache_dir=".cache")   # loads lazily, caches arrays
train, val, test = data.split((0.8, 0.1, 0.1), seed=0)  # deterministic

loader = train.to_pytorch_dataloader(
    "event_sequence", batch_size=32, shuffle=True, num_workers=4
)
for events, lengths in loader:        # (B, T) padded, plus true lengths
    ...
```

Scores are ragged — every piece has a different number of notes, events and
frames — so torch's default collate cannot stack them. The loader pads the batch
and hands back the true `lengths` alongside, because the padding value is not
reserved (`0` is a valid event code, pitch and velocity), so trailing zeros are
genuinely ambiguous. Use `lengths` for a mask or `pack_padded_sequence`.

TensorFlow works the same way:

```python
loader = train.to_tensorflow_dataloader("piano_roll", batch_size=16)
for rolls, lengths in loader:         # padded_batch of (B, T, 128)
    ...
```

Need your own loader? `lytk.datasets.pad_collate` is usable as a `collate_fn`
directly, and `to_pytorch_dataset()` / `to_tensorflow_dataset()` return plain
unbatched datasets.

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
| `src/lib.rs` | Rust library root |
| `src/python.rs` | PyO3 bindings — the `lytk._core` extension module |
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
- **Dataset loaders** — `lytk.datasets` with a lazy `Dataset`/`FolderDataset` over all
  six input formats, deterministic train/val/test splits, on-disk representation
  caching, and lazily-imported PyTorch / TensorFlow adapters: plain datasets
  (`to_pytorch_dataset`, `to_tensorflow_dataset`) and padded, length-carrying data
  loaders (`to_pytorch_dataloader`, `to_tensorflow_dataloader`, `pad_collate`) for
  ragged scores — optional `lytk[torch]` / `lytk[tensorflow]` extras
- **Python bindings** — full PyO3 API: `from_musicxml`, `from_lilypond`, `from_abc`,
  `to_musicxml`, `to_lilypond`, `to_abc`, `from_midi`, `to_midi`, `flatten`, `transpose`,
  `change_language`, `invert`, `retrograde`, `Score.to_json/dict`, representation
  encoders and metrics
- **Python CLI** — the shipped `lytk` console script mirrors the Rust binary
  (`convert`, `transpose`, `info`, `flatten`) with process-parallel batch conversion
  honoring `--jobs`; batch mode exits non-zero if any file fails
- **1014 Rust tests** (unit + integration: CLI, fixture regression, property-based,
  round-trip, semantic round-trip, fidelity scoreboard) + 139 Python tests, all passing
- **Semantic fidelity gate** — committed non-decreasing baselines: LilyPond 35/35 and
  MusicXML 152/152 fixtures preserve note counts and pitch multisets on round-trip
- **Criterion benchmarks** — ~52× faster than python-ly for transpose; ~40× for language change

### Not yet implemented

- music21-parity MIR features (see roadmap)
- MEI adapter (deferred past v1.0)
- Humdrum spine rearrangement (`*^` / `*v`) — reported as a clear error

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
cargo fmt && cargo clippy --all-targets -- -D warnings
```

## Acknowledgements

lytk's design draws on prior art in symbolic-music tooling. None of these are
dependencies; they were studied as references:

- [LilyPond](https://lilypond.org/) — the notation language and compiler
- [tree-sitter-lilypond](https://github.com/nwhetsell/tree-sitter-lilypond) — the
  grammar vendored in [`src/tree-sitter/`](src/tree-sitter/), MIT licensed
  (© Nathan Whetsell); its notice ships in
  [`src/tree-sitter/LICENSE`](src/tree-sitter/LICENSE)
- [python-ly](https://github.com/frescobaldi/python-ly) /
  [quickly](https://github.com/frescobaldi/quickly) — tokenizer and transposer
  reference, and the Python performance baseline
- [music21](https://web.mit.edu/music21/) — IR design and MIR feature reference
- [abjad](https://abjad.github.io/) — score-building API and visitor patterns
- [muspy](https://github.com/salu133445/muspy) — ML representation conventions
  (note-array, event sequence, piano-roll)
- [symusic](https://github.com/Yikai-Liao/symusic) — fast MIDI IR reference
- [MuseScore](https://musescore.org/) — MusicXML import/export behaviour reference

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security reports: [SECURITY.md](SECURITY.md).

## License

GNU General Public License v2.0 only — see [LICENSE](LICENSE).
