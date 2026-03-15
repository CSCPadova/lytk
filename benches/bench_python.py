"""Python baseline benchmarks comparing lytk (Rust) vs python-ly (pure Python).

Run with:
    pytest benches/bench_python.py -v --benchmark-columns=mean,stddev,rounds
    pytest benches/bench_python.py -v --benchmark-json=benches/results.json

These benchmarks measure the same operations in both lytk (Rust via PyO3) and
python-ly (pure Python) to quantify the speedup from the Rust implementation.
"""

from __future__ import annotations

import textwrap
from pathlib import Path

import pytest

# ---------------------------------------------------------------------------
# python-ly imports
# ---------------------------------------------------------------------------
from ly.document import Cursor, Document
from ly.pitch import Pitch as LyPitch
from ly.pitch.translate import translate as ly_translate
from ly.pitch.transpose import Transposer, transpose as ly_transpose

# ---------------------------------------------------------------------------
# lytk (Rust) imports
# ---------------------------------------------------------------------------
import lytk

# ---------------------------------------------------------------------------
# Fixtures / constants
# ---------------------------------------------------------------------------

MXML_PITCHES = Path("tests/fixtures/xml/xmlFiles/01a-Pitches-Pitches.xml")
MXML_MULTI = Path("tests/fixtures/xml/xmlFiles/41a-MultiParts-Partorder.xml")
MXML_STAFFGROUPS = Path("tests/fixtures/xml/xmlFiles/41c-StaffGroups.xml")
LY_RELATIVE = Path("tests/fixtures/ly/relative-repeat.ly")

# A realistic LilyPond fragment for python-ly benchmarks (no IR needed)
LY_SNIPPET = textwrap.dedent(r"""
\version "2.24.0"
\language "nederlands"
\header { title = "Benchmark" }
\relative c' {
  c4 d e f | g a b c |
  d,8 e fis g a b cis d |
  e,16 fis gis a b cis dis e fis gis ais b cis dis eis fis |
  c,2 g' | a4. b8 c2 |
  \repeat unfold 4 { c,8 e g c e g c e }
  fis,,4 gis ais b | cis dis eis fis |
  bes,2 ees | aes4 des ges ces |
}
""").strip()

# Scale up the snippet for more meaningful timings
LY_LARGE = "\n".join([LY_SNIPPET] * 20)


# ===================================================================
# Parsing benchmarks
# ===================================================================


class TestParseBenchmarks:
    """Compare document/score parsing speed."""

    def test_parse_ly_python_ly(self, benchmark):
        """python-ly: tokenise a LilyPond string into a Document."""
        benchmark(Document, LY_LARGE)

    def test_parse_ly_lytk(self, benchmark):
        """lytk (Rust): parse a LilyPond string into a Score IR."""
        benchmark(lytk.from_lilypond_string, LY_LARGE)

    def test_parse_mxml_lytk(self, benchmark):
        """lytk (Rust): parse a MusicXML file into a Score IR."""
        benchmark(lytk.from_musicxml, str(MXML_PITCHES))

    def test_parse_mxml_string_lytk(self, benchmark):
        """lytk (Rust): parse a MusicXML string into a Score IR."""
        xml = MXML_PITCHES.read_text()
        benchmark(lytk.from_musicxml_string, xml)


# ===================================================================
# Transpose benchmarks
# ===================================================================


class TestTransposeBenchmarks:
    """Compare transposition speed."""

    def test_transpose_python_ly(self, benchmark):
        """python-ly: transpose an entire document up a major third."""

        def do_transpose():
            doc = Document(LY_LARGE)
            cursor = Cursor(doc, 0, None)
            t = Transposer(LyPitch(0, 0, 0), LyPitch(2, 0, 0))  # C → E
            ly_transpose(cursor, t)
            return doc.plaintext()

        benchmark(do_transpose)

    def test_transpose_lytk(self, benchmark):
        """lytk (Rust): transpose a Score by 4 semitones (major third)."""
        score = lytk.from_lilypond_string(LY_LARGE)

        def do_transpose():
            return lytk.transpose(score, semitones=4)

        benchmark(do_transpose)


# ===================================================================
# Language translation benchmarks
# ===================================================================


class TestLanguageBenchmarks:
    """Compare pitch language translation speed."""

    def test_translate_python_ly(self, benchmark):
        """python-ly: translate pitch names from nederlands to deutsch."""

        def do_translate():
            doc = Document(LY_LARGE)
            cursor = Cursor(doc, 0, None)
            ly_translate(cursor, "deutsch")
            return doc.plaintext()

        benchmark(do_translate)

    def test_translate_lytk(self, benchmark):
        """lytk (Rust): change pitch language to deutsch."""
        score = lytk.from_lilypond_string(LY_LARGE)

        def do_translate():
            return lytk.change_language(score, "deutsch")

        benchmark(do_translate)


# ===================================================================
# Emission benchmarks (lytk only — python-ly has no equivalent)
# ===================================================================


class TestEmitBenchmarks:
    """Emission benchmarks — lytk only (python-ly doesn't emit from IR)."""

    def test_emit_lilypond_lytk(self, benchmark):
        """lytk (Rust): emit Score as LilyPond string."""
        score = lytk.from_musicxml(str(MXML_PITCHES))
        benchmark(lytk.to_lilypond, score)

    def test_emit_musicxml_lytk(self, benchmark):
        """lytk (Rust): emit Score as MusicXML string."""
        score = lytk.from_musicxml(str(MXML_PITCHES))
        benchmark(lytk.to_musicxml, score)


# ===================================================================
# Round-trip benchmarks
# ===================================================================


class TestRoundtripBenchmarks:
    """Full pipeline benchmarks."""

    def test_roundtrip_mxml_to_ly_lytk(self, benchmark):
        """lytk (Rust): MusicXML → IR → LilyPond."""
        xml = MXML_PITCHES.read_text()

        def do_roundtrip():
            score = lytk.from_musicxml_string(xml)
            return lytk.to_lilypond(score)

        benchmark(do_roundtrip)

    def test_pipeline_transpose_lytk(self, benchmark):
        """lytk (Rust): MusicXML → IR → transpose → LilyPond."""
        xml = MXML_PITCHES.read_text()

        def do_pipeline():
            score = lytk.from_musicxml_string(xml)
            score = lytk.transpose(score, semitones=5)
            return lytk.to_lilypond(score)

        benchmark(do_pipeline)

    def test_pipeline_language_lytk(self, benchmark):
        """lytk (Rust): parse LilyPond → change language → emit LilyPond."""
        def do_pipeline():
            score = lytk.from_lilypond_string(LY_LARGE)
            score = lytk.change_language(score, "english")
            return lytk.to_lilypond(score)

        benchmark(do_pipeline)

    def test_pipeline_language_python_ly(self, benchmark):
        """python-ly: parse → translate to english → plaintext."""
        def do_pipeline():
            doc = Document(LY_LARGE)
            cursor = Cursor(doc, 0, None)
            ly_translate(cursor, "english")
            return doc.plaintext()

        benchmark(do_pipeline)


# ===================================================================
# Transform benchmarks (lytk only — python-ly has no invert/retrograde)
# ===================================================================


class TestTransformBenchmarks:
    """Additional transform benchmarks — lytk only."""

    def test_invert_lytk(self, benchmark):
        """lytk (Rust): invert a Score around C4."""
        score = lytk.from_lilypond_string(LY_LARGE)
        benchmark(lytk.invert, score)

    def test_retrograde_lytk(self, benchmark):
        """lytk (Rust): retrograde (reverse note order)."""
        score = lytk.from_lilypond_string(LY_LARGE)
        benchmark(lytk.retrograde, score)
