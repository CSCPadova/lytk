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
./target/release/lytk convert input.xml -o output.ly
./target/release/lytk flatten score.ly -o flat.ly
./target/release/lytk transpose input.ly --semitones 3 -o transposed.ly
./target/release/lytk info input.xml

# Python
pip install lytk  # (once published)
```

```python
import lytk

score = lytk.from_musicxml("input.xml")
score = lytk.transpose(score, semitones=3)
lytk.to_lilypond(score, "output.ly")
```

## Architecture

```
1. Parser        tree-sitter-lilypond grammar → parse tree
2. IR            Score → Part(Group) → Voice → Measure → Note/Rest/…
3. Transforms    Idempotent, composable passes on the IR
4. Adapters      from_ir / to_ir per format (ly, mxml, midi, …)
5. CLI + lib     rayon-parallel batch CLI; Rust crate + PyO3 bindings
```

### Conversion Pipeline

```
MusicXML (.xml/.mxl)  ──→  IR (Score tree)  ──→  LilyPond (.ly)
LilyPond (.ly)        ──→  IR (Score tree)  ──→  MusicXML (.xml)
MIDI (.mid)           ──→  IR (Score tree)  ──→  LilyPond / MusicXML
```

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

- **IR layer** — complete `Score → Part → Voice → Measure → Note/Rest/Chord` tree with `Pitch`,
  `Duration`, `Articulation`, `Direction`, `Barline`, `Clef`, `KeySignature`, `TimeSignature`,
  `Lyric`, and pitch language data for all 11 LilyPond languages
- **MusicXML → IR** — parses all 143 MusicXML test suite fixtures; supports MusicXML 4.0
  including harmonies, figured bass, page layout, coda/segno, da capo/dal segno
- **LilyPond → IR** — full parser via tree-sitter: notes, rests, chords, tuplets, grace notes,
  ties, slurs, beams, articulations, dynamics, wedges, ornaments, lyrics (`\lyricsto`,
  `\lyricmode`), clef/key/time, repeats with voltas, variables, multi-staff, `\autoBeamOff`,
  glissando, arpeggio, tremolo, after-grace, `\markup` text, shorthand symbols, figured bass
- **IR → MusicXML** — full MusicXML 4.0 emitter with all notation elements
- **IR → LilyPond** — emitter with relative pitch, all notation, multi-staff, lyrics, voltas
- **LilyPond flatten** — recursive `\include` expander with circular dependency detection,
  extension fallback, extra search paths (`-I`), and `\version`/`\language`/`\header` normalization
- **Transforms** — `Transpose`, `ChangeLanguage`, `Invert`, `Retrograde`; composable via
  `apply_all`; dual OOP + functional API
- **MIDI adapter** — `midly`-based MIDI → IR and IR → MIDI (optional feature, to be made default)
- **CLI** — `convert`, `transpose`, `info`, `flatten` subcommands; batch mode with rayon
- **Python bindings** — full PyO3 API: `from_musicxml`, `from_lilypond`, `to_musicxml`,
  `to_lilypond`, `transpose`, `change_language`, `invert`, `retrograde`, `Score.to_json/dict`
- **255 unit tests** + 15 CLI integration tests + 26 Python tests, all passing
- **Criterion benchmarks** — ~52× faster than python-ly for transpose; ~40× for language change

### Not yet implemented

- MIDI first-class (currently behind `--features midi`; planned as default)
- music21-parity MIR features (see roadmap)
- ABC notation adapter
- MEI adapter
- Humdrum adapter
- Chord name parser (`\chordmode`)

## Build & Test

See [docs/development.md](docs/development.md) for the full guide.

```bash
# Rust — compile and test
cargo build
cargo test

# With MIDI support
cargo build --features midi
cargo test --features midi

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
