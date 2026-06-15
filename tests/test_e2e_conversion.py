"""End-to-end cross-format conversion fidelity.

Every format reads/writes through the shared IR, so any A -> B conversion should
preserve the *note content* (pitch multiset) — that is the universal lossless
invariant, since every supported format can represent notes. Losses of
articulations / dynamics / lyrics / slurs are format-inherent and out of scope
here; this suite pins the note-content invariant and guards against regressions
of the conversion bugs found by the fidelity audit.

The note signature is taken via the format-agnostic note-array representation
(onset, duration, pitch, velocity), so it is independent of how each format
spells things.

Known format-inherent limits (NOT asserted as lossless, by design):
  * -> ABC for multi-voice / piano sources: ABC v1 emits a single melodic line,
    so polyphony is dropped (ABC v2 `V:` multi-voice would be needed).
  * -> MIDI: durations are quantized and free-time (cadenza/`\\cadenzaOn`) cannot
    be represented, so duration equality is not required (pitch still is, mostly).
"""
from __future__ import annotations

from collections import Counter
from pathlib import Path

import pytest

import lytk

FIX = Path(__file__).parent / "fixtures"
RES = 96  # note-array steps per quarter

_LOADERS = {
    "ly": lytk.from_lilypond,
    "xml": lytk.from_musicxml,
    "abc": lytk.from_abc,
    "midi": lytk.from_midi,
}


def _pitch_multiset(score) -> Counter:
    arr = lytk.to_note_array(score.to_music_document(), RES)
    return Counter(int(p) for p in arr[:, 2])


def _emit_reparse(score, fmt: str, tmp_path: Path):
    """score -> fmt -> re-parse -> Score (round-trips through fmt's writer+reader)."""
    if fmt == "ly":
        return lytk.from_lilypond_string(lytk.to_lilypond(score))
    if fmt == "xml":
        return lytk.from_musicxml_string(lytk.to_musicxml(score))
    if fmt == "abc":
        return lytk.from_abc_string(lytk.to_abc(score))
    if fmt == "midi":
        p = str(tmp_path / "rt.mid")
        lytk.to_midi(score, p)
        return lytk.from_midi(p)
    raise ValueError(fmt)


# Curated fixtures that are single-line / simple enough that the full note
# content survives a round trip through every notation format. (Several of the
# ly ones are the Music21-exported corpus that exposed the relative-octave bug.)
LOSSLESS_NOTATION = [
    ("ly", "ly/00a01a21-e760-475e-ba61-a7e1bb919d3b.ly"),
    ("ly", "ly/0a0ca4ef-b5d5-4e99-94ad-8500deece021.ly"),
    ("xml", "xml/01a-Pitches-Pitches.xml"),
    ("xml", "xml/01b-Pitches-Intervals.xml"),
    ("abc", "abc/simple.abc"),
    ("abc", "abc/chords.abc"),
]


@pytest.mark.parametrize("src_fmt,rel", LOSSLESS_NOTATION)
@pytest.mark.parametrize("dst_fmt", ["ly", "xml", "abc"])
def test_notation_conversion_preserves_pitch(src_fmt, rel, dst_fmt, tmp_path):
    """A -> {ly,xml,abc} -> A preserves the pitch multiset for single-line music."""
    score = _LOADERS[src_fmt](str(FIX / rel))
    before = _pitch_multiset(score)
    assert sum(before.values()) > 0, "fixture has no notes"
    after = _pitch_multiset(_emit_reparse(score, dst_fmt, tmp_path))
    assert after == before, (
        f"{src_fmt} -> {dst_fmt} changed the pitch multiset "
        f"({sum(before.values())} -> {sum(after.values())} notes)"
    )


@pytest.mark.parametrize("src_fmt,rel", LOSSLESS_NOTATION)
def test_to_midi_preserves_pitch_set(src_fmt, rel, tmp_path):
    """-> MIDI preserves the set of pitches (durations may be quantized)."""
    score = _LOADERS[src_fmt](str(FIX / rel))
    before = set(_pitch_multiset(score))
    after = set(_pitch_multiset(_emit_reparse(score, "midi", tmp_path)))
    # MIDI can split/merge tied or repeated notes; the pitch *set* must survive.
    missing = before - after
    assert not missing, f"{src_fmt} -> midi dropped pitches {sorted(missing)}"


# ---------------------------------------------------------------------------
# Targeted regressions for bugs the audit found and fixed
# ---------------------------------------------------------------------------

def test_regression_relative_octave_shift_single_staff():
    """ly->ly must not shift an octave (the `\\relative a' { a' }` double-count)."""
    f = str(FIX / "ly/00a01a21-e760-475e-ba61-a7e1bb919d3b.ly")
    s = lytk.from_lilypond(f)
    before = _pitch_multiset(s)
    after = _pitch_multiset(lytk.from_lilypond_string(lytk.to_lilypond(s)))
    assert after == before
    # Specifically: no wholesale +12 transposition.
    assert max(before) == max(after)


def test_regression_ly_to_abc_not_empty():
    """ly->abc must not drop every note (the Simultaneous first-branch-only bug)."""
    s = lytk.from_lilypond(str(FIX / "ly/00a01a21-e760-475e-ba61-a7e1bb919d3b.ly"))
    n = sum(_pitch_multiset(lytk.from_abc_string(lytk.to_abc(s))).values())
    assert n > 0, "ly -> abc produced no notes"
