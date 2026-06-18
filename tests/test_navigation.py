"""Tests for structured note navigation (typed Part/Measure/Voice/Note/...)."""

from __future__ import annotations

import lytk

# c'4 d'4 e'4 f'4 in one 4/4 bar, plus a two-note chord and a rest.
SRC = r"""\version "2.24.0"
\score { \new Staff { \time 4/4 c'4 d'4 <e' g'>4 r4 } \layout { } }
"""


def _score():
    return lytk.from_lilypond_string(SRC)


class TestNavigation:
    def test_iter_parts_returns_typed_parts(self):
        parts = _score().iter_parts()
        assert isinstance(parts, list)
        assert len(parts) == 1
        assert isinstance(parts[0], lytk.Part)

    def test_walk_to_notes(self):
        part = _score().iter_parts()[0]
        assert len(part.measures) >= 1
        measure = part.measures[0]
        assert isinstance(measure, lytk.Measure)
        assert measure.number == 1
        assert measure.time_signature == ("4", 4, None)
        voice = measure.voices[0]
        assert isinstance(voice, lytk.Voice)
        elements = voice.elements
        # c, d, chord, rest
        kinds = [type(e).__name__ for e in elements]
        assert "Note" in kinds
        assert "Chord" in kinds
        assert "Rest" in kinds

    def test_note_pitch_and_duration(self):
        part = _score().iter_parts()[0]
        first = part.measures[0].voices[0].elements[0]
        assert isinstance(first, lytk.Note)
        assert first.pitch.name == "C4"
        assert first.pitch.midi == 60
        assert first.pitch.step == "C"
        assert first.pitch.octave == 4
        assert first.duration == 1.0  # one quarter note
        assert first.duration_fraction == (1, 4)

    def test_chord_notes(self):
        part = _score().iter_parts()[0]
        elements = part.measures[0].voices[0].elements
        chord = next(e for e in elements if isinstance(e, lytk.Chord))
        midis = sorted(n.midi for n in chord.notes)
        assert midis == [64, 67]  # e' g'
        assert len(chord) == 2

    def test_part_notes_flattens_chords(self):
        part = _score().iter_parts()[0]
        # c, d, e, g  (chord members flattened, rest excluded)
        midis = sorted(n.midi for n in part.notes)
        assert midis == [60, 62, 64, 67]

    def test_repr(self):
        part = _score().iter_parts()[0]
        assert "Part" in repr(part)
        note = part.measures[0].voices[0].elements[0]
        assert "Note" in repr(note)
        assert "C4" in repr(note.pitch)

    def test_multipart_navigation(self):
        # ABC multi-voice → two parts, each independently navigable.
        abc = "X:1\nM:4/4\nL:1/4\nK:C\nV:1\nCDEF|\nV:2\nC,D,E,F,|\n"
        score = lytk.from_abc_string(abc)
        parts = score.iter_parts()
        assert len(parts) == 2
        top = sorted(n.midi for n in parts[0].notes)
        bottom = sorted(n.midi for n in parts[1].notes)
        assert top == [60, 62, 64, 65]
        assert bottom == [48, 50, 52, 53]
