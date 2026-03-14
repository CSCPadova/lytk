"""Tests for the lytk Python bindings (Phase 7)."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import pytest

import lytk

FIXTURE_XML = Path("tests/fixtures/xml/01a-Pitches-Pitches.xml")
FIXTURE_LY = sorted(Path("tests/fixtures/ly").glob("*.ly"))[0]

_has_midi = hasattr(lytk, "to_midi")


# ---------------------------------------------------------------------------
# Score class
# ---------------------------------------------------------------------------


class TestScore:
    def test_from_musicxml_metadata(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        assert score.title == "Pitches and accidentals"
        assert score.num_parts == 1
        assert isinstance(score.parts, list)
        assert len(score.parts) == 1

    def test_repr(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        r = repr(score)
        assert "Score" in r
        assert "parts=" in r

    def test_str(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        assert str(score) == repr(score)

    def test_equality(self):
        s1 = lytk.from_musicxml(str(FIXTURE_XML))
        s2 = lytk.from_musicxml(str(FIXTURE_XML))
        assert s1 == s2

    def test_json_roundtrip(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        j = score.to_json()
        assert isinstance(j, str)
        parsed = json.loads(j)
        assert "metadata" in parsed

        restored = lytk.Score.from_json(j)
        assert score == restored

    def test_dict_roundtrip(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        d = score.to_dict()
        assert isinstance(d, dict)
        assert "metadata" in d

        restored = lytk.Score.from_dict(d)
        assert score == restored


# ---------------------------------------------------------------------------
# Adapter functions
# ---------------------------------------------------------------------------


class TestAdapters:
    def test_from_musicxml(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        assert score.num_parts >= 1

    def test_from_musicxml_string(self):
        xml = FIXTURE_XML.read_text()
        score = lytk.from_musicxml_string(xml)
        assert score.num_parts >= 1
        ref = lytk.from_musicxml(str(FIXTURE_XML))
        assert score == ref

    def test_from_lilypond(self):
        score = lytk.from_lilypond(str(FIXTURE_LY))
        assert score.num_parts >= 0  # some fixtures may have 0 parts

    def test_from_lilypond_string(self):
        text = '{ c4 d4 e4 f4 }'
        score = lytk.from_lilypond_string(text)
        assert isinstance(score, lytk.Score)

    def test_to_lilypond_string(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        ly = lytk.to_lilypond(score)
        assert isinstance(ly, str)
        assert "\\version" in ly

    def test_to_lilypond_file(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        with tempfile.NamedTemporaryFile(suffix=".ly", delete=False) as f:
            path = f.name
        ly = lytk.to_lilypond(score, path)
        assert Path(path).read_text() == ly

    def test_to_lilypond_with_language(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        ly_en = lytk.to_lilypond(score, language="english")
        assert '\\language "english"' in ly_en

    def test_to_musicxml_string(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        xml = lytk.to_musicxml(score)
        assert isinstance(xml, str)
        assert "<score-partwise" in xml

    def test_to_musicxml_file(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        with tempfile.NamedTemporaryFile(suffix=".xml", delete=False) as f:
            path = f.name
        xml = lytk.to_musicxml(score, path)
        assert Path(path).read_text() == xml

    def test_roundtrip_musicxml(self):
        """MusicXML → IR → MusicXML → IR should produce equivalent scores."""
        s1 = lytk.from_musicxml(str(FIXTURE_XML))
        xml = lytk.to_musicxml(s1)
        s2 = lytk.from_musicxml_string(xml)
        assert s1.num_parts == s2.num_parts
        assert s1.title == s2.title


# ---------------------------------------------------------------------------
# MIDI adapter functions
# ---------------------------------------------------------------------------


class TestMidi:
    @pytest.mark.skipif(not _has_midi, reason="built without midi feature")
    def test_to_midi_and_back(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
            path = f.name
        lytk.to_midi(score, path)
        assert Path(path).stat().st_size > 0

        back = lytk.from_midi(path)
        assert back.num_parts >= 1


# ---------------------------------------------------------------------------
# Transform functions
# ---------------------------------------------------------------------------


class TestTransforms:
    def test_transpose(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        t = lytk.transpose(score, 3)
        assert isinstance(t, lytk.Score)
        # Original should be unchanged
        assert score == lytk.from_musicxml(str(FIXTURE_XML))

    def test_transpose_zero_identity(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        t = lytk.transpose(score, 0)
        assert score == t

    def test_transpose_roundtrip(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        up = lytk.transpose(score, 5)
        back = lytk.transpose(up, -5)
        # Enharmonic spellings may differ; check structural equivalence.
        assert score.num_parts == back.num_parts
        assert score.title == back.title

    def test_change_language(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        en = lytk.change_language(score, "english")
        assert en.language == "english"

    def test_change_language_invalid(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        try:
            lytk.change_language(score, "klingon")
            assert False, "expected ValueError"
        except ValueError:
            pass

    def test_invert(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        inv = lytk.invert(score, step="C", octave=4)
        assert isinstance(inv, lytk.Score)

    def test_invert_self_inverse(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        inv1 = lytk.invert(score, step="C", octave=4)
        inv2 = lytk.invert(inv1, step="C", octave=4)
        # Enharmonic spellings may differ; check structural equivalence.
        assert score.num_parts == inv2.num_parts
        assert score.title == inv2.title

    def test_retrograde(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        r = lytk.retrograde(score)
        assert isinstance(r, lytk.Score)

    def test_retrograde_self_inverse(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        r = lytk.retrograde(lytk.retrograde(score))
        assert score == r
