"""lytk — music notation conversion and augmentation toolkit."""

from __future__ import annotations

from lytk._core import (
    MusicDocument,
    Score,
    change_language,
    from_event_sequence,
    from_lilypond,
    from_lilypond_music,
    from_lilypond_music_string,
    from_lilypond_string,
    from_musicxml,
    from_musicxml_string,
    from_note_array,
    from_piano_roll,
    invert,
    retrograde,
    to_event_sequence,
    to_lilypond,
    to_lilypond_music,
    to_musicxml,
    to_note_array,
    to_piano_roll,
    transpose,
)

__all__ = [
    "MusicDocument",
    "Score",
    # Adapters
    "from_musicxml",
    "from_musicxml_string",
    "from_lilypond",
    "from_lilypond_string",
    "from_lilypond_music",
    "from_lilypond_music_string",
    "to_lilypond",
    "to_lilypond_music",
    "to_musicxml",
    # Transforms
    "transpose",
    "change_language",
    "invert",
    "retrograde",
    # ML representations (Epic D)
    "to_note_array",
    "from_note_array",
    "to_event_sequence",
    "from_event_sequence",
    "to_piano_roll",
    "from_piano_roll",
]

# MIDI functions are only available when built with the "midi" feature.
try:
    from lytk._core import from_midi, to_midi

    __all__ += ["from_midi", "to_midi"]
except ImportError:
    pass
