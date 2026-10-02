# Python API reference

*Generated from `src/lytk/_core.pyi` and `src/lytk/datasets/` by
`python scripts/python_api.py`; a test fails when this file is out of date.*

`import lytk` gives everything below; `lytk.__version__` is the version.

- **Errors.** Readers (`from_*`, `Score.from_json`/`from_dict`,
  `MusicDocument.from_json`, `flatten`) raise `lytk.ParseError`, a
  `ValueError`, when the input cannot be read, and `OSError` when a file
  cannot be opened. `strict=True` LilyPond readers raise
  `lytk.LilyPondSyntaxError`, a `ParseError`, when the reading has an error.
  Writers (`to_*`) raise `ValueError` when a score cannot be written. Any
  function may raise `lytk.InternalError`, a bug in lytk.
- **Positions** (`Diagnostic`, `HeaderField`, `Token`) are character
  offsets: `text[d.start:d.end]` is the span.
- **One LilyPond parse.** `from_lilypond_string` and
  `from_lilypond_music_string` (and their file twins) run the same parse,
  walk and assembly: the Music tree of `MusicDocument` is lifted from the
  `Score`. Whatever one accepts, the other accepts too.

## Errors and diagnostics

### class `LytkError(Exception)`

Base class of the errors lytk raises itself.

### class `ParseError(LytkError, ValueError)`

The input could not be read: malformed, not the format expected, or
past the reader's bounds. A ValueError too.

### class `LilyPondSyntaxError(ParseError)`

LilyPond read with ``strict=True`` has errors.

### class `InternalError(LytkError)`

A bug in lytk: the Rust code panicked. Every function and method that
reads, writes or transforms music raises this instead of letting a Rust
panic through; the message names the panic and where it happened.

### class `Diagnostic`

One finding of the LilyPond reader. Comparable, hashable, picklable.

#### `Diagnostic(severity: str, code: str, message: str, line: int, column: int, start: int, end: int)`

#### `Diagnostic.severity: str`

``"error"``: LilyPond rejects the input; ``"warning"``: lytk does not
read it, or cannot represent it, and reads around it.

#### `Diagnostic.code: str`

A stable identifier: ``syntax-error``, ``missing-token``,
``invalid-duration``, ``invalid-ratio``, ``not-lilypond``,
``too-large``, ``invalid-version`` (errors); ``unknown-command``, ``unrecognized-token``,
``ignored-include``, ``unknown-language``, ``dropped-music``,
``skipped-score``, ``unsupported-value`` (warnings).

#### `Diagnostic.message: str`

#### `Diagnostic.line: int`

1-based.

#### `Diagnostic.column: int`

1-based, in characters.

#### `Diagnostic.start: int`

Character offset: ``text[d.start:d.end]`` is the span.

#### `Diagnostic.end: int`

### `check_lilypond(text: str, *, semantic: bool=False, include_paths: list[str] | None=None) -> list[Diagnostic]`

Diagnostics of LilyPond text, in source order: the syntax only (fast,
nothing is read), or with ``semantic=True`` what a reading reports too.
Never raises for bad input: input too large to read is a ``too-large``
error. *include_paths* follows includes as the readers do; an included
file that cannot be read raises ``OSError``.

### class `HeaderField`

A field of a LilyPond ``\header`` block (see :func:`header_fields`).

#### `HeaderField.key: str`

#### `HeaderField.value: str`

The value as text: strings decoded, ``\markup`` as its words,
``#"…"`` as its string.

#### `HeaderField.start: int`

Character offset: ``text[f.start:f.end]`` is the whole ``key = value``.

#### `HeaderField.end: int`

#### `HeaderField.score: int | None`

Index of the ``\score`` block the field is in (file order), or
``None`` at the top level.

### `header_fields(text: str) -> list[HeaderField]`

Every ``\header`` field of LilyPond text, from the parse tree alone
(nothing is read); fields with values other than text (``##f``) are left
out.


## LilyPond \version

### class `LilyPondVersion`

A LilyPond version, compared numerically (``2.24`` equals ``2.24.0``).
Accepted as LilyPond 2.24 accepts it: ``major.minor.patch`` with an
optional fourth part (kept, not compared), or ``major.minor`` with an even
minor. Other strings raise :class:`ParseError`. Hashable, picklable.

#### `LilyPondVersion(text: str)`

#### `LilyPondVersion.major: int`

#### `LilyPondVersion.minor: int`

#### `LilyPondVersion.patch: int`

#### `LilyPondVersion.extra: str | None`

The fourth part, if any.

### `lilypond_version(text: str) -> LilyPondVersion | None`

The version the first ``\version`` statement states, from the parse
tree (a commented-out one does not count); *None* when there is none or
it is invalid (``check_lilypond`` reports ``invalid-version``).

### `set_lilypond_version(text: str, version: LilyPondVersion | str) -> str`

*text* with every ``\version`` statement stating *version* (a string
is written as given, once checked), or with one added at the top.

### `strip_lilypond_version(text: str) -> str`

*text* without its ``\version`` statements.


## LilyPond tokens

### class `Token`

A token of LilyPond text (see :func:`tokenize`). Hashable.

#### `Token.kind: str`

``comment``, ``string``, ``command``, ``symbol``, ``number``,
``fraction``, ``punctuation``, ``scheme`` (the ``#`` or ``$`` that
starts embedded Scheme), ``boolean``, ``character``, ``keyword``
(Scheme's) or ``error``.

#### `Token.scheme: bool`

Whether the token is Scheme rather than LilyPond.

#### `Token.text: str`

#### `Token.start: int`

Character offset: ``text[t.start:t.end]`` is the token.

#### `Token.end: int`

#### `Token.line: int`

1-based.

#### `Token.column: int`

1-based, in characters.

### `tokenize(text: str) -> list[Token]`

The tokens of LilyPond text, from the parse tree; strings and comments
whole, embedded Scheme as Scheme tokens. Every character but whitespace
is in exactly one token, ``error`` tokens holding what the grammar cannot
tokenize.

### `strip_comments(text: str) -> str`

*text* without its comments, LilyPond's and embedded Scheme's.


## Statistics

### `info(score: Score) -> dict[str, Any]`

A score's metadata and counts, as ``lytk info --json`` prints them:
``title`` … ``language``, ``lilypond_version``; ``part_count``,
``note_count``, ``voice_count``, ``bar_count``, ``duration_quarters``,
``lyric_count``, ``chord_symbol_count``, ``grace_note_count``; and
``parts``, a dict per part.

### `source_stats(text: str) -> dict[str, int]`

Counts of LilyPond source text from its tokens: ``bytes``, ``lines``,
``tokens``, ``comments``, ``scheme``, ``error_tokens``.


## Scores

### class `MusicDocument`

Opaque handle to a parsed music document (Layer 1 Music tree).

#### `MusicDocument.title: str | None`

#### `MusicDocument.composer: str | None`

#### `MusicDocument.subtitle: str | None`

#### `MusicDocument.arranger: str | None`

#### `MusicDocument.language: str | None`

#### `MusicDocument.diagnostics: list[Diagnostic]`

What reading LilyPond reported (empty for other sources).

#### `MusicDocument.lyricist: str | None`

#### `MusicDocument.lilypond_version: LilyPondVersion | None`

As :attr:`Score.lilypond_version`.

#### `MusicDocument.header: dict[str, str]`

Every header field, as :attr:`Score.header`.

#### `MusicDocument.notes(resolution: int=480) -> list[tuple[int, int, int, int]]`

Notes as ``(onset, duration, pitch, velocity)`` tuples in time steps.

#### `MusicDocument.to_json() -> str`

#### static `MusicDocument.from_json(json: str) -> MusicDocument`

#### `MusicDocument.to_score() -> Score`

### class `Score`

Opaque handle to a parsed music score.

#### `Score.title: str | None`

#### `Score.composer: str | None`

#### `Score.subtitle: str | None`

#### `Score.arranger: str | None`

#### `Score.lyricist: str | None`

#### `Score.lilypond_version: LilyPondVersion | None`

The version the source's ``\version`` states, for a score read
from LilyPond that states a valid one.

#### `Score.language: str | None`

#### `Score.num_parts: int`

#### `Score.parts: list[str]`

#### `Score.diagnostics: list[Diagnostic]`

What reading LilyPond reported (empty for other sources); not part
of ``to_dict``/``to_json``.

#### `Score.header: dict[str, str]`

Every header field: ``title``, ``subtitle``, ``composer``,
``arranger`` and ``lyricist`` (LilyPond's ``poet``) when set, then the
others (``copyright``, ``opus``, ``texidoc``, …) by key.

#### `Score.to_json() -> str`

#### static `Score.from_json(json: str) -> Score`

#### `Score.to_dict() -> dict[str, Any]`

#### static `Score.from_dict(dict: dict[str, Any]) -> Score`

#### `Score.to_music_document() -> MusicDocument`

#### `Score.notes(resolution: int=480) -> list[tuple[int, int, int, int]]`

Notes as ``(onset, duration, pitch, velocity)`` tuples in time steps.

#### `Score.iter_parts() -> list[Part]`

Structured navigation: parts as typed :class:`Part` objects.


## Structured note navigation (read-only Layer-2 tree)

### class `Part`

An instrument part — a sequence of measures (read-only).

#### `Part.name: str`

#### `Part.abbreviation: str`

#### `Part.part_id: str`

#### `Part.midi_program: int`

#### `Part.midi_instrument: str`

#### `Part.midi_channel: int`

#### `Part.staves: int`

#### `Part.measures: list[Measure]`

#### `Part.notes: list[Note]`

Every sounding note in the part.

### class `Measure`

A measure / bar (read-only).

#### `Measure.number: int`

#### `Measure.number_label: str | None`

#### `Measure.implicit: bool`

#### `Measure.senza_misura: bool`

#### `Measure.time_signature: tuple[str, int, str | None] | None`

``(beats, beat_type, symbol)`` if set at this measure, else None.

#### `Measure.key_signature: tuple[int, str] | None`

``(fifths, mode)`` if set at this measure, else None.

#### `Measure.voices: list[Voice]`

#### `Measure.notes: list[Note]`

### class `Voice`

A voice within a measure (read-only).

#### `Voice.number: int`

#### `Voice.elements: list[Note | Rest | Chord]`

#### `Voice.notes: list[Note]`

Every sounding note (chord members flattened).

### class `Note`

A single pitched note (read-only).

#### `Note.pitch: Pitch`

#### `Note.midi: int`

#### `Note.duration: float`

Duration in quarter-note lengths.

#### `Note.duration_fraction: tuple[int, int]`

Exact duration as ``(numerator, denominator)`` of a whole note.

#### `Note.voice: int`

#### `Note.staff: int`

#### `Note.is_grace: bool`

#### `Note.velocity: int | None`

MIDI velocity the note was played with, when known.

#### `Note.ties: list[str]`

Tie events: ``"start"`` / ``"stop"`` / ``"continue"``.

#### `Note.articulations: list[str]`

#### `Note.lyrics: list[str]`

#### `Note.lyric_syllables: list[dict[str, object]]`

The note's syllables with their verse: dicts of ``verse``, ``name``,
``text``, ``syllabic`` (``"single"``, ``"begin"``, ``"middle"``,
``"end"``), ``extend`` and ``elision`` (words sung on one note, joined
with ``‿``).

### class `Rest`

A rest or spacer (read-only).

#### `Rest.duration: float`

#### `Rest.duration_fraction: tuple[int, int]`

#### `Rest.voice: int`

#### `Rest.staff: int`

#### `Rest.is_measure_rest: bool`

#### `Rest.is_spacer: bool`

### class `Chord`

A chord — notes sounding together (read-only).

#### `Chord.duration: float`

#### `Chord.duration_fraction: tuple[int, int]`

#### `Chord.voice: int`

#### `Chord.staff: int`

#### `Chord.notes: list[Note]`

### class `Pitch`

A note's pitch (read-only).

#### `Pitch.step: str`

Diatonic step name, e.g. ``"C"``.

#### `Pitch.alter: int`

Chromatic alteration in semitones (1 = sharp, -1 = flat).

#### `Pitch.octave: int`

#### `Pitch.midi: int`

MIDI note number (middle C = 60).

#### `Pitch.name: str`

Scientific pitch name, e.g. ``"C#4"``.


## Adapters

### `from_musicxml(path: str) -> Score`

### `from_musicxml_string(xml: str) -> Score`

### `from_musicxml_bytes(data: bytes) -> Score`

Parse MusicXML or compressed MXL from in-memory bytes.

### `from_lilypond(path: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> Score`

The first movement; ``strict=True`` raises LilyPondSyntaxError on an
error. The diagnostics are in ``Score.diagnostics`` either way. With
*include_paths*, ``\include`` statements are followed (the file's directory,
then the paths); a diagnostic in an included file is reported at its
``\include``, and one not found is an ``ignored-include`` warning.

### `from_lilypond_string(text: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> Score`

### `from_lilypond_movements(path: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> list[Score]`

Every movement of a LilyPond file: one score per ``\score`` block and
per top-level music expression, each with the file's diagnostics.

### `from_lilypond_movements_string(text: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> list[Score]`

Every movement of LilyPond text, each with the text's diagnostics.

### `from_lilypond_music(path: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> MusicDocument`

### `from_lilypond_music_movements(path: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> list[MusicDocument]`

Every movement of a LilyPond file as a Music tree, each with the
file's diagnostics.

### `from_lilypond_music_string(text: str, *, language: str | None=None, strict: bool=False, include_paths: list[str] | None=None) -> MusicDocument`

### `from_abc(path: str) -> Score`

### `from_humdrum(path: str) -> Score`

### `from_abc_string(text: str) -> Score`

### `from_abc_tunes(path: str) -> list[Score]`

Every tune of an ABC file, one score each (``from_abc`` reads the first).

### `from_abc_tunes_string(text: str) -> list[Score]`

Every tune of an ABC string, one score each.

### `from_humdrum_string(text: str) -> Score`

### `to_lilypond(score: Score, path: str | None=None, *, language: str | None=None, relative: bool | None=None, version: LilyPondVersion | str | None=None) -> str`

*relative*: ``True`` for ``\relative`` entry, ``False`` for absolute,
``None`` for the score's own.

### `to_lilypond_music(doc: MusicDocument, path: str | None=None, *, version: LilyPondVersion | str | None=None) -> str`

### `to_musicxml(score: Score, path: str | None=None) -> str`

### `to_mxl_bytes(score: Score) -> bytes`

Serialize a score to compressed MusicXML (a ZIP archive).

### `to_abc(score: Score, path: str | None=None) -> str`

### `to_humdrum(score: Score, path: str | None=None) -> str`


## LilyPond \include flattening

### `flatten(input: str, output: str | None=None, *, include_paths: list[str] | None=None, add_markers: bool=True) -> str`

### `flatten_string(text: str, *, base_dir: str | None=None, include_paths: list[str] | None=None, add_markers: bool=True) -> str`

Expand the ``\include`` directives of LilyPond text (relative ones
against *base_dir*, then *include_paths*), found on the parse tree.
ParseError for a missing file (LilyPond's own, like ``english.ly``, stay
as they are), a circular include or an expansion past the bounds.


## Transforms

### `transpose(music: _M, semitones: int) -> _M`

### `transpose_interval(music: _M, interval: str) -> _M`

### `transpose_to_key(music: _M, key: str) -> _M`

### `change_language(music: _M, language: str) -> _M`

### `invert(music: _M, *, step: str='C', alter: int=0, octave: int=4) -> _M`

### `retrograde(music: _M) -> _M`


## ML representations (Epic D)

### `to_note_array(doc: MusicDocument, resolution: int=480, *, pitch: str='sounding') -> npt.NDArray[np.int32]`

Encode a document as a ``(N, 4)`` array: (onset, duration, pitch, velocity).

Pitches sound as played (a B♭ clarinet's written D is a C) unless
``pitch="written"``. Velocities are those ``to_midi`` plays (LilyPond's
dynamics table, 90 without a dynamic).

### `from_note_array(array: npt.NDArray[np.int32], resolution: int=480) -> MusicDocument`

Decode a ``(N, 4)`` note array back into a document.

### `to_event_sequence(doc: MusicDocument, resolution: int=480, max_time_shift: int=100, velocity_bins: int=32, encode_velocity: bool=True) -> npt.NDArray[np.int64]`

Encode a document as a 1-D event-code sequence (Performance-RNN style).

### `from_event_sequence(array: npt.NDArray[np.int64], resolution: int=480, max_time_shift: int=100, velocity_bins: int=32, encode_velocity: bool=True) -> MusicDocument`

Decode an event-code sequence back into a document.

### `to_piano_roll(doc: MusicDocument, resolution: int=480, encode_velocity: bool=True) -> npt.NDArray[np.uint8]`

Encode a document as a ``(T, 128)`` piano-roll matrix.

### `from_piano_roll(array: npt.NDArray[np.uint8], resolution: int=480, encode_velocity: bool=True) -> MusicDocument`

Decode a ``(T, 128)`` piano-roll matrix back into a document.

### `compute_metrics(doc: MusicDocument, resolution: int=480, measure_resolution: int | None=None) -> dict[str, Any]`

Compute objective evaluation metrics (NaN where undefined).

Keys: n_pitches_used, n_pitch_classes_used, pitch_range,
pitch_class_histogram, pitch_entropy, pitch_class_entropy, polyphony,
polyphony_rate, empty_beat_rate, scale_consistency, groove_consistency.


## MIDI

### `from_midi(path: str, *, quantize: int | None=None, swing: bool | None=None) -> Score`

Parse a Standard MIDI File. ``quantize`` (4, 8, 16 or 32) is the
shortest plain note value a played file is snapped to; ``swing`` reads
swung eighths as straight ones marked "Swing" (``True``), never
(``False``), or when a played file swings (``None``).

### `from_midi_bytes(data: bytes, *, quantize: int | None=None, swing: bool | None=None) -> Score`

Parse a Standard MIDI File from in-memory bytes.

### `to_midi(score: Score, path: str, *, unfold_repeats: bool=True) -> None`

Write a score to a Standard MIDI File, playing repeats out unless
``unfold_repeats=False``.

### `to_midi_bytes(score: Score, *, unfold_repeats: bool=True) -> bytes`

Serialize a score to Standard MIDI File bytes.


## Datasets (`lytk.datasets`)

### class `Dataset`

Abstract base: an indexable collection of :class:`MusicDocument` objects.

Subclasses implement :meth:`__len__` and :meth:`__getitem__`. Everything
else (representation conversion, metrics, splits, torch/tf adapters) is
built on top of those two.

``on_error`` says what iterating does with an item that cannot be read:
``"raise"`` (the default) raises, ``"skip"`` leaves it out and records it
in :attr:`errors`, ``"warn"`` does the same and warns. Iteration covers
``for doc in ds``, :meth:`iter_representation`, :meth:`to_representation`,
:meth:`metrics` and the TensorFlow adapters; indexing (``ds[i]``, the
torch adapters) always raises for such an item.

#### `Dataset.ids: list[str]`

An identifier per item, in order (here the index as a string).

#### `Dataset.errors: dict[str, str]`

The items iteration skipped: id → ``"ErrorType: message"``.

#### `Dataset.iter_representation(representation: str, **kwargs: Any) -> Iterator[np.ndarray]`

Lazily convert each item to ``representation``, one array at a time.

``representation`` is one of ``"note_array"``, ``"event_sequence"``,
``"piano_roll"``; ``**kwargs`` are forwarded to the converter. Unlike
:meth:`to_representation`, this never materialises the whole dataset in
memory, so it scales to large folders. When a ``cache_dir`` is set the
per-item cache is consulted as each item is produced.

#### `Dataset.to_representation(representation: str, **kwargs: Any) -> list[np.ndarray]`

Eagerly convert every item to ``representation`` (one array per item
that can be read).

This materialises the whole dataset; use :meth:`iter_representation`
for a memory-bounded stream over large corpora.

#### `Dataset.to_note_arrays(resolution: int=480) -> list[np.ndarray]`

#### `Dataset.to_event_sequences(**kwargs: Any) -> list[np.ndarray]`

#### `Dataset.to_pianorolls(resolution: int=480, encode_velocity: bool=True) -> list[np.ndarray]`

#### `Dataset.metrics(resolution: int=480, measure_resolution: int | None=None) -> list[dict[str, Any]]`

Compute objective metrics for every item that can be read.

#### `Dataset.split(ratios: Sequence[float]=(0.8, 0.1, 0.1), seed: int=0, *, groups: Any=None) -> tuple['Subset', ...]`

Deterministically partition into subsets by the given ratios.

Returns one :class:`Subset` per ratio (e.g. train/val/test). With
``groups`` (one key per item, or a function of the item's id), items of
a group land in the same subset: the groups are shuffled and each goes
to the subset furthest below its share of the items.

#### `Dataset.to_pytorch_dataset(representation: str='note_array', *, return_ids: bool=False, **kwargs: Any)`

Return a ``torch.utils.data.Dataset`` yielding tensors of the
chosen representation, or ``(tensor, id)`` pairs with ``return_ids``.
Requires PyTorch.

Conversion is lazy: each item is converted (and cached, if a
``cache_dir`` is set) only when it is indexed, so constructing the
dataset does not materialise the whole corpus in memory.

#### `Dataset.to_tensorflow_dataset(representation: str='note_array', *, return_ids: bool=False, **kwargs: Any)`

Return a ``tf.data.Dataset`` of the chosen representation, or of
``(tensor, id)`` pairs with ``return_ids``. Requires TensorFlow.

The dataset streams items through a generator, converting (and
caching, if a ``cache_dir`` is set) one item at a time rather than
building the whole corpus up front. Items that cannot be read are
skipped or raise as ``on_error`` says.

#### `Dataset.to_pytorch_dataloader(representation: str='note_array', *, batch_size: int=1, shuffle: bool=False, pad_value: float=0.0, return_ids: bool=False, representation_kwargs: dict[str, Any] | None=None, **loader_kwargs: Any)`

Return a ready-to-train ``torch.utils.data.DataLoader``.

Like :meth:`to_pytorch_dataset` but batched, with :func:`pad_collate`
wired in so that ``batch_size > 1`` works on ragged scores. Each batch
is a ``(padded, lengths)`` tuple, ``(padded, lengths, ids)`` with
``return_ids``.

Unlike the other methods on this class, ``**kwargs`` here go to the
``DataLoader`` (``num_workers``, ``pin_memory``, ``drop_last``, …);
arguments for the representation converter go in
``representation_kwargs``. Pass your own ``collate_fn`` to override the
padding behaviour.

```python
train, val, test = FolderDataset("corpus/").split()
loader = train.to_pytorch_dataloader(
    "event_sequence", batch_size=32, shuffle=True, num_workers=4
)
for events, lengths in loader:
    ...
```

#### `Dataset.to_tensorflow_dataloader(representation: str='note_array', *, batch_size: int=1, shuffle: bool=False, pad_value: float=0.0, return_ids: bool=False, representation_kwargs: dict[str, Any] | None=None)`

Return a batched ``tf.data.Dataset`` of ``(padded, lengths)`` tuples,
``(padded, lengths, ids)`` with ``return_ids``.

The TensorFlow counterpart of :meth:`to_pytorch_dataloader`: pads each
batch along the ragged first axis with ``padded_batch`` and carries the
true lengths alongside, for the same reason as :func:`pad_collate`
(``0`` is a valid value, so padding is not self-identifying).

```python
loader = train.to_tensorflow_dataloader("piano_roll", batch_size=16)
for rolls, lengths in loader:
    ...
```

### class `FolderDataset(Dataset)`

A dataset over the supported music files found in a directory.

Documents are loaded lazily on access; an item's id is its path relative to
``root``. ``movements="all"`` makes each movement of a file (each
``\score`` of a LilyPond file, each tune of an ABC file) an item.
``language``, ``include_paths`` and ``strict`` are passed to the LilyPond
reader, ``quantize`` to the MIDI one. ``on_error`` is described in
:class:`Dataset`. When ``cache_dir`` is given, converted representations
are cached to ``.npy`` files keyed by the file's content, how it is read,
the lytk version and the conversion parameters (the content of included
files is not part of the key).

#### `FolderDataset(root: str | Path, recursive: bool=True, extensions: Sequence[str] | None=None, cache_dir: str | Path | None=None, *, on_error: str='raise', movements: str='first', language: str | None=None, include_paths: Sequence[str | Path] | None=None, strict: bool=False, quantize: int | None=None)`

#### `FolderDataset.ids: list[str]`

The source's id per item; with ``movements="all"``, followed by
``#`` and the movement's number, from 1.

#### `FolderDataset.filenames: list[str]`

### class `RecordsDataset(Dataset)`

A dataset over records (dicts) holding music as text: each record's
``text_field`` in ``format`` (``"lilypond"``, ``"musicxml"``, ``"abc"`` or
``"humdrum"``), identified by its ``id_field``.

Items keep their record (:meth:`record`), and their id is the record's
(followed by ``#`` and the movement's number with ``movements="all"``).
With ``split_field``, the records carry their own splits: :meth:`split`
returns them instead of re-shuffling by ratio. The other options are as in
:class:`FolderDataset`; the cache is keyed by the record's text.

#### `RecordsDataset(records: Iterable[dict[str, Any]], *, text_field: str='text', id_field: str='id', format: str='lilypond', split_field: str | None=None, cache_dir: str | Path | None=None, on_error: str='raise', movements: str='first', language: str | None=None, include_paths: Sequence[str | Path] | None=None, strict: bool=False)`

#### `RecordsDataset.ids: list[str]`

The source's id per item; with ``movements="all"``, followed by
``#`` and the movement's number, from 1.

#### class method `RecordsDataset.from_records(records: Iterable[dict[str, Any]], **kwargs: Any) -> 'RecordsDataset'`

A dataset over an iterable of records (keywords as the class).

#### class method `RecordsDataset.from_jsonl(path: str | Path, **kwargs: Any) -> 'RecordsDataset'`

A dataset over a JSON Lines file, one record per line (keywords as
the class).

#### `RecordsDataset.record(index: int) -> dict[str, Any]`

The record item ``index`` comes from.

#### `RecordsDataset.split(ratios: Sequence[float] | None=None, seed: int=0, *, groups: Any=None, field: str | None=None)`

The records' own splits, or a ratio split.

With ``field`` (or ``split_field`` given to the dataset, when no
``ratios`` are), returns ``{value: Subset}`` for each value of that
field, in order of first appearance: a curated, decontaminated split
stays as it is. Otherwise splits by ``ratios`` (default
``(0.8, 0.1, 0.1)``) as :meth:`Dataset.split` does; ``groups`` may name
a field whose equal values stay together.

### class `Subset(Dataset)`

A view of a parent dataset restricted to a list of indices. Its items,
ids, cache and errors are the parent's.

#### `Subset(dataset: Dataset, indices: Sequence[int])`

#### `Subset.ids: list[str]`

#### `Subset.on_error: str`

#### `Subset.errors: dict[str, str]`

### `load_document(path: str | Path, *, language: str | None=None, include_paths: Sequence[str] | None=None, strict: bool=False, quantize: int | None=None)`

Load any supported music file as a :class:`MusicDocument` (its first
movement).

LilyPond is parsed directly into the Layer-1 Music tree; every other format
is parsed into a Score and lifted. ``language``, ``include_paths`` and
``strict`` are passed to the LilyPond reader, ``quantize`` to the MIDI one.

### `pad_collate(batch: Sequence[Any], pad_value: float=0.0)`

Collate variable-length representation tensors into one padded batch.

Every representation is ragged along its first axis — note arrays are
``(n_notes, 4)``, event sequences ``(n_events,)``, piano rolls
``(n_frames, 128)`` — and the number of notes/events/frames differs per
score, so ``torch``'s default collate (which stacks) raises
``RuntimeError: stack expects each tensor to be equal size``. This pads the
first axis to the longest item in the batch.

Returns ``(padded, lengths)``, or ``(padded, lengths, ids)`` when the items
are ``(tensor, id)`` pairs (a dataset made with ``return_ids=True``). The
lengths are returned rather than left to be inferred from ``pad_value``,
because the padding value is not reserved: ``0`` is a legitimate event
code, pitch and velocity, so trailing zeros are genuinely ambiguous. Use
them to build a mask or to ``pack_padded_sequence``.

Suitable as a ``collate_fn`` for any ``torch.utils.data.DataLoader``:

```python
from functools import partial
DataLoader(ds, batch_size=8, collate_fn=partial(pad_collate, pad_value=-1))
```

### Constants

- `SUPPORTED_EXTENSIONS`: `['.abc', '.ily', '.kern', '.krn', '.ly', '.mid', '.midi', '.musicxml', '.mxl', '.xml']`
- `TEXT_FORMATS`: `('lilypond', 'musicxml', 'abc', 'humdrum')`
