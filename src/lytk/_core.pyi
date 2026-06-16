"""Type stubs for the compiled Rust extension (``lytk._core``)."""

from __future__ import annotations

from typing import Any

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
    def language(self) -> str | None: ...
    @property
    def num_parts(self) -> int: ...
    @property
    def parts(self) -> list[str]: ...
    def to_json(self) -> str: ...
    @staticmethod
    def from_json(json: str) -> Score: ...
    def to_dict(self) -> dict[str, Any]: ...
    @staticmethod
    def from_dict(dict: dict[str, Any]) -> Score: ...
    def to_music_document(self) -> MusicDocument: ...
    def __eq__(self, other: object) -> bool: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class MusicDocument:
    """Opaque handle to a parsed music document (Layer 1 Music tree)."""

    @property
    def title(self) -> str | None: ...
    @property
    def composer(self) -> str | None: ...
    def to_json(self) -> str: ...
    @staticmethod
    def from_json(json: str) -> MusicDocument: ...
    def to_score(self) -> Score: ...
    def __eq__(self, other: object) -> bool: ...
    def __repr__(self) -> str: ...

# -- Adapters ----------------------------------------------------------------

def from_musicxml(path: str) -> Score: ...
def from_musicxml_string(xml: str) -> Score: ...
def from_lilypond(path: str, *, language: str | None = None) -> Score: ...
def from_lilypond_string(text: str, *, language: str | None = None) -> Score: ...
def from_lilypond_music(
    path: str, *, language: str | None = None
) -> MusicDocument: ...
def from_lilypond_music_string(
    text: str, *, language: str | None = None
) -> MusicDocument: ...
def to_lilypond(
    score: Score,
    path: str | None = None,
    *,
    language: str | None = None,
) -> str: ...
def to_lilypond_music(
    doc: MusicDocument, path: str | None = None
) -> str: ...
def to_musicxml(score: Score, path: str | None = None) -> str: ...
def flatten(
    input: str,
    output: str | None = None,
    *,
    include_paths: list[str] | None = None,
    add_markers: bool = True,
) -> str: ...
def from_abc(path: str) -> Score: ...
def from_abc_string(text: str) -> Score: ...
def to_abc(score: Score, path: str | None = None) -> str: ...

# MIDI (only available when built with the "midi" feature)
def from_midi(path: str) -> Score: ...
def to_midi(score: Score, path: str) -> None: ...

# -- Transforms --------------------------------------------------------------

def transpose(score: Score, semitones: int) -> Score: ...
def change_language(score: Score, language: str) -> Score: ...
def invert(
    score: Score,
    *,
    step: str = "C",
    alter: int = 0,
    octave: int = 4,
) -> Score: ...
def retrograde(score: Score) -> Score: ...

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
