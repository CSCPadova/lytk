"""lytk — music notation conversion and augmentation toolkit."""

from __future__ import annotations

from lytk._core import (
    Score,
    change_language,
    from_lilypond,
    from_lilypond_string,
    from_musicxml,
    from_musicxml_string,
    invert,
    retrograde,
    to_lilypond,
    to_musicxml,
    transpose,
)

__all__ = [
    "Score",
    # Adapters
    "from_musicxml",
    "from_musicxml_string",
    "from_lilypond",
    "from_lilypond_string",
    "to_lilypond",
    "to_musicxml",
    # Transforms
    "transpose",
    "change_language",
    "invert",
    "retrograde",
]

# MIDI functions are only available when built with the "midi" feature.
try:
    from lytk._core import from_midi, to_midi

    __all__ += ["from_midi", "to_midi"]
except ImportError:
    pass
