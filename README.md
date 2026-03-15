# lytk

A Rust library with Python bindings for large-scale music notation data augmentation. It processes LilyPond, MusicXML, MIDI, and other symbolic music formats through a shared Internal Representation (IR), applying transformations for dataset creation and augmentation.

## Architecture

The project is organized in five layers:

```
1. Parser        tree-sitter-lilypond grammar → parse tree
2. IR            Score → Part(Group) → Voice → Measure → Note/Rest/…
3. Transforms    Idempotent, composable passes on the IR
4. Adapters      from_ir / to_ir per format (ly, mxl, midi, abc, …)
5. CLI + lib     rayon-parallel batch CLI; clean public Rust crate + PyO3 bindings
```

### Conversion Pipeline

```
MusicXML (.xml/.mxl) ──→ IR (Score tree) ──→ LilyPond (.ly)
LilyPond (.ly)       ──→ IR (Score tree) ──→ MusicXML (.xml)
```

The IR-based pipeline supports MusicXML 4.0 features including: notes, rests, chords, key/time/clef signatures, articulations, dynamics, slurs, ties, tuplets, grace notes, cue notes, lyrics, multi-voice/staff parts, part groups, directions (tempo, rehearsal marks, octave shifts, pedal, text), barlines with repeats and endings.

### Module Layout

| Path | Purpose |
|---|---|
| `src/ir/` | Internal Representation — `Score`, `Part`, `Voice`, `Measure`, `Note`, `Pitch`, `Duration`, etc. |
| `src/parser.rs` | tree-sitter LilyPond parser wrapper |
| `src/adapters/` | Format adapters — `MxmlToIrAdapter`, MXL ZIP handler |
| `src/transforms/` | `Transform` trait and composable passes |
| `src/lib.rs` | Library root and PyO3 module |
| `src/main.rs` | CLI entry point (stub) |
| `src/lytk/` | Python package installed as `import lytk` |

### Current Status

**Implemented:**
- Complete IR layer with `Score` → `Part` → `Voice` → `Measure` → `Note`/`Rest` tree, plus `Pitch`, `Duration`, `Articulation`, `Direction`, `Barline`, `Clef`, `KeySignature`, `TimeSignature`, `Transpose`, `Lyric`, pitch language data for 11 languages
- tree-sitter LilyPond parser wrapper
- MusicXML → IR adapter (parses all 143 MusicXML test suite fixtures)
- MXL (ZIP) archive extraction
- Transform trait with composition helpers
- 52 unit tests + 1 doc test, all passing

**Not yet implemented:** LilyPond → IR adapter, IR → LilyPond emitter, IR → MusicXML adapter, MIDI adapter, individual transforms (transpose, language change, etc.), CLI commands, Python bindings beyond stub.

See [docs/design.md](docs/design.md) for detailed architecture and [docs/roadmap.md](docs/roadmap.md) for planned work.

## Build & Test

See [docs/development.md](docs/development.md) for the full guide including prerequisites, debug vs release builds, test filtering, fixture tests, and CLI usage.

```bash
# Rust — compile and test
cargo build
cargo test

# Python extension — dev install (rebuilds Rust on change)
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
| `lytk-py/` | Python prototype — working IR + mxl↔ly pipeline (read-only reference) |
| `python-ly/` | LilyPond library from Frescobaldi (lexer, tokenizer, transposer) |
| `quickly/` | Successor of python-ly with improved API |
| `lilypond/` | The LilyPond compiler source |
| `tree-sitter-lilypond/` | tree-sitter grammars for LilyPond |
| `lily/` | Reference code from LilyPond's `musicxml2ly` plugin (legacy) |
| `lilypond-export/` | LilyPond plugin for MusicXML/Humdrum export |
| `lilybert/` | BERT model with LilyPond tokenizer and data augmentations |
| `MEILER/` | MEI to LilyPond converter |
| `lilypond-midi-input/` | Rust library for real-time MIDI to LilyPond |
| `abjad/` | Python package to programmatically create LilyPond scores |
| `lilypond-rs/` | Rust crate inspired by abjad |

## Development

TDD — every feature must have tests. Follow DRY, modularity, and composability. Performance matters for large-scale processing.

See [docs/design.md](docs/design.md) for design decisions and coding conventions.

## License

GNU General Public License v2.0
