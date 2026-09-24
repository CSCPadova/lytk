"""Tests for the ``lytk`` command line (``lytk.cli``).

Most tests drive the Typer app in-process with ``CliRunner``; a few run the
installed ``lytk`` console script, the way users call it.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest
from typer.testing import CliRunner

import lytk
from lytk import cli

FIXTURE_XML = Path("tests/fixtures/xml/01a-Pitches-Pitches.xml")
FIXTURE_LY_DIR = Path("tests/fixtures/ly")
FIXTURE_XML_DIR = Path("tests/fixtures/xml")

# A single-staff, single-voice snippet in absolute entry.
SIMPLE_LY = "\\version \"2.24.0\"\n{ c'4 e'4 g'4 }\n"


def run(*args: str, input: str | bytes | None = None):
    """Invoke the CLI in-process."""
    return CliRunner().invoke(cli.app, list(args), input=input)


def ok(*args: str, input: str | bytes | None = None):
    result = run(*args, input=input)
    assert result.exit_code == 0, result.stderr or result.stdout
    return result


@pytest.fixture
def simple_ly(tmp_path: Path) -> Path:
    path = tmp_path / "in.ly"
    path.write_text(SIMPLE_LY)
    return path


# ---------------------------------------------------------------------------
# The installed command
# ---------------------------------------------------------------------------


class TestEntryPoint:
    def test_console_script_on_path(self):
        result = subprocess.run(["lytk", "--help"], capture_output=True, text=True, check=False)
        assert result.returncode == 0
        assert "convert" in result.stdout

    def test_python_dash_m(self):
        result = subprocess.run(
            [sys.executable, "-m", "lytk.cli", "--version"], capture_output=True, text=True, check=False
        )
        assert result.returncode == 0
        assert result.stdout.startswith("lytk ")

    def test_help_lists_every_command(self):
        out = ok("--help").stdout
        for command in [
            "convert", "transpose", "invert", "retrograde", "change-language", "abs2rel",
            "rel2abs", "info", "positions", "bundle", "diff", "batch", "flatten",
        ]:
            assert command in out

    def test_no_args_shows_usage_and_fails(self):
        result = run()
        assert result.exit_code != 0
        assert "Usage" in result.output


# ---------------------------------------------------------------------------
# convert
# ---------------------------------------------------------------------------


class TestConvert:
    def test_xml_to_ly(self, tmp_path: Path):
        out = tmp_path / "output.ly"
        ok("convert", str(FIXTURE_XML), "-o", str(out))
        assert "\\version" in out.read_text()

    def test_ly_to_xml(self, tmp_path: Path):
        out = tmp_path / "output.xml"
        ok("convert", str(sorted(FIXTURE_LY_DIR.glob("*.ly"))[0]), "-o", str(out))
        assert "<score-partwise" in out.read_text()

    def test_explicit_format(self, tmp_path: Path):
        out = tmp_path / "output.dat"
        ok("convert", str(FIXTURE_XML), "-o", str(out), "--format", "ly")
        assert "\\version" in out.read_text()

    def test_missing_input(self, tmp_path: Path):
        result = run("convert", "nonexistent.xml", "-o", str(tmp_path / "o.ly"))
        assert result.exit_code == 1
        assert "no such file: nonexistent.xml" in result.stderr

    def test_unsupported_input(self, tmp_path: Path):
        bad = tmp_path / "score.txt"
        bad.write_text("not music")
        result = run("info", str(bad))
        assert result.exit_code == 1
        assert "unsupported input format" in result.stderr

    def test_unknown_output_format(self, tmp_path: Path):
        result = run("convert", str(FIXTURE_XML), "-o", str(tmp_path / "output.pdf"))
        assert result.exit_code == 1
        assert "cannot infer output format" in result.stderr

    @pytest.mark.parametrize("ext", ["mid", "abc", "krn", "xml", "mxl"])
    def test_every_output_format(self, tmp_path: Path, ext: str):
        out = tmp_path / f"out.{ext}"
        ok("convert", str(FIXTURE_XML), "-o", str(out))
        assert out.stat().st_size > 0

    def test_midi_to_ly_and_xml(self, tmp_path: Path):
        mid = tmp_path / "in.mid"
        ok("convert", str(FIXTURE_XML), "-o", str(mid))
        ok("convert", str(mid), "-o", str(tmp_path / "back.ly"))
        ok("convert", str(mid), "-o", str(tmp_path / "back.xml"))
        assert "Parts:" in ok("info", str(mid)).stdout

    def test_abc_both_ways(self, tmp_path: Path, simple_ly: Path):
        abc = tmp_path / "t.abc"
        ok("convert", str(simple_ly), "-o", str(abc))
        assert "X:" in abc.read_text()
        ok("convert", str(abc), "-o", str(tmp_path / "back.ly"))

    def test_kern_both_ways_and_transform(self, tmp_path: Path):
        src = tmp_path / "t.krn"
        src.write_text("**kern\n*M4/4\n4c\n4e\n4g\n4cc\n*-\n")
        ly = tmp_path / "t.ly"
        ok("convert", str(src), "-o", str(ly))
        back = tmp_path / "back.krn"
        ok("convert", str(ly), "-o", str(back))
        text = back.read_text()
        assert "**kern" in text and "4cc" in text
        up = tmp_path / "up.krn"
        ok("transpose", str(src), "-o", str(up), "-s", "2")
        assert "4d" in up.read_text()

    def test_mxl_is_compressed_and_reads_back(self, tmp_path: Path, simple_ly: Path):
        out = tmp_path / "t.mxl"
        ok("convert", str(simple_ly), "-o", str(out))
        assert out.read_bytes()[:4] == b"PK\x03\x04"
        ok("diff", str(simple_ly), str(out))

    def test_several_movements(self, tmp_path: Path):
        """The first movement goes to the requested path, the rest beside it."""
        src = tmp_path / "two.ly"
        src.write_text(r"\score { \new Staff { c'1 } } \score { \new Staff { d'1 } }")
        out = tmp_path / "out.xml"
        result = ok("convert", str(src), "-o", str(out))
        assert out.exists() and (tmp_path / "out_02.xml").exists()
        assert "2 movements" in result.stderr

    def test_several_movements_refuse_stdout(self, tmp_path: Path):
        src = tmp_path / "two.ly"
        src.write_text(r"\score { \new Staff { c'1 } } \score { \new Staff { d'1 } }")
        result = run("convert", str(src), "-o", "-", "-f", "xml")
        assert result.exit_code == 1
        assert "stdout" in result.stderr

    def test_ly_to_ly_keeps_structure(self, tmp_path: Path):
        """LilyPond to LilyPond goes through the Music tree, not measures."""
        src = tmp_path / "m.ly"
        src.write_text(r"\score { \new Staff { \key c \major c'4 d' e' f' } }")
        out = tmp_path / "up.ly"
        ok("transpose", str(src), "-o", str(out), "-s", "2")
        text = out.read_text()
        assert "\\key d \\major" in text
        assert "pB =" not in text


class TestStreams:
    def test_stdin_to_stdout(self):
        result = ok("convert", "-", "-o", "-", "--from", "ly", "-f", "xml", input=SIMPLE_LY)
        assert "score-partwise" in result.stdout

    def test_binary_formats_to_stdout(self):
        assert ok("convert", "-", "-o", "-", "--from", "ly", "-f", "midi", input=SIMPLE_LY).stdout_bytes[:4] == b"MThd"
        assert ok("convert", "-", "-o", "-", "--from", "ly", "-f", "mxl", input=SIMPLE_LY).stdout_bytes[:2] == b"PK"

    def test_binary_formats_from_stdin(self, tmp_path: Path):
        mid = tmp_path / "in.mid"
        ok("convert", str(FIXTURE_XML), "-o", str(mid))
        result = ok("convert", "-", "-o", "-", "--from", "midi", "-f", "ly", input=mid.read_bytes())
        assert "\\version" in result.stdout

    def test_stdin_needs_from(self, tmp_path: Path):
        result = run("convert", "-", "-o", str(tmp_path / "out.xml"), input=SIMPLE_LY)
        assert result.exit_code == 1
        assert "requires --from" in result.stderr

    def test_stdout_needs_format(self):
        result = run("convert", str(FIXTURE_XML), "-o", "-")
        assert result.exit_code == 1
        assert "requires --format" in result.stderr


# ---------------------------------------------------------------------------
# convert — a directory
# ---------------------------------------------------------------------------


def _small_corpus(tmp_path: Path, n: int = 6) -> Path:
    corpus = tmp_path / "in"
    for i, src in enumerate(sorted(FIXTURE_XML_DIR.glob("*.xml"))[:n]):
        sub = corpus / ("a" if i % 2 == 0 else "b")
        sub.mkdir(parents=True, exist_ok=True)
        (sub / src.name).write_bytes(src.read_bytes())
    return corpus


class TestConvertDirectory:
    def test_converts_every_file(self, tmp_path: Path):
        out = tmp_path / "out"
        result = ok("convert", str(FIXTURE_XML_DIR), "-o", str(out), "--jobs", "1")
        assert "Processed" in result.stderr
        scores = [f for f in FIXTURE_XML_DIR.iterdir() if f.suffix in cli._EXT_FORMAT]
        assert len(list(out.glob("*.ly"))) == len(scores)

    def test_forced_format(self, tmp_path: Path):
        out = tmp_path / "out"
        ok("convert", str(_small_corpus(tmp_path)), "-o", str(out), "-f", "midi", "-j", "1")
        assert list(out.rglob("*.mid"))

    def test_partial_failure_exits_nonzero(self, tmp_path: Path):
        corpus = tmp_path / "in"
        corpus.mkdir()
        (corpus / "good.xml").write_bytes(FIXTURE_XML.read_bytes())
        (corpus / "bad.xml").write_text("this is not valid musicxml")
        out = tmp_path / "out"
        result = run("convert", str(corpus), "-o", str(out))
        assert result.exit_code == 1
        assert "Processed 2 files" in result.stderr
        assert "1 of 2 file(s) failed to convert" in result.stderr
        assert (out / "good.ly").exists()

    def test_parallel_matches_serial(self, tmp_path: Path):
        corpus = _small_corpus(tmp_path)
        serial, parallel = tmp_path / "serial", tmp_path / "parallel"
        ok("convert", str(corpus), "-o", str(serial), "--jobs", "1")
        ok("convert", str(corpus), "-o", str(parallel), "--jobs", "4")
        s = {p.relative_to(serial): p.read_text() for p in serial.rglob("*.ly")}
        p = {p.relative_to(parallel): p.read_text() for p in parallel.rglob("*.ly")}
        assert s and s == p


class TestParallelism:
    """--jobs drives the worker pool, rather than only succeeding."""

    def test_resolve_jobs(self):
        assert cli._resolve_jobs(4) == 4
        assert cli._resolve_jobs(1) == 1
        assert cli._resolve_jobs(0) >= 1

    def test_jobs_sets_pool_size(self, tmp_path: Path, monkeypatch):
        used: dict[str, int | None] = {}

        class SpyPool:
            def __init__(self, max_workers=None):
                used["workers"] = max_workers

            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

            def map(self, fn, tasks):
                return [fn(t) for t in tasks]

        monkeypatch.setattr(cli, "ProcessPoolExecutor", SpyPool)
        out = tmp_path / "out"
        cli._run_batch(FIXTURE_XML_DIR, out, None, jobs=4)
        assert used["workers"] == 4
        assert list(out.glob("*.ly"))

    def test_one_job_uses_no_pool(self, tmp_path: Path, monkeypatch):
        class Boom:
            def __init__(self, *a, **k):
                raise AssertionError("--jobs 1 must not start a process pool")

        monkeypatch.setattr(cli, "ProcessPoolExecutor", Boom)
        out = tmp_path / "out"
        cli._run_batch(FIXTURE_XML_DIR, out, None, jobs=1)
        assert list(out.glob("*.ly"))


# ---------------------------------------------------------------------------
# Transforms
# ---------------------------------------------------------------------------


class TestTranspose:
    def test_semitones_up_and_down(self, tmp_path: Path):
        ok("transpose", str(FIXTURE_XML), "-o", str(tmp_path / "up.ly"), "--semitones", "3")
        ok("transpose", str(FIXTURE_XML), "-o", str(tmp_path / "down.ly"), "--semitones", "-5")
        assert (tmp_path / "down.ly").exists()

    def test_zero_is_identity(self, tmp_path: Path):
        ok("convert", str(FIXTURE_XML), "-o", str(tmp_path / "a.ly"))
        ok("transpose", str(FIXTURE_XML), "-o", str(tmp_path / "b.ly"), "-s", "0")
        assert (tmp_path / "a.ly").read_text() == (tmp_path / "b.ly").read_text()

    def test_up_and_back_is_equal(self, tmp_path: Path):
        up, back = tmp_path / "up.xml", tmp_path / "back.xml"
        ok("transpose", str(FIXTURE_XML), "-o", str(up), "-s", "7")
        ok("transpose", str(up), "-o", str(back), "-s", "-7")
        ok("diff", str(FIXTURE_XML), str(back))

    def test_interval_and_key(self, simple_ly: Path):
        assert "e'4" in ok("transpose", str(simple_ly), "-o", "-", "-f", "ly", "--interval", "M3").stdout
        assert "fis'4" in ok("transpose", str(simple_ly), "-o", "-", "-f", "ly", "--to-key", "D").stdout

    def test_exactly_one_mode(self, simple_ly: Path):
        for extra in ([], ["-s", "2", "--interval", "M3"]):
            result = run("transpose", str(simple_ly), "-o", "-", "-f", "ly", *extra)
            assert result.exit_code == 1
            assert "exactly one" in result.stderr

    def test_bad_interval(self, simple_ly: Path):
        result = run("transpose", str(simple_ly), "-o", "-", "-f", "ly", "--interval", "Q9")
        assert result.exit_code == 1
        assert result.stderr.startswith("error:")


class TestOtherTransforms:
    def test_invert(self, simple_ly: Path):
        # c' e' g' mirrored around F#3: c e, gis, → absolute c gis, f,
        out = ok("invert", str(simple_ly), "-o", "-", "-f", "ly", "--axis", "fs3").stdout
        assert "c4" in out and "gis,4" in out and "f,4" in out

    def test_invert_bad_axis(self, simple_ly: Path):
        result = run("invert", str(simple_ly), "-o", "-", "-f", "ly", "--axis", "zzz")
        assert result.exit_code == 1
        assert "axis" in result.stderr

    @pytest.mark.parametrize(
        ("text", "expected"),
        [("c4", ("C", 0, 4)), ("fs3", ("F", 1, 3)), ("bf5", ("B", -1, 5)), ("C#4", ("C", 1, 4)), ("css2", ("C", 2, 2))],
    )
    def test_parse_axis(self, text: str, expected: tuple[str, int, int]):
        assert cli.parse_axis(text) == expected

    def test_retrograde(self, simple_ly: Path):
        out = ok("retrograde", str(simple_ly), "-o", "-", "-f", "ly").stdout
        assert out.index("g'4") < out.index("c'4")

    def test_change_language(self, simple_ly: Path):
        out = ok("change-language", str(simple_ly), "-o", "-", "-f", "ly", "-l", "italiano").stdout
        assert '\\language "italiano"' in out

    def test_change_language_unknown(self, simple_ly: Path):
        result = run("change-language", str(simple_ly), "-o", "-", "-f", "ly", "-l", "klingon")
        assert result.exit_code == 1
        assert "unknown language" in result.stderr

    def test_abs2rel_and_rel2abs(self, tmp_path: Path, simple_ly: Path):
        assert "\\relative" in ok("abs2rel", str(simple_ly), "-o", "-").stdout
        rel = tmp_path / "rel.ly"
        rel.write_text("\\relative c' { c4 d4 e4 }\n")
        assert "\\relative" not in ok("rel2abs", str(rel), "-o", "-").stdout

    def test_abs2rel_needs_lilypond(self):
        result = run("abs2rel", str(FIXTURE_XML), "-o", "-")
        assert result.exit_code == 1
        assert "LilyPond" in result.stderr


# ---------------------------------------------------------------------------
# Inspection
# ---------------------------------------------------------------------------


class TestInspect:
    def test_info(self):
        out = ok("info", str(FIXTURE_XML)).stdout
        assert "Title:    Pitches and accidentals" in out
        assert "Parts:    1" in out
        assert "(28 measures)" in out

    def test_info_json(self):
        data = json.loads(ok("info", str(FIXTURE_XML), "--json").stdout)
        assert data["part_count"] == 1
        assert data["note_count"] == len(lytk.from_musicxml(str(FIXTURE_XML)).iter_parts()[0].notes)
        assert data["parts"][0]["measures"] == 28

    def test_positions(self):
        data = json.loads(ok("positions", str(FIXTURE_XML)).stdout)
        assert data["unit"] == "quarter"
        bars = data["parts"][0]["measures"]
        assert bars[0] == {"number": 1, "start": 0.0, "duration": 4.0}
        assert bars[1]["start"] == 4.0

    def test_positions_grace_notes_take_no_time(self, tmp_path: Path):
        src = tmp_path / "g.ly"
        src.write_text("{ \\grace d'8 c'1 }")
        bars = json.loads(ok("positions", str(src)).stdout)["parts"][0]["measures"]
        assert bars[0]["duration"] == 4.0

    def test_diff_equal(self):
        assert "semantically equal" in ok("diff", str(FIXTURE_XML), str(FIXTURE_XML)).stdout

    def test_diff_differ(self, tmp_path: Path):
        t = tmp_path / "t.xml"
        ok("transpose", str(FIXTURE_XML), "-o", str(t), "-s", "2")
        result = run("diff", str(FIXTURE_XML), str(t), "--json")
        assert result.exit_code == 1
        assert json.loads(result.stdout)["equal"] is False

    def test_bundle(self, tmp_path: Path):
        out = tmp_path / "parts"
        ok("bundle", "tests/fixtures/xml/41a-MultiParts-Partorder.xml", "-o", str(out), "-f", "ly")
        files = sorted(p.name for p in out.glob("*.ly"))
        assert len(files) == 4
        assert files[0].startswith("41a-MultiParts-Partorder_")
        assert lytk.from_lilypond(str(out / files[0])).num_parts == 1


# ---------------------------------------------------------------------------
# batch
# ---------------------------------------------------------------------------


class TestBatchJobs:
    def _jobs(self, tmp_path: Path, spec: list[dict]) -> Path:
        path = tmp_path / "jobs.json"
        path.write_text(json.dumps(spec))
        return path

    def test_runs_jobs(self, tmp_path: Path):
        a, b = tmp_path / "out" / "a.ly", tmp_path / "out" / "b.ly"
        jobs = self._jobs(
            tmp_path,
            [
                {"in": str(FIXTURE_XML), "out": str(a), "format": "ly"},
                {"input": str(FIXTURE_XML), "output": str(b), "interval": "M3"},
            ],
        )
        ok("batch", str(jobs), "-j", "1")
        assert a.exists() and b.exists()
        assert a.read_text() != b.read_text()

    def test_partial_failure_and_report(self, tmp_path: Path):
        good, report = tmp_path / "good.ly", tmp_path / "report.json"
        jobs = self._jobs(
            tmp_path,
            [
                {"in": str(FIXTURE_XML), "out": str(good), "format": "ly"},
                {"in": "nonexistent.xml", "out": str(tmp_path / "bad.ly"), "format": "ly"},
            ],
        )
        result = run("batch", str(jobs), "-j", "2", "--report", str(report))
        assert result.exit_code == 1
        assert "1 of 2 job(s) failed" in result.stderr
        assert good.exists()
        assert [r["ok"] for r in json.loads(report.read_text())] == [True, False]

    def test_invalid_file(self, tmp_path: Path):
        bad = tmp_path / "jobs.json"
        bad.write_text("{not json")
        result = run("batch", str(bad))
        assert result.exit_code == 1
        assert "invalid batch job file" in result.stderr


# ---------------------------------------------------------------------------
# flatten
# ---------------------------------------------------------------------------


class TestFlatten:
    def _project(self, tmp_path: Path, included: str = "{ c'4 d'4 }\n") -> Path:
        (tmp_path / "inc.ily").write_text(included)
        main = tmp_path / "main.ly"
        main.write_text('\\include "inc.ily"\n')
        return main

    def test_to_file(self, tmp_path: Path):
        out = tmp_path / "flat.ly"
        ok("flatten", str(self._project(tmp_path)), "-o", str(out))
        text = out.read_text()
        assert "c'4 d'4" in text and "\\include" not in text

    def test_to_stdout(self, tmp_path: Path):
        assert "e'4" in ok("flatten", str(self._project(tmp_path, "{ e'4 }\n"))).stdout

    def test_no_markers(self, tmp_path: Path):
        main = self._project(tmp_path, "{ g'4 }\n")
        assert "BEGIN INCLUDE" in ok("flatten", str(main)).stdout
        without = ok("flatten", str(main), "--no-markers").stdout
        assert "BEGIN INCLUDE" not in without and "g'4" in without

    def test_include_path(self, tmp_path: Path):
        lib = tmp_path / "lib"
        lib.mkdir()
        (lib / "inc.ily").write_text("{ a'4 }\n")
        main = tmp_path / "main.ly"
        main.write_text('\\include "inc.ily"\n')
        assert "a'4" in ok("flatten", str(main), "-I", str(lib)).stdout
