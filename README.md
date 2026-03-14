# lytk

`lytk` is a toolkit for manipulating LilyPond music notation files at scale. It supports bidirectional conversion between MusicXML and LilyPond, midi and lilypond, abc and lilypond, with plans for data augmentation, transposition, and other transformations on large datasets.

## Conversion Pipeline

```
MusicXML → internal representation → .ly text
.ly text → internal representation → MusicXML
```

The IR-based pipeline supports MusicXML 4.0 features including: notes/rests/chords, key/time/clef signatures, articulations, dynamics, slurs, ties, tuplets, grace notes, lyrics, multi-voice/staff parts, part groups, directions (tempo, rehearsal marks, octave shifts, pedal), barlines with repeats and endings.

## References

- `lilypond` — the original lilypond compiler with tree sitter grammar
- `python-ly` — LilyPond library from Frescobaldi (lexer, tokenizer, transposer, language support)
- `quickly` — successor of `python-ly` with improved API
- `lily/` — reference code from LilyPond's original `musicxml2ly` plugin (legacy)
- `lytk-py/` - an example in python of internal representation, with mxl2ly conversion pipeline
- `musicxmlTestSuite` — extensive MusicXML test suite for validation
- `lilypond-export` — LilyPond plugin for MusicXML/Humdrum export
- `lilybert` — BERT model with LilyPond tokenizer and data augmentations
- `MEILER` — MEI to LilyPond converter
- `lilypond-midi-input` — Rust library for real-time MIDI to LilyPond
- `abjad` - is a python package to programmatically create lilypond scores
- `lilypond-rs` - is a rust crate inspired by abjad
- `tree-sitter-lilypond` - is a repo with tree sitter grammars for lilypond and lilypond scheme

## Development

We follow TDD — every feature must be tested. Follow classic design patterns, DRY, modularity, and composability. Performance matters for large-scale processing.

When contributing, write docstrings and consider updating documentation in `docs/`.

## License

GNU General Public License v2.0
