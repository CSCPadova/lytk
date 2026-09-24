# lytk

[![CI](https://github.com/CSCPadova/lytk/actions/workflows/ci.yml/badge.svg)](https://github.com/CSCPadova/lytk/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/lytk.svg)](https://pypi.org/project/lytk/)
[![Python](https://img.shields.io/pypi/pyversions/lytk.svg)](https://pypi.org/project/lytk/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**lytk reads and writes symbolic music, and turns it into training data.**
Scores in LilyPond, MusicXML, MIDI, ABC and Humdrum `**kern` go through one
internal representation, so any format converts to any other. The same scores
come out as NumPy note arrays, piano rolls and event sequences, ready for
PyTorch or TensorFlow. The core is written in Rust; you use it from Python or
from the command line.

> **Preview release (0.x).** Conversion, transforms and the ML pipeline are
> tested and ready to use. The API can still change before 1.0: notation marks
> (articulations, ornaments, dynamics) will move from strings to enums.

## Why lytk

- **LilyPond is a first-class format.** lytk reads real LilyPond files, not a
  subset: variables, `\relative`, piano scores with several voices per staff,
  repeats and voltas, cadenzas, lyrics, chord names and figured bass. It writes
  them back out too, in any of LilyPond's 12 note-name languages.
- **Conversions are measured, not assumed.** Every round trip (LilyPond,
  MusicXML, ABC, `**kern`, MIDI) is checked in CI against a test corpus for note
  counts, pitches, onsets and durations. The results can only improve from one
  release to the next.
- **Built for machine learning.** Encoders for note arrays, piano rolls and
  Performance-RNN event sequences, objective metrics from the muspy family,
  folder datasets with deterministic splits and caching, and padded data
  loaders for PyTorch and TensorFlow.
- **Fast.** A Rust core with prebuilt wheels, and a batch converter that uses
  every core. It transposes LilyPond about 50× faster than python-ly.

## Install

```bash
pip install lytk                  # every format, transform and representation
pip install "lytk[torch]"         # + PyTorch datasets and data loaders
pip install "lytk[tensorflow]"    # + tf.data datasets and data loaders
pip install "lytk[eval]"          # + generation-evaluation metrics (scipy, FMD)
pip install "lytk[all]"           # everything above
```

Wheels are prebuilt for Linux, macOS and Windows and cover CPython 3.10 and
newer, so no Rust toolchain is needed. PyTorch and TensorFlow are optional and
only imported when you ask for a loader.

## Quick start

```python
import lytk

score = lytk.from_musicxml("input.xml")      # or from_lilypond, from_midi, from_abc, from_humdrum
score = lytk.transpose(score, semitones=3)   # also invert, retrograde, change_language
lytk.to_lilypond(score, "output.ly")         # or to_musicxml, to_midi, to_abc, to_humdrum
abc = lytk.to_abc(score)                     # without a path, writers return the text
```

Turn a score into arrays:

```python
doc = score.to_music_document()
notes = lytk.to_note_array(doc)        # (N, 4): onset, duration, pitch, velocity
roll = lytk.to_piano_roll(doc)         # (T, 128)
events = lytk.to_event_sequence(doc)   # Performance-RNN event codes
stats = lytk.compute_metrics(doc)      # pitch-class entropy, polyphony, scale consistency, …
```

Each encoder has an inverse (`from_note_array`, …). The arrays are ordinary
NumPy arrays, and NumPy supports [DLPack](https://dmlc.github.io/dlpack/latest/),
so PyTorch, JAX and CuPy can use them without copying:

```python
import torch
tensor = torch.from_dlpack(lytk.to_piano_roll(doc))   # shares the buffer
```

Build a dataset from a folder of scores in any mix of formats:

```python
from lytk.datasets import FolderDataset

data = FolderDataset("corpus/", cache_dir=".cache")
train, val, test = data.split((0.8, 0.1, 0.1), seed=0)

loader = train.to_pytorch_dataloader("event_sequence", batch_size=32, shuffle=True)
for events, lengths in loader:         # padded batch plus each item's true length
    ...
```

Scores differ in length, so each batch is padded. The true lengths come back
alongside it because `0` is a valid event, pitch and velocity, so padding alone
can't tell you where a score ends. `to_tensorflow_dataloader` works the same way.

The package also installs a `lytk` command:

```bash
lytk convert input.xml -o output.ly           # formats are taken from the extensions
lytk convert corpus/ -o out/ -f xml -j 8      # a whole folder, in parallel
lytk transpose input.ly -s 3 -o up.ly
lytk flatten score.ly -o flat.ly              # inline every \include
lytk info input.mxl
```

It also inverts, reverses, changes note-name languages, compares scores
(`lytk diff`) and runs JSON batch jobs; `lytk --help` lists every command and
[docs/cli.md](docs/cli.md) describes them.

## Formats

| Format | Extensions | Read | Write |
|---|---|:---:|:---:|
| LilyPond | `.ly` `.ily` | ✓ | ✓ |
| MusicXML | `.xml` `.musicxml` | ✓ | ✓ |
| Compressed MusicXML | `.mxl` | ✓ | ✓ |
| MIDI | `.mid` `.midi` | ✓ | ✓ |
| ABC | `.abc` | ✓ | ✓ |
| Humdrum `**kern` | `.krn` | ✓ | ✓ |

[docs/import-export.md](docs/import-export.md) lists what each reader and
writer keeps. MusicXML is the most complete; MIDI keeps no slurs,
articulations or lyrics.

## Not there yet

- Tablature, percussion (unpitched notes) and fretboard diagrams.
- MIDI import transcribes the file as it is. It does not yet quantize, separate
  voices, detect tuplets or split the hands of a piano part.
- Non-traditional key signatures, cross-staff notes (`\change Staff`), and
  Humdrum spine splits (`*^`, `*v`). Files that use spine splits are rejected
  with an error.
- The ABC and `**kern` writers produce one stream per staff, so two voices
  sharing a staff are not kept separate.
- MEI is planned after 1.0.

## Documentation

- [Command line](docs/cli.md)
- [What each format reads and writes](docs/import-export.md)
- [Design and internals](docs/design.md)
- [Building from source and running the tests](docs/development.md)
- [Changelog](docs/changelog.md) and [roadmap](docs/roadmap.md)

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md),
and [SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## Acknowledgements

lytk builds on a lot of prior work in music notation software. Its LilyPond
parser uses the
[tree-sitter-lilypond](https://github.com/nwhetsell/tree-sitter-lilypond)
grammar by Nathan Whetsell (MIT, notice in
[`src/tree-sitter/LICENSE`](src/tree-sitter/LICENSE)). Its design also owes a
lot to [LilyPond](https://lilypond.org/),
[python-ly](https://github.com/frescobaldi/python-ly),
[music21](https://web.mit.edu/music21/),
[muspy](https://github.com/salu133445/muspy),
[abjad](https://abjad.github.io/),
[symusic](https://github.com/Yikai-Liao/symusic) and
[MuseScore](https://musescore.org/).

## License

MIT, see [LICENSE](LICENSE). The test fixtures are third-party scores under
their own terms ([tests/fixtures/README.md](tests/fixtures/README.md)).
