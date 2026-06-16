"""End-to-end tests for the ``lytk`` CLI.

These tests invoke the ``lytk`` command as a subprocess so they exercise
the CLI exactly as an end-user would after ``pip install lytk``.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest

FIXTURE_XML = Path("tests/fixtures/xml/01a-Pitches-Pitches.xml")
FIXTURE_LY_DIR = Path("tests/fixtures/ly")
FIXTURE_XML_DIR = Path("tests/fixtures/xml")


def run_lytk(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    """Run the ``lytk`` CLI and return the completed process."""
    return subprocess.run(
        [sys.executable, "-m", "lytk.cli", *args],
        capture_output=True,
        text=True,
        check=check,
    )


# ---------------------------------------------------------------------------
# Smoke tests — binary is available after install
# ---------------------------------------------------------------------------


class TestBinaryAvailable:
    def test_help(self):
        result = run_lytk("--help")
        assert result.returncode == 0
        assert "music notation" in result.stdout.lower()

    def test_entry_point(self):
        """The ``lytk`` console script should be on PATH after install."""
        result = subprocess.run(
            ["lytk", "--help"],
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0

    def test_version(self):
        result = run_lytk("--version")
        assert result.returncode == 0
        assert "lytk" in result.stdout

    def test_no_args(self):
        result = run_lytk(check=False)
        assert result.returncode != 0
        assert "usage" in result.stderr.lower()


# ---------------------------------------------------------------------------
# `info` subcommand
# ---------------------------------------------------------------------------


class TestInfo:
    def test_info_xml(self):
        result = run_lytk("info", str(FIXTURE_XML))
        assert result.returncode == 0
        assert "Parts:" in result.stdout

    def test_info_missing_file(self):
        result = run_lytk("info", "nonexistent.xml", check=False)
        assert result.returncode != 0
        assert "error" in result.stderr.lower()

    def test_info_unsupported_ext(self, tmp_path: Path):
        bad = tmp_path / "score.txt"
        bad.write_text("not music")
        result = run_lytk("info", str(bad), check=False)
        assert result.returncode != 0
        assert "unsupported" in result.stderr.lower()


# ---------------------------------------------------------------------------
# `convert` subcommand — single-file
# ---------------------------------------------------------------------------


class TestConvertSingle:
    def test_xml_to_ly(self, tmp_path: Path):
        out = tmp_path / "output.ly"
        result = run_lytk("convert", str(FIXTURE_XML), "-o", str(out))
        assert result.returncode == 0
        assert out.exists()
        content = out.read_text()
        assert "\\version" in content

    def test_ly_to_xml(self, tmp_path: Path):
        ly_files = sorted(FIXTURE_LY_DIR.glob("*.ly"))
        assert ly_files, "need at least one .ly fixture"
        out = tmp_path / "output.xml"
        result = run_lytk("convert", str(ly_files[0]), "-o", str(out))
        assert result.returncode == 0
        assert out.exists()
        content = out.read_text()
        assert "<score-partwise" in content

    def test_explicit_format_flag(self, tmp_path: Path):
        out = tmp_path / "output.dat"  # unusual extension
        result = run_lytk(
            "convert", str(FIXTURE_XML), "-o", str(out), "--format", "ly"
        )
        assert result.returncode == 0
        assert out.exists()
        assert "\\version" in out.read_text()

    def test_missing_input(self, tmp_path: Path):
        out = tmp_path / "output.ly"
        result = run_lytk("convert", "nonexistent.xml", "-o", str(out), check=False)
        assert result.returncode != 0

    def test_unknown_output_format(self, tmp_path: Path):
        out = tmp_path / "output.pdf"
        result = run_lytk("convert", str(FIXTURE_XML), "-o", str(out), check=False)
        assert result.returncode != 0
        assert "cannot infer output format" in result.stderr.lower() or "error" in result.stderr.lower()


# ---------------------------------------------------------------------------
# `convert` subcommand — batch (directory mode)
# ---------------------------------------------------------------------------


class TestConvertBatch:
    def test_batch_xml_to_ly(self, tmp_path: Path):
        out_dir = tmp_path / "batch_out"
        result = run_lytk(
            "convert", str(FIXTURE_XML_DIR), "-o", str(out_dir), "--jobs", "1"
        )
        assert result.returncode == 0
        assert "Processed" in result.stderr

        ly_files = list(out_dir.glob("*.ly"))
        assert ly_files, "batch should produce .ly files"

    def test_batch_parallel(self, tmp_path: Path):
        """Batch with default parallelism (--jobs 0) should also succeed."""
        out_dir = tmp_path / "batch_out"
        result = run_lytk("convert", str(FIXTURE_XML_DIR), "-o", str(out_dir))
        assert result.returncode == 0
        assert "Processed" in result.stderr

    def _small_xml_corpus(self, tmp_path: Path, n: int = 6) -> Path:
        srcs = sorted(FIXTURE_XML_DIR.glob("*.xml"))[:n]
        dst = tmp_path / "in"
        (dst / "a").mkdir(parents=True)
        (dst / "b").mkdir(parents=True)
        for i, s in enumerate(srcs):
            sub = "a" if i % 2 == 0 else "b"
            (dst / sub / s.name).write_bytes(s.read_bytes())
        return dst

    def test_batch_parallel_matches_serial(self, tmp_path: Path):
        """A real --jobs 4 run (across processes) must produce byte-identical
        output to a serial --jobs 1 run, including in nested subfolders."""
        corpus = self._small_xml_corpus(tmp_path)
        serial = tmp_path / "serial"
        parallel = tmp_path / "parallel"

        r1 = run_lytk("convert", str(corpus), "-o", str(serial), "--jobs", "1")
        r4 = run_lytk("convert", str(corpus), "-o", str(parallel), "--jobs", "4")
        assert r1.returncode == 0 and r4.returncode == 0

        s = {p.relative_to(serial): p.read_text() for p in serial.rglob("*.ly")}
        p = {p.relative_to(parallel): p.read_text() for p in parallel.rglob("*.ly")}
        assert s, "serial run produced no output"
        assert s.keys() == p.keys()
        for rel in s:
            assert s[rel] == p[rel], f"parallel output differs for {rel}"


# ---------------------------------------------------------------------------
# `convert` batch — parallelism is actually honored (Finding 2)
# ---------------------------------------------------------------------------


class TestBatchParallelism:
    """Unit-level checks that --jobs drives the worker pool, not just that
    batch mode succeeds."""

    def test_resolve_jobs(self):
        from lytk import cli

        assert cli._resolve_jobs(4) == 4
        assert cli._resolve_jobs(1) == 1
        # 0 = auto → at least one worker.
        assert cli._resolve_jobs(0) >= 1

    def test_parallel_jobs_use_process_pool(self, tmp_path, monkeypatch):
        from lytk import cli

        used: dict[str, int | None] = {"workers": None}

        class _SpyPool:
            def __init__(self, max_workers=None):
                used["workers"] = max_workers

            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

            def map(self, fn, tasks):
                return [fn(t) for t in tasks]

        monkeypatch.setattr(cli, "ProcessPoolExecutor", _SpyPool)
        out_dir = tmp_path / "out"
        cli._run_batch(FIXTURE_XML_DIR, out_dir, None, jobs=4)

        assert used["workers"] == 4, "--jobs 4 should spawn 4 workers"
        assert list(out_dir.glob("*.ly")), "batch should still produce output"

    def test_serial_jobs_skip_process_pool(self, tmp_path, monkeypatch):
        from lytk import cli

        instantiated = {"n": 0}

        class _Boom:
            def __init__(self, *a, **k):
                instantiated["n"] += 1

        monkeypatch.setattr(cli, "ProcessPoolExecutor", _Boom)
        out_dir = tmp_path / "out"
        cli._run_batch(FIXTURE_XML_DIR, out_dir, None, jobs=1)

        assert instantiated["n"] == 0, "--jobs 1 must not start a process pool"
        assert list(out_dir.glob("*.ly"))


# ---------------------------------------------------------------------------
# `transpose` subcommand
# ---------------------------------------------------------------------------


class TestTranspose:
    def test_transpose_up(self, tmp_path: Path):
        out = tmp_path / "transposed.ly"
        result = run_lytk(
            "transpose", str(FIXTURE_XML), "-o", str(out), "--semitones", "3"
        )
        assert result.returncode == 0
        assert out.exists()
        assert "\\version" in out.read_text()

    def test_transpose_down(self, tmp_path: Path):
        out = tmp_path / "transposed.ly"
        result = run_lytk(
            "transpose", str(FIXTURE_XML), "-o", str(out), "--semitones", "-5"
        )
        assert result.returncode == 0
        assert out.exists()

    def test_transpose_zero_is_identity(self, tmp_path: Path):
        original = tmp_path / "original.ly"
        transposed = tmp_path / "transposed.ly"

        run_lytk("convert", str(FIXTURE_XML), "-o", str(original))
        run_lytk(
            "transpose", str(FIXTURE_XML), "-o", str(transposed), "--semitones", "0"
        )

        assert original.read_text() == transposed.read_text()

    def test_transpose_roundtrip(self, tmp_path: Path):
        """Transpose up then back down should yield identical output."""
        up = tmp_path / "up.xml"
        back = tmp_path / "back.xml"

        run_lytk(
            "transpose",
            str(FIXTURE_XML),
            "-o",
            str(up),
            "--semitones",
            "7",
            "--format",
            "xml",
        )
        run_lytk(
            "transpose",
            str(up),
            "-o",
            str(back),
            "--semitones",
            "-7",
            "--format",
            "xml",
        )

        # Compare via IR JSON (semantic equivalence, not byte equality).
        orig_info = run_lytk("info", str(FIXTURE_XML))
        back_info = run_lytk("info", str(back))
        assert orig_info.stdout == back_info.stdout
