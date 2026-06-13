"""Tests for the ML representations (Epic D) and their numpy interop (EDT4)."""

from __future__ import annotations

import numpy as np

import lytk

# A small 3-note phrase: c'4 d'4 e'2.
SRC = r"""\version "2.24.0"
\score { \new Staff { c'4 d'4 e'2 } \layout { } }
"""


def _doc():
    return lytk.from_lilypond_music_string(SRC)


# ---------------------------------------------------------------------------
# Note-array
# ---------------------------------------------------------------------------


class TestNoteArray:
    def test_shape_and_dtype(self):
        arr = lytk.to_note_array(_doc(), 480)
        assert arr.shape == (3, 4)
        assert arr.dtype == np.int32

    def test_values(self):
        arr = lytk.to_note_array(_doc(), 480)
        assert list(arr[:, 0]) == [0, 480, 960]  # onsets
        assert list(arr[:, 1]) == [480, 480, 960]  # durations
        assert list(arr[:, 2]) == [60, 62, 64]  # pitches (C4, D4, E4)

    def test_roundtrip_exact(self):
        arr = lytk.to_note_array(_doc(), 480)
        doc2 = lytk.from_note_array(arr, 480)
        assert np.array_equal(arr, lytk.to_note_array(doc2, 480))

    def test_bad_shape_raises(self):
        import pytest

        with pytest.raises(ValueError):
            lytk.from_note_array(np.zeros((3, 3), dtype=np.int32), 480)


# ---------------------------------------------------------------------------
# Event sequence
# ---------------------------------------------------------------------------


class TestEventSequence:
    def test_1d_int(self):
        ev = lytk.to_event_sequence(_doc(), 24)
        assert ev.ndim == 1
        assert ev.dtype == np.int64

    def test_roundtrip_onset_duration_pitch(self):
        doc = _doc()
        ev = lytk.to_event_sequence(doc, 24)
        back = lytk.from_event_sequence(ev, 24)
        a = lytk.to_note_array(back, 24)
        ref = lytk.to_note_array(doc, 24)
        assert np.array_equal(a[:, :3], ref[:, :3])

    def test_no_velocity_smaller_vocab(self):
        with_vel = lytk.to_event_sequence(_doc(), 24, encode_velocity=True)
        without = lytk.to_event_sequence(_doc(), 24, encode_velocity=False)
        # Dropping velocity events shortens the sequence.
        assert len(without) < len(with_vel)


# ---------------------------------------------------------------------------
# Piano-roll
# ---------------------------------------------------------------------------


class TestPianoRoll:
    def test_shape_and_dtype(self):
        pr = lytk.to_piano_roll(_doc(), 4)  # quarter = 4 steps
        assert pr.shape == (16, 128)  # 4 + 4 + 8 steps
        assert pr.dtype == np.uint8

    def test_velocity_cells(self):
        pr = lytk.to_piano_roll(_doc(), 4)
        assert pr[0, 60] == 64  # default velocity
        assert pr[0, 59] == 0

    def test_binary_mode(self):
        pr = lytk.to_piano_roll(_doc(), 4, encode_velocity=False)
        assert pr[0, 60] == 1

    def test_roundtrip_distinct_pitches(self):
        doc = _doc()
        pr = lytk.to_piano_roll(doc, 4)
        back = lytk.from_piano_roll(pr, 4)
        a = lytk.to_note_array(back, 4)
        ref = lytk.to_note_array(doc, 4)
        assert np.array_equal(a[:, :3], ref[:, :3])

    def test_bad_shape_raises(self):
        import pytest

        with pytest.raises(ValueError):
            lytk.from_piano_roll(np.zeros((4, 64), dtype=np.uint8), 4)
