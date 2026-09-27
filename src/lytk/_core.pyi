"""Type stubs for the compiled Rust extension (``lytk._core``)."""

from __future__ import annotations

from typing import Any, TypeVar

__version__: str

class LytkError(Exception):
    """Base class of the errors lytk raises itself."""

class InternalError(LytkError):
    """A bug in lytk: the Rust code panicked. Every function and method that
    reads, writes or transforms music raises this instead of letting a Rust
    panic through; the message names the panic and where it happened."""

class ParseError(LytkError, ValueError):
    """The input could not be read: malformed, not the format expected, or
    past the reader's bounds. A ValueError too."""

class LilyPondSyntaxError(ParseError):
    """LilyPond read with ``strict=True`` has errors."""

    diagnostics: list[Diagnostic]
    """Every diagnostic of the reading, errors and warnings."""

class Diagnostic:
    """One finding of the LilyPond reader. Comparable, hashable, picklable."""

    def __init__(
        self,
        severity: str,
        code: str,
        message: str,
        line: int,
        column: int,
        start: int,
        end: int,
    ) -> None: ...

    @property
    def severity(self) -> str:
        """``"error"``: LilyPond rejects the input; ``"warning"``: lytk does not
        read it, or cannot represent it, and reads around it."""
        ...
    @property
    def code(self) -> str:
        """A stable identifier: ``syntax-error``, ``missing-token``,
        ``invalid-duration``, ``invalid-ratio``, ``not-lilypond``,
        ``too-large`` (errors); ``unknown-command``, ``unrecognized-token``,
        ``ignored-include``, ``unknown-language``, ``dropped-music``,
        ``skipped-score``, ``unsupported-value`` (warnings)."""
        ...
    @property
    def message(self) -> str: ...
    @property
    def line(self) -> int:
        """1-based."""
        ...
    @property
    def column(self) -> int:
        """1-based, in characters."""
        ...
    @property
    def start(self) -> int:
        """Character offset: ``text[d.start:d.end]`` is the span."""
        ...
    @property
    def end(self) -> int: ...
    def __str__(self) -> str:
        """``3:12: error: missing `}` [missing-token]``"""
        ...

class Score:
    """Opaque handle to a parsed music score."""

    @property
    def title(self) -> str | None: ...
    @property
    def composer(self) -> str | None: ...
    @property
    def subtitle(self) -> str | None: ...
    @property
    def arranger(self) -> str | None: ...
    @property
    def lyricist(self) -> str | None: ...
    @property
    def language(self) -> str | None: ...
    @property
    def num_parts(self) -> int: ...
    @property
    def parts(self) -> list[str]: ...
    @property
    def diagnostics(self) -> list[Diagnostic]:
        """What reading LilyPond reported (empty for other sources); not part
        of ``to_dict``/``to_json``."""
        ...
    @property
    def header(self) -> dict[str, str]:
        """Every header field: ``title``, ``subtitle``, ``composer``,
        ``arranger`` and ``lyricist`` (LilyPond's ``poet``) when set, then the
        others (``copyright``, ``opus``, ``texidoc``, …) by key."""
        ...
    def to_json(self) -> str: ...
    @staticmethod
    def from_json(json: str) -> Score: ...
    def to_dict(self) -> dict[str, Any]: ...
    @staticmethod
    def from_dict(dict: dict[str, Any]) -> Score: ...
    def to_music_document(self) -> MusicDocument: ...
    def notes(self, resolution: int = 480) -> list[tuple[int, int, int, int]]:
        """Notes as ``(onset, duration, pitch, velocity)`` tuples in time steps."""
        ...
    def iter_parts(self) -> list[Part]:
        """Structured navigation: parts as typed :class:`Part` objects."""
        ...
    def __eq__(self, other: object) -> bool: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class MusicDocument:
    """Opaque handle to a parsed music document (Layer 1 Music tree)."""

    @property
    def title(self) -> str | None: ...
    @property
    def composer(self) -> str | None: ...
    @property
    def subtitle(self) -> str | None: ...
    @property
    def arranger(self) -> str | None: ...
    @property
    def language(self) -> str | None: ...
    @property
    def diagnostics(self) -> list[Diagnostic]:
        """What reading LilyPond reported (empty for other sources)."""
        ...
    @property
    def lyricist(self) -> str | None: ...
    @property
    def header(self) -> dict[str, str]:
        """Every header field, as :attr:`Score.header`."""
        ...
    def notes(self, resolution: int = 480) -> list[tuple[int, int, int, int]]:
        """Notes as ``(onset, duration, pitch, velocity)`` tuples in time steps."""
        ...
    def to_json(self) -> str: ...
    @staticmethod
    def from_json(json: str) -> MusicDocument: ...
    def to_score(self) -> Score: ...
    def __eq__(self, other: object) -> bool: ...
    def __repr__(self) -> str: ...

# -- Structured note navigation (read-only Layer-2 tree) ---------------------

class Pitch:
    """A note's pitch (read-only)."""

    @property
    def step(self) -> str:
        """Diatonic step name, e.g. ``"C"``."""
        ...
    @property
    def alter(self) -> int:
        """Chromatic alteration in semitones (1 = sharp, -1 = flat)."""
        ...
    @property
    def octave(self) -> int: ...
    @property
    def midi(self) -> int:
        """MIDI note number (middle C = 60)."""
        ...
    @property
    def name(self) -> str:
        """Scientific pitch name, e.g. ``"C#4"``."""
        ...
    def __repr__(self) -> str: ...

class Note:
    """A single pitched note (read-only)."""

    @property
    def pitch(self) -> Pitch: ...
    @property
    def midi(self) -> int: ...
    @property
    def duration(self) -> float:
        """Duration in quarter-note lengths."""
        ...
    @property
    def duration_fraction(self) -> tuple[int, int]:
        """Exact duration as ``(numerator, denominator)`` of a whole note."""
        ...
    @property
    def voice(self) -> int: ...
    @property
    def staff(self) -> int: ...
    @property
    def is_grace(self) -> bool: ...
    @property
    def velocity(self) -> int | None:
        """MIDI velocity the note was played with, when known."""
        ...
    @property
    def ties(self) -> list[str]:
        """Tie events: ``"start"`` / ``"stop"`` / ``"continue"``."""
        ...
    @property
    def articulations(self) -> list[str]: ...
    @property
    def lyrics(self) -> list[str]: ...
    def __repr__(self) -> str: ...

class Rest:
    """A rest or spacer (read-only)."""

    @property
    def duration(self) -> float: ...
    @property
    def duration_fraction(self) -> tuple[int, int]: ...
    @property
    def voice(self) -> int: ...
    @property
    def staff(self) -> int: ...
    @property
    def is_measure_rest(self) -> bool: ...
    @property
    def is_spacer(self) -> bool: ...
    def __repr__(self) -> str: ...

class Chord:
    """A chord — notes sounding together (read-only)."""

    @property
    def duration(self) -> float: ...
    @property
    def duration_fraction(self) -> tuple[int, int]: ...
    @property
    def voice(self) -> int: ...
    @property
    def staff(self) -> int: ...
    @property
    def notes(self) -> list[Note]: ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...

class Voice:
    """A voice within a measure (read-only)."""

    @property
    def number(self) -> int: ...
    @property
    def elements(self) -> list[Note | Rest | Chord]: ...
    @property
    def notes(self) -> list[Note]:
        """Every sounding note (chord members flattened)."""
        ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...

class Measure:
    """A measure / bar (read-only)."""

    @property
    def number(self) -> int: ...
    @property
    def number_label(self) -> str | None: ...
    @property
    def implicit(self) -> bool: ...
    @property
    def senza_misura(self) -> bool: ...
    @property
    def time_signature(self) -> tuple[str, int, str | None] | None:
        """``(beats, beat_type, symbol)`` if set at this measure, else None."""
        ...
    @property
    def key_signature(self) -> tuple[int, str] | None:
        """``(fifths, mode)`` if set at this measure, else None."""
        ...
    @property
    def voices(self) -> list[Voice]: ...
    @property
    def notes(self) -> list[Note]: ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...

class Part:
    """An instrument part — a sequence of measures (read-only)."""

    @property
    def name(self) -> str: ...
    @property
    def abbreviation(self) -> str: ...
    @property
    def part_id(self) -> str: ...
    @property
    def midi_program(self) -> int: ...
    @property
    def midi_instrument(self) -> str: ...
    @property
    def midi_channel(self) -> int: ...
    @property
    def staves(self) -> int: ...
    @property
    def measures(self) -> list[Measure]: ...
    @property
    def notes(self) -> list[Note]:
        """Every sounding note in the part."""
        ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...

# -- Adapters ----------------------------------------------------------------
#
# Readers (``from_*``, ``Score.from_json``/``from_dict``,
# ``MusicDocument.from_json``, ``flatten``) raise ParseError (a ValueError)
# when the input cannot be read, and OSError when a file cannot be opened.
# ``strict=True`` LilyPond readers raise LilyPondSyntaxError (a ParseError) on
# errors. Writers (``to_*``) raise ValueError when a score cannot be written,
# OSError when the file cannot be. Anything may raise InternalError.

class HeaderField:
    """A field of a LilyPond ``\\header`` block (see :func:`header_fields`)."""

    @property
    def key(self) -> str: ...
    @property
    def value(self) -> str:
        """The value as text: strings decoded, ``\\markup`` as its words,
        ``#"…"`` as its string."""
        ...
    @property
    def start(self) -> int:
        """Character offset: ``text[f.start:f.end]`` is the whole ``key = value``."""
        ...
    @property
    def end(self) -> int: ...
    @property
    def score(self) -> int | None:
        """Index of the ``\\score`` block the field is in (file order), or
        ``None`` at the top level."""
        ...

def header_fields(text: str) -> list[HeaderField]:
    """Every ``\\header`` field of LilyPond text, from the parse tree alone
    (nothing is read); fields with values other than text (``##f``) are left
    out."""
    ...

def check_lilypond(text: str, *, semantic: bool = False) -> list[Diagnostic]:
    """Diagnostics of LilyPond text, in source order: the syntax only (fast,
    nothing is read), or with ``semantic=True`` what a reading reports too.
    Never raises for bad input: input too large to read is a ``too-large``
    error."""
    ...

def from_musicxml(path: str) -> Score: ...
def from_musicxml_string(xml: str) -> Score: ...
def from_musicxml_bytes(data: bytes) -> Score:
    """Parse MusicXML or compressed MXL from in-memory bytes."""
    ...
def from_lilypond(
    path: str, *, language: str | None = None, strict: bool = False
) -> Score:
    """The first movement; ``strict=True`` raises LilyPondSyntaxError on an
    error. The diagnostics are in ``Score.diagnostics`` either way."""
    ...
def from_lilypond_string(
    text: str, *, language: str | None = None, strict: bool = False
) -> Score: ...
def from_lilypond_movements(
    path: str, *, language: str | None = None, strict: bool = False
) -> list[Score]:
    """Every movement of a LilyPond file: one score per ``\\score`` block,
    each with the file's diagnostics."""
    ...
def from_lilypond_music(
    path: str, *, language: str | None = None, strict: bool = False
) -> MusicDocument: ...
def from_lilypond_music_string(
    text: str, *, language: str | None = None, strict: bool = False
) -> MusicDocument: ...
def to_lilypond(
    score: Score,
    path: str | None = None,
    *,
    language: str | None = None,
    relative: bool | None = None,
) -> str:
    """*relative*: ``True`` for ``\\relative`` entry, ``False`` for absolute,
    ``None`` for the score's own."""
    ...
def to_lilypond_music(
    doc: MusicDocument, path: str | None = None
) -> str: ...
def to_musicxml(score: Score, path: str | None = None) -> str: ...
def to_mxl_bytes(score: Score) -> bytes:
    """Serialize a score to compressed MusicXML (a ZIP archive)."""
    ...
def flatten(
    input: str,
    output: str | None = None,
    *,
    include_paths: list[str] | None = None,
    add_markers: bool = True,
) -> str: ...
def from_abc(path: str) -> Score: ...
def from_abc_string(text: str) -> Score: ...
def from_abc_tunes(path: str) -> list[Score]:
    """Every tune of an ABC file, one score each (``from_abc`` reads the first)."""
def to_abc(score: Score, path: str | None = None) -> str: ...
def from_humdrum(path: str) -> Score: ...
def from_humdrum_string(text: str) -> Score: ...
def to_humdrum(score: Score, path: str | None = None) -> str: ...

# MIDI (always built in)
def from_midi(
    path: str, *, quantize: int | None = None, swing: bool | None = None
) -> Score:
    """Parse a Standard MIDI File. ``quantize`` (4, 8, 16 or 32) is the
    shortest plain note value a played file is snapped to; ``swing`` reads
    swung eighths as straight ones marked "Swing" (``True``), never
    (``False``), or when a played file swings (``None``)."""
def from_midi_bytes(
    data: bytes, *, quantize: int | None = None, swing: bool | None = None
) -> Score:
    """Parse a Standard MIDI File from in-memory bytes."""
    ...
def to_midi(score: Score, path: str, *, unfold_repeats: bool = True) -> None:
    """Write a score to a Standard MIDI File, playing repeats out unless
    ``unfold_repeats=False``."""
    ...
def to_midi_bytes(score: Score, *, unfold_repeats: bool = True) -> bytes:
    """Serialize a score to Standard MIDI File bytes."""
    ...

# -- Transforms --------------------------------------------------------------

_M = TypeVar("_M", Score, MusicDocument)

def transpose(music: _M, semitones: int) -> _M: ...
def transpose_interval(music: _M, interval: str) -> _M: ...
def transpose_to_key(music: _M, key: str) -> _M: ...
def change_language(music: _M, language: str) -> _M: ...
def invert(
    music: _M,
    *,
    step: str = "C",
    alter: int = 0,
    octave: int = 4,
) -> _M: ...
def retrograde(music: _M) -> _M: ...

# -- ML representations (Epic D) ---------------------------------------------

import numpy as np
import numpy.typing as npt

def to_note_array(
    doc: MusicDocument, resolution: int = 480
) -> npt.NDArray[np.int32]:
    """Encode a document as a ``(N, 4)`` array: (onset, duration, pitch, velocity)."""
    ...

def from_note_array(
    array: npt.NDArray[np.int32], resolution: int = 480
) -> MusicDocument:
    """Decode a ``(N, 4)`` note array back into a document."""
    ...

def to_event_sequence(
    doc: MusicDocument,
    resolution: int = 480,
    max_time_shift: int = 100,
    velocity_bins: int = 32,
    encode_velocity: bool = True,
) -> npt.NDArray[np.int64]:
    """Encode a document as a 1-D event-code sequence (Performance-RNN style)."""
    ...

def from_event_sequence(
    array: npt.NDArray[np.int64],
    resolution: int = 480,
    max_time_shift: int = 100,
    velocity_bins: int = 32,
    encode_velocity: bool = True,
) -> MusicDocument:
    """Decode an event-code sequence back into a document."""
    ...

def to_piano_roll(
    doc: MusicDocument, resolution: int = 480, encode_velocity: bool = True
) -> npt.NDArray[np.uint8]:
    """Encode a document as a ``(T, 128)`` piano-roll matrix."""
    ...

def from_piano_roll(
    array: npt.NDArray[np.uint8],
    resolution: int = 480,
    encode_velocity: bool = True,
) -> MusicDocument:
    """Decode a ``(T, 128)`` piano-roll matrix back into a document."""
    ...

def compute_metrics(
    doc: MusicDocument,
    resolution: int = 480,
    measure_resolution: int | None = None,
) -> dict[str, Any]:
    """Compute objective evaluation metrics (NaN where undefined).

    Keys: n_pitches_used, n_pitch_classes_used, pitch_range,
    pitch_class_histogram, pitch_entropy, pitch_class_entropy, polyphony,
    polyphony_rate, empty_beat_rate, scale_consistency, groove_consistency.
    """
    ...
