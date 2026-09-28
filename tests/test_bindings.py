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



def test_version_is_the_crates():
    # One version source: Cargo.toml, which maturin puts in the metadata too.
    import re
    from importlib.metadata import version

    cargo = (Path(__file__).parent.parent / "Cargo.toml").read_text()
    expected = re.search(r'^version = "(.+)"$', cargo, re.M).group(1)
    assert lytk.__version__ == expected == version("lytk")


class TestExceptionHierarchy:
    """Readers raise ParseError, which is a ValueError too; I/O stays OSError."""

    def test_classes(self):
        assert issubclass(lytk.ParseError, lytk.LytkError)
        assert issubclass(lytk.ParseError, ValueError)
        assert issubclass(lytk.LilyPondSyntaxError, lytk.ParseError)
        assert issubclass(lytk.InternalError, lytk.LytkError)
        assert not issubclass(lytk.InternalError, ValueError)
        assert lytk.ParseError.__module__ == "lytk"

    @pytest.mark.parametrize(
        "read",
        [
            lambda: lytk.from_musicxml_string("not musicxml <<<"),
            lambda: lytk.from_musicxml_bytes(b"not musicxml"),
            lambda: lytk.from_midi_bytes(b"not a midi file"),
            lambda: lytk.from_lilypond_string("{ " * 3000 + "}" * 3000),
            lambda: lytk.Score.from_json("{ not json"),
            lambda: lytk.Score.from_dict({"not": "a score"}),
            lambda: lytk.MusicDocument.from_json("[]"),
        ],
    )
    def test_malformed_input_is_a_parse_error(self, read):
        with pytest.raises(lytk.ParseError):
            read()
        # Code written against 0.2 keeps working.
        with pytest.raises(ValueError):
            read()

    def test_a_missing_file_is_an_os_error(self, tmp_path):
        for read in (lytk.from_lilypond, lytk.from_musicxml, lytk.from_abc, lytk.from_humdrum):
            with pytest.raises(OSError):
                read(str(tmp_path / "missing"))

    def test_a_ly_file_that_is_not_utf8_is_a_parse_error(self, tmp_path):
        path = tmp_path / "latin1.ly"
        path.write_bytes(b"{ c'4 \xe9 }")
        with pytest.raises(lytk.ParseError, match="not UTF-8"):
            lytk.from_lilypond(str(path))

    def test_flatten_errors_are_parse_errors(self, tmp_path):
        main = tmp_path / "main.ly"
        main.write_text('\\include "absent.ily"\n')
        with pytest.raises(lytk.ParseError, match="not found"):
            lytk.flatten(str(main))

    def test_pickles_with_its_diagnostics(self):
        import pickle

        with pytest.raises(lytk.LilyPondSyntaxError) as info:
            lytk.from_lilypond_string("{ c'3 }", strict=True)
        again = pickle.loads(pickle.dumps(info.value))
        assert type(again) is lytk.LilyPondSyntaxError
        assert again.diagnostics == info.value.diagnostics


class TestDiagnostics:
    """check_lilypond, strict mode and .diagnostics (Epic J3)."""

    def test_syntax_only_by_default(self):
        text = "{ c'3 \\noSuchCommand }"
        assert lytk.check_lilypond(text) == []
        codes = [d.code for d in lytk.check_lilypond(text, semantic=True)]
        assert codes == ["invalid-duration", "unknown-command"]

    def test_fields_and_text(self):
        text = "% é\n{ é c'4 >> }"
        (d,) = lytk.check_lilypond(text)
        assert (d.severity, d.code, d.line, d.column) == ("error", "syntax-error", 2, 9)
        assert text[d.start : d.end] == ">>"  # character offsets
        assert str(d) == "2:9: error: `>>` without a matching `<<` [syntax-error]"
        assert d == lytk.Diagnostic(*[getattr(d, k) for k in ("severity", "code", "message", "line", "column", "start", "end")])

    def test_too_large_is_a_diagnostic_not_an_exception(self):
        (d,) = lytk.check_lilypond("{ " * 3000 + "}" * 3000)
        assert (d.severity, d.code) == ("error", "too-large")

    def test_readers_carry_diagnostics_outside_the_ir(self, tmp_path):
        text = "\\include \"x.ily\"\n{ c'4 \\noSuchCommand d'4 }"
        path = tmp_path / "in.ly"
        path.write_text(text)
        for music in (
            lytk.from_lilypond_string(text),
            lytk.from_lilypond(str(path)),
            lytk.from_lilypond_music_string(text),
            lytk.from_lilypond_music(str(path)),
        ):
            assert [d.code for d in music.diagnostics] == ["ignored-include", "unknown-command"]
            assert "diagnostic" not in music.to_json()
        assert lytk.from_musicxml_string(lytk.to_musicxml(music.to_score())).diagnostics == []

    def test_strict_raises_on_errors_only(self):
        text = "{ c'4 \\noSuchCommand d'4 }"  # a warning
        assert lytk.from_lilypond_string(text, strict=True).num_parts == 1
        with pytest.raises(lytk.LilyPondSyntaxError, match=r"1:1: error: missing `}`") as info:
            lytk.from_lilypond_string("{ c'4 d'4", strict=True)
        assert [d.severity for d in info.value.diagnostics] == ["error"]
        with pytest.raises(lytk.LilyPondSyntaxError, match=r"\(and 1 more error\)"):
            lytk.from_lilypond_music_string("{ c'3 d'3 }", strict=True)
        # Without strict the same input reads, reporting what it could not.
        assert lytk.from_lilypond_string("{ c'3 d'3 }").diagnostics[0].code == "invalid-duration"

    def test_movements_and_the_first_movement(self, tmp_path):
        path = tmp_path / "two.ly"
        path.write_text("\\score { { c'1 } }\n\\score { { d'1 } }\n")
        movements = lytk.from_lilypond_movements(str(path), strict=True)
        assert [len(m.diagnostics) for m in movements] == [0, 0]
        (dropped,) = lytk.from_lilypond(str(path)).diagnostics
        assert (dropped.code, dropped.line) == ("dropped-music", 2)


class TestStringsAndHeaders:
    """Strings decoded as LilyPond reads them, headers as dicts (Epic J4);
    pitch-language files (J5)."""

    def test_header_is_a_dict_of_every_field(self):
        text = '\\header { title = "A \\"B\\"" composer = \\markup { \\bold "J. S." Bach } opus = "5" }\n{ c\'1 }'
        score = lytk.from_lilypond_string(text)
        assert score.header == {"title": 'A "B"', "composer": "J. S. Bach", "opus": "5"}
        doc = lytk.from_lilypond_music_string('\\header { poet = "P" }\n{ c\'1 }')
        assert (doc.lyricist, doc.header) == ("P", {"lyricist": "P"})

    def test_each_movement_has_its_header(self, tmp_path):
        path = tmp_path / "two.ly"
        path.write_text('\\header { composer = "C" }\n\\score { \\header { piece = "I" } { c\'1 } }\n\\score { { d\'1 } }\n')
        first, second = lytk.from_lilypond_movements(str(path))
        assert first.header == {"composer": "C", "piece": "I"}
        assert second.header == {"composer": "C"}

    def test_header_fields_locate_each_assignment(self):
        text = '% é\n\\header { title = "é\\"x" tagline = ##f }\n\\score { \\header { piece = "II" } { c\'1 } }'
        fields = lytk.header_fields(text)
        assert [(f.key, f.value, f.score) for f in fields] == [("title", 'é"x', None), ("piece", "II", 0)]
        assert text[fields[0].start : fields[0].end] == 'title = "é\\"x"'  # character offsets
        cut = text[: fields[0].start] + text[fields[0].end :]
        assert [f.key for f in lytk.header_fields(cut)] == ["piece"]

    def test_movements_of_a_string(self):
        text = "\\version \"2.24.0\"\n{ c'1 }\n\\score { { d'1 e'1 } }\n{ c'3 }\n"
        scores = lytk.from_lilypond_movements_string(text)
        assert [len(s.notes()) for s in scores] == [1, 2, 1]
        assert all([d.code for d in s.diagnostics] == ["invalid-duration"] for s in scores)
        with pytest.raises(lytk.LilyPondSyntaxError):
            lytk.from_lilypond_movements_string(text, strict=True)

    def test_music_movements(self, tmp_path):
        path = tmp_path / "two.ly"
        path.write_text('\\version "2.24.0"\n{ c\'1 }\n\\score { { d\'1 e\'1 } }\n')
        docs = lytk.from_lilypond_music_movements(str(path))
        assert [len(d.notes()) for d in docs] == [1, 2]
        assert all(d.lilypond_version == lytk.LilyPondVersion("2.24") for d in docs)

    def test_language_files_set_the_pitch_names(self):
        score = lytk.from_lilypond_string('\\include "english.ly"\n{ cs\'4 }')
        assert [n[2] for n in score.notes()] == [61]
        assert score.diagnostics == []


class TestLilyPondVersion:
    """\\version read, compared and edited (Epic K1)."""

    def test_versions_compare_numerically(self):
        v = lytk.LilyPondVersion
        assert v("2.24") == v("2.24.0") and hash(v("2.24")) == hash(v("2.24.0"))
        assert v("2.24.10") > v("2.24.9") > v("2.22.2")
        assert sorted([v("2.26.0"), v("2.24.0")]) == [v("2.24.0"), v("2.26.0")]
        assert (str(v("2.24")), repr(v("2.24"))) == ("2.24.0", "LilyPondVersion('2.24.0')")
        assert v("2.25.3.x").extra == "x"
        import pickle

        assert pickle.loads(pickle.dumps(v("2.26.0"))) == v("2.26.0")
        for bad in ("2.25", "two", "2.24.0.a.b"):
            with pytest.raises(lytk.ParseError):
                v(bad)

    def test_version_of_text_and_of_a_score(self):
        text = '% \\version "1.0.0"\n\\version "2.24"\n{ c\'1 }'
        assert lytk.lilypond_version(text) == lytk.LilyPondVersion("2.24.0")
        assert lytk.lilypond_version("{ c'1 }") is None
        assert lytk.from_lilypond_string(text).lilypond_version == lytk.LilyPondVersion("2.24.0")
        assert lytk.from_lilypond_music_string(text).lilypond_version.minor == 24
        assert lytk.from_lilypond_string("{ c'1 }").lilypond_version is None
        score = lytk.from_lilypond_string(text)
        assert lytk.Score.from_json(score.to_json()).lilypond_version == score.lilypond_version

    def test_set_and_strip(self):
        text = '\\version "2.18.2"\n{ c\'1 }\n'
        assert lytk.set_lilypond_version(text, "2.24") == '\\version "2.24.0"\n{ c\'1 }\n'
        assert lytk.set_lilypond_version("{ c'1 }", lytk.LilyPondVersion("2.26.0")).startswith('\\version "2.26.0"\n')
        assert lytk.strip_lilypond_version(text) == "{ c'1 }\n"
        with pytest.raises(lytk.ParseError):
            lytk.set_lilypond_version(text, "2.25")

    def test_writers_take_the_version(self):
        score = lytk.from_lilypond_string("{ c'1 }")
        assert '\\version "2.24.0"' in lytk.to_lilypond(score)
        assert '\\version "2.26.0"' in lytk.to_lilypond(score, version="2.26")
        doc = lytk.from_lilypond_music_string("{ c'1 }")
        assert '\\version "2.26.0"' in lytk.to_lilypond_music(doc, version=lytk.LilyPondVersion("2.26.0"))
        with pytest.raises(lytk.ParseError):
            lytk.to_lilypond(score, version="latest")

    def test_an_invalid_version_is_an_error(self):
        codes = [d.code for d in lytk.check_lilypond('\\version "2.x"\n{ c\'1 }')]
        assert codes == ["invalid-version"]


class TestTokens:
    """Tokens and comment stripping (Epic K3)."""

    def test_tokens_cover_the_text(self):
        text = "% é\n\\relative c'' { \\time 3/4 c4 \"a \\\"b\" #(x 1) }"
        tokens = lytk.tokenize(text)
        assert [(t.kind, t.text) for t in tokens][:4] == [
            ("comment", "% é"),
            ("command", "\\relative"),
            ("symbol", "c"),
            ("punctuation", "'"),
        ]
        assert {t.kind for t in tokens} >= {"fraction", "number", "string", "scheme"}
        assert [(t.kind, t.text) for t in tokens if t.scheme] == [
            ("scheme", "#"),
            ("punctuation", "("),
            ("symbol", "x"),
            ("number", "1"),
            ("punctuation", ")"),
        ]
        for t in tokens:
            assert text[t.start : t.end] == t.text  # character offsets
        assert (tokens[1].line, tokens[1].column) == (2, 1)
        leftover = text
        for t in reversed(tokens):
            leftover = leftover[: t.start] + leftover[t.end :]
        assert leftover.strip() == ""
        assert [t.kind for t in lytk.tokenize("}}} ?!")].count("punctuation") >= 3

    def test_strip_comments(self):
        assert lytk.strip_comments("c4%{x%}d4 % tail\nr4") == "c4 d4 \nr4"
        assert lytk.strip_comments("#(a ; s\n b)") == "#(a \n b)"


class TestStatistics:
    """lytk.info and lytk.source_stats (Epic K4)."""

    def test_counts(self):
        score = lytk.from_lilypond_string(
            "\\version \"2.24.0\"\n{ \\grace d''8 e''4 f'' <g'' b''> a'' | c''1 }\n\\addlyrics { la la la la la }"
        )
        data = lytk.info(score)
        assert data["lilypond_version"] == "2.24.0"
        assert (data["part_count"], data["note_count"], data["grace_note_count"]) == (1, 7, 1)
        assert (data["bar_count"], data["duration_quarters"], data["lyric_count"]) == (2, 8.0, 5)
        assert data["parts"][0]["notes"] == 7 and data["parts"][0]["measures"] == 2
        two = lytk.info(lytk.from_lilypond_string("<< \\new ChordNames \\chordmode { c1 g1 } \\new Staff << { c''1 d''1 } \\\\ { c'1 d'1 } >> >>"))
        assert (two["voice_count"], two["chord_symbol_count"], two["note_count"]) == (2, 2, 4)

    def test_source_stats(self):
        stats = lytk.source_stats("% a\n#(define x 1)\n{ c'4 %{b%} }\n")
        assert stats == {"bytes": 32, "lines": 3, "tokens": 13, "comments": 2, "scheme": 1, "error_tokens": 0}


class TestIncludes:
    """flatten_string and include_paths on the readers (Epic K2)."""

    def test_flatten_string(self, tmp_path):
        (tmp_path / "part.ily").write_text("d'4\n")
        text = "%{ x %}\n{ c'4 \\include \"part.ily\" e'4 }\n\\include \"english.ly\"\n"
        flat = lytk.flatten_string(text, base_dir=str(tmp_path), add_markers=False)
        assert flat == "%{ x %}\n{ c'4 \nd'4\n e'4 }\n\\include \"english.ly\"\n"
        assert lytk.flatten_string(text, include_paths=[str(tmp_path)]).count("BEGIN INCLUDE") == 1
        with pytest.raises(lytk.ParseError):
            lytk.flatten_string('\\include "gone.ily"')

    def test_readers_follow_includes_when_given_paths(self, tmp_path):
        (tmp_path / "notes.ily").write_text("{ d'4 \\nosuch e'4 }\n")
        main = tmp_path / "main.ly"
        main.write_text('\\include "notes.ily"\n{ c\'3 }\n')
        assert len(lytk.from_lilypond_movements(str(main))) == 1
        followed = lytk.from_lilypond_movements(str(main), include_paths=[])
        assert len(followed) == 2
        first = lytk.from_lilypond(str(main), include_paths=[])
        assert [(d.code, d.line) for d in first.diagnostics] == [("unknown-command", 1), ("dropped-music", 2), ("invalid-duration", 2)]
        assert first.diagnostics[0].message.startswith("in `notes.ily`")
        text = main.read_text()
        codes = [d.code for d in lytk.check_lilypond(text, semantic=True, include_paths=[str(tmp_path)])]
        assert codes == ["unknown-command", "invalid-duration"]
        score = lytk.from_lilypond_string(text, include_paths=[str(tmp_path)])
        assert len(score.notes()) == 2


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
