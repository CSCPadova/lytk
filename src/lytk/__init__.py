"""lytk — music notation conversion and augmentation toolkit."""

from __future__ import annotations

from lytk._core import (
    Chord,
    Measure,
    MusicDocument,
    Note,
    Part,
    Pitch,
    Rest,
    Score,
    Voice,
    change_language,
    compute_metrics,
    flatten,
    from_abc,
    from_abc_string,
    from_event_sequence,
    from_lilypond,
    from_lilypond_music,
    from_lilypond_music_string,
    from_lilypond_string,
    from_musicxml,
    from_musicxml_bytes,
    from_musicxml_string,
    from_note_array,
    from_piano_roll,
    invert,
    retrograde,
    to_abc,
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
    # Structured note navigation (read-only Layer-2 tree)
    "Part",
    "Measure",
    "Voice",
    "Note",
    "Rest",
    "Chord",
    "Pitch",
    # Adapters
    "from_musicxml",
    "from_musicxml_string",
    "from_musicxml_bytes",
    "from_lilypond",
    "from_lilypond_string",
    "from_lilypond_music",
    "from_lilypond_music_string",
    "from_abc",
    "from_abc_string",
    "to_lilypond",
    "to_lilypond_music",
    "to_musicxml",
    "to_abc",
    # LilyPond \include flattening
    "flatten",
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
    "compute_metrics",
]

# MIDI is always built into the extension.
from lytk._core import (  # noqa: E402
    from_midi,
    from_midi_bytes,
    to_midi,
    to_midi_bytes,
)

__all__ += ["from_midi", "from_midi_bytes", "to_midi", "to_midi_bytes"]
