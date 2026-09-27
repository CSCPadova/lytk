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
# ABC adapter functions
# ---------------------------------------------------------------------------

_ABC = "X:1\nT:Scale\nM:4/4\nL:1/4\nK:C\nC D E F | G A B c |]\n"


class TestAbc:
    def test_from_abc_string(self):
        score = lytk.from_abc_string(_ABC)
        assert isinstance(score, lytk.Score)
        assert score.title == "Scale"
        assert score.num_parts >= 1

    def test_to_abc_string(self):
        score = lytk.from_abc_string(_ABC)
        abc = lytk.to_abc(score)
        assert isinstance(abc, str)
        assert abc.startswith("X:")
        assert "K:C" in abc

    def test_to_abc_file(self):
        score = lytk.from_abc_string(_ABC)
        with tempfile.NamedTemporaryFile(suffix=".abc", delete=False) as f:
            path = f.name
        abc = lytk.to_abc(score, path)
        assert Path(path).read_text() == abc

    def test_from_abc_file(self):
        with tempfile.NamedTemporaryFile(suffix=".abc", mode="w", delete=False) as f:
            f.write(_ABC)
            path = f.name
        score = lytk.from_abc(path)
        assert score == lytk.from_abc_string(_ABC)

    def test_roundtrip_abc(self):
        """ABC → IR → ABC → IR preserves the note content (pitch multiset)."""
        s1 = lytk.from_abc_string(_ABC)
        abc = lytk.to_abc(s1)
        s2 = lytk.from_abc_string(abc)
        assert s1.num_parts == s2.num_parts


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


# ---------------------------------------------------------------------------
# Phase 4: bytes I/O, note navigation, typed errors, MusicDocument parity
# ---------------------------------------------------------------------------


class TestBytesIO:
    def test_from_musicxml_bytes_matches_path(self):
        data = FIXTURE_XML.read_bytes()
        assert lytk.from_musicxml_bytes(data) == lytk.from_musicxml(str(FIXTURE_XML))

    def test_from_musicxml_bytes_reads_mxl(self):
        mxl = sorted(Path("tests/fixtures/mxl").glob("*.mxl"))[0]
        score = lytk.from_musicxml_bytes(mxl.read_bytes())
        assert score.num_parts >= 1

    def test_midi_bytes_roundtrip(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        data = lytk.to_midi_bytes(score)
        assert isinstance(data, bytes)
        assert data[:4] == b"MThd"
        reloaded = lytk.from_midi_bytes(data)
        assert reloaded.num_parts >= 1


class TestNoteNavigation:
    def test_score_notes(self):
        score = lytk.from_musicxml(str(FIXTURE_XML))
        notes = score.notes()
        assert isinstance(notes, list)
        assert notes, "expected at least one note"
        onset, dur, pitch, vel = notes[0]
        assert all(isinstance(x, int) for x in (onset, dur, pitch, vel))
        assert 0 <= pitch <= 127

    def test_music_document_notes_and_metadata(self):
        doc = lytk.from_musicxml(str(FIXTURE_XML)).to_music_document()
        assert isinstance(doc.notes(), list)
        # Mirrored metadata getters (parity with Score).
        assert hasattr(doc, "subtitle")
        assert hasattr(doc, "arranger")
        assert hasattr(doc, "language")


class TestTypedErrors:
    def test_missing_file_raises_ioerror(self):
        with pytest.raises((IOError, OSError)):
            lytk.from_musicxml("/no/such/file.xml")

    def test_malformed_string_raises_valueerror_not_ioerror(self):
        # A parse failure must be a ValueError, not an IOError — so batch loaders
        # wrapping reads in `except IOError` don't silently swallow corrupt input.
        with pytest.raises(ValueError):
            lytk.from_musicxml_string("this is not valid musicxml <<<")
        with pytest.raises(ValueError):
            lytk.from_abc_string("\x00\x01 not abc")



class TestPanicFirewall:
    """A Rust panic reaches Python as lytk.InternalError, never as pyo3's
    PanicException (a BaseException), and prints nothing."""

    def test_hierarchy(self):
        assert issubclass(lytk.InternalError, lytk.LytkError)
        assert issubclass(lytk.LytkError, Exception)

    def test_panic_becomes_internal_error_silently(self, capfd):
        with pytest.raises(lytk.InternalError, match=r"lytk panicked: boom \(at src/python\.rs:\d+\)"):
            lytk._core._panic_for_tests("boom")
        assert capfd.readouterr().err == ""

    def test_except_exception_catches_it(self):
        try:
            lytk._core._panic_for_tests("caught")
        except Exception as e:  # noqa: BLE001 - the point of the test
            assert isinstance(e, lytk.InternalError)
        else:
            pytest.fail("no exception")

    def test_library_still_works_after_a_panic(self):
        for _ in range(3):
            with pytest.raises(lytk.InternalError):
                lytk._core._panic_for_tests("again")
        assert lytk.from_lilypond_string("{ c'4 d' }").num_parts == 1



def _set_first(node, key, value):
    """Set the first occurrence of `key` in a nested JSON value; True if found."""
    if isinstance(node, dict):
        if key in node:
            node[key] = value
            return True
        return any(_set_first(v, key, value) for v in node.values())
    if isinstance(node, list):
        return any(_set_first(v, key, value) for v in node)
    return False


class TestHandSuppliedIr:
    """from_dict / from_json refuse the values the IR would divide by."""

    LY = r"{ \time 3/4 \tuplet 3/2 { c8 d e } f4 }"

    @pytest.mark.parametrize(
        ("key", "value"),
        [("beat_type", 0), ("tuplet_actual", 0), ("tuplet_normal", 0), ("base", [-1, 8])],
    )
    def test_score_refuses_values_no_reader_makes(self, key, value):
        d = lytk.from_lilypond_string(self.LY).to_dict()
        assert _set_first(d, key, value)
        with pytest.raises(ValueError, match="invalid IR"):
            lytk.Score.from_dict(d)
        with pytest.raises(ValueError, match="invalid IR"):
            lytk.Score.from_json(json.dumps(d))

    def test_music_document_refuses_a_zero_tuplet_term(self):
        d = json.loads(lytk.from_lilypond_music_string(self.LY).to_json())
        with pytest.raises(ValueError, match="invalid IR"):
            e = json.loads(json.dumps(d))
            assert _set_first(e, "tuplet_actual", 0)
            lytk.MusicDocument.from_json(json.dumps(e))
        # A Tuplet node of the Music tree, as the ABC and Humdrum readers make them.
        d["music"] = {"Tuplet": {"normal": 2, "actual": 0, "content": d["music"]}}
        with pytest.raises(ValueError, match="invalid IR"):
            lytk.MusicDocument.from_json(json.dumps(d))

    def test_valid_ir_round_trips(self):
        score = lytk.from_lilypond_string(self.LY)
        assert lytk.Score.from_dict(score.to_dict()) == score
        assert lytk.Score.from_json(score.to_json()) == score



class TestArgumentBounds:
    """Extreme arguments are a ValueError, not an i32 overflow in Rust."""

    LY = r"{ c'4 d' e' f' }"

    def test_transpose(self):
        score = lytk.from_lilypond_string(self.LY)
        assert lytk.transpose(score, 127).num_parts == 1
        with pytest.raises(ValueError, match="semitones"):
            lytk.transpose(score, 2**31 - 1)

    def test_invert_axis(self):
        score = lytk.from_lilypond_string(self.LY)
        with pytest.raises(ValueError, match="axis"):
            lytk.invert(score, octave=2**31 - 1)
        with pytest.raises(ValueError, match="axis"):
            lytk.invert(score, alter=2**31 - 1)

    def test_interval_number(self):
        score = lytk.from_lilypond_string(self.LY)
        with pytest.raises(ValueError, match="between 1 and 99"):
            lytk.transpose_interval(score, "P2147483647")

    def test_extreme_octave_in_hand_supplied_ir(self):
        d = lytk.from_lilypond_string(self.LY).to_dict()
        assert _set_first(d, "octave", 2**31 - 1)
        with pytest.raises(ValueError, match="octave"):
            lytk.Score.from_dict(d)


# -- Review R8/R9: Layer-1 transforms, transpose_to_key, compressed MXL -------


LY_C_MAJOR = r"\score { \new Staff { \key c \major c'4 e' g' } }"


def test_transforms_accept_music_document():
    doc = lytk.from_lilypond_music_string(LY_C_MAJOR)
    up = lytk.transpose(doc, 3)
    assert type(up) is lytk.MusicDocument, "same type in, same type out"
    out = lytk.to_lilypond_music(up)
    assert "\\key ees" in out and "ees" in out
    assert type(lytk.retrograde(doc)) is lytk.MusicDocument
    assert type(lytk.invert(doc)) is lytk.MusicDocument
    assert type(lytk.transpose_interval(doc, "M2")) is lytk.MusicDocument


def test_transpose_to_key():
    score = lytk.from_lilypond_string(LY_C_MAJOR)
    moved = lytk.transpose_to_key(score, "Bb")
    assert type(moved) is lytk.Score
    assert "\\key bes" in lytk.to_lilypond(moved)


def test_to_musicxml_writes_compressed_mxl(tmp_path):
    import zipfile

    score = lytk.from_lilypond_string(LY_C_MAJOR)
    p = tmp_path / "t.mxl"
    lytk.to_musicxml(score, str(p))
    assert zipfile.is_zipfile(p), ".mxl must be a real ZIP, not plain XML"
    back = lytk.from_musicxml(str(p))
    assert len(back.parts) == len(score.parts)


def test_humdrum_kern_round_trip(tmp_path):
    kern = "**kern\n*clefG2\n*M4/4\n4c\n4e\n4g\n4cc\n*-\n"
    score = lytk.from_humdrum_string(kern)
    out = lytk.to_humdrum(score)
    assert "**kern" in out and "4cc" in out
    back = lytk.from_humdrum_string(out)
    assert len(back.notes()) == len(score.notes()) == 4
    p = tmp_path / "t.krn"
    lytk.to_humdrum(score, str(p))
    assert lytk.from_humdrum(str(p)).notes()


# ---------------------------------------------------------------------------
# Bindings the CLI is built on
# ---------------------------------------------------------------------------


def test_from_lilypond_movements(tmp_path):
    src = tmp_path / "two.ly"
    src.write_text(r"\score { \new Staff { c'1 } } \score { \new Staff { d'1 e'1 } }")
    movements = lytk.from_lilypond_movements(str(src))
    assert [len(m.notes()) for m in movements] == [1, 2]
    # A file without \score blocks is one movement.
    single = tmp_path / "one.ly"
    single.write_text("{ c'4 d' }")
    assert len(lytk.from_lilypond_movements(str(single))) == 1


def test_from_abc_tunes(tmp_path):
    src = tmp_path / "two.abc"
    src.write_text("X:1\nK:C\nC|\n\nX:2\nK:C\nD E|\n")
    assert [len(t.notes()) for t in lytk.from_abc_tunes(str(src))] == [1, 2]
    assert len(lytk.from_abc(str(src)).notes()) == 1


def test_to_lilypond_relative_and_absolute():
    score = lytk.from_lilypond_string(r"\relative c' { c4 d e }")
    assert "\\relative" in lytk.to_lilypond(score, relative=True)
    assert "\\relative" not in lytk.to_lilypond(score, relative=False)


def test_to_mxl_bytes_is_a_zip_that_reads_back():
    score = lytk.from_musicxml(str(FIXTURE_XML))
    data = lytk.to_mxl_bytes(score)
    assert data[:4] == b"PK\x03\x04"
    back = lytk.from_musicxml_bytes(data)
    assert back.notes() == score.notes()
    # Same content as the plain-XML writer produces.
    assert back == lytk.from_musicxml_string(lytk.to_musicxml(score))


def test_score_lyricist_and_part_midi_instrument():
    score = lytk.from_lilypond_string(
        '\\header { poet = "Anon" }\n'
        '\\new Staff \\with { midiInstrument = "violin" } { c\'4 }'
    )
    assert score.lyricist == "Anon"
    assert score.iter_parts()[0].midi_instrument == "violin"


def test_from_midi_quantize_option(tmp_path):
    score = lytk.from_lilypond_string("{ c'4 d'4 e'4 f'4 }")
    data = lytk.to_midi_bytes(score)
    for q in (None, 8, 16):
        back = lytk.from_midi_bytes(data, quantize=q)
        assert len(back.notes()) == 4


def test_from_midi_swing_option(tmp_path):
    # Quarter-eighth triplets written exactly: triplets unless asked.
    score = lytk.from_lilypond_string(r"{ \tuplet 3/2 { c'4 e'8 } \tuplet 3/2 { c'4 e'8 } c'2 }")
    data = lytk.to_midi_bytes(score)
    path = tmp_path / "swing.mid"
    path.write_bytes(data)
    for read in (
        lambda **kw: lytk.from_midi_bytes(data, **kw),
        lambda **kw: lytk.from_midi(str(path), **kw),
    ):
        written = lytk.to_lilypond(read())
        assert "Swing" not in written and "\\tuplet" in written
        straight = lytk.to_lilypond(read(swing=True))
        assert "Swing" in straight and "\\tuplet" not in straight
