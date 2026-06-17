"""lytk CLI — batch music notation conversion and augmentation.

This module provides the ``lytk`` console command.  It is a thin Python
wrapper around the compiled Rust extension (``lytk._core``), matching
the interface of the Rust ``main.rs`` binary.

Subcommands::

    lytk convert  <input> -o <output> [--format ly|xml|abc|midi] [--jobs N]
    lytk transpose <input> -o <output> --semitones N [--format ly|xml|abc|midi]
    lytk info     <input>
    lytk flatten  <input> [-o <output>] [-I <dir> ...] [--no-markers]
"""

from __future__ import annotations

import argparse
import os
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import lytk


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_SUPPORTED_EXTS: set[str] = {".ly", ".ily", ".xml", ".musicxml", ".mxl", ".abc"}
# MIDI is always built into the extension.
_has_midi = True
_SUPPORTED_EXTS |= {".mid", ".midi"}


def _parse_input(path: Path) -> lytk.Score:
    ext = path.suffix.lower()
    if ext in {".ly", ".ily"}:
        return lytk.from_lilypond(str(path))
    if ext in {".xml", ".musicxml", ".mxl"}:
        return lytk.from_musicxml(str(path))
    if ext == ".abc":
        return lytk.from_abc(str(path))
    if _has_midi and ext in {".mid", ".midi"}:
        return lytk.from_midi(str(path))
    print(f"error: unsupported input format: {ext}", file=sys.stderr)
    sys.exit(1)


def _invert_ext(path: Path) -> str:
    # LilyPond inverts to MusicXML; every other input inverts to LilyPond.
    if path.suffix.lower() in {".ly", ".ily"}:
        return ".xml"
    return ".ly"


def _write_output(
    score: lytk.Score,
    path: Path,
    fmt: str | None,
) -> None:
    if fmt is None:
        ext = path.suffix.lower()
    else:
        ext = f".{fmt}"

    if ext in {".ly", ".ily"}:
        lang = score.language
        lytk.to_lilypond(score, str(path), language=lang)
    elif ext in {".xml", ".musicxml"}:
        lytk.to_musicxml(score, str(path))
    elif ext == ".abc":
        lytk.to_abc(score, str(path))
    elif _has_midi and ext in {".mid", ".midi"}:
        lytk.to_midi(score, str(path))
    else:
        print(
            f"error: cannot infer output format from {path.suffix}; use --format",
            file=sys.stderr,
        )
        sys.exit(1)


def _collect_input_files(directory: Path) -> list[Path]:
    files: list[Path] = []
    for root, _dirs, names in os.walk(directory):
        for name in names:
            p = Path(root) / name
            if p.suffix.lower() in _SUPPORTED_EXTS:
                files.append(p)
    files.sort()
    return files


def _resolve_jobs(jobs: int) -> int:
    """Resolve the requested worker count (``0`` = auto → one per CPU)."""
    if jobs and jobs > 0:
        return jobs
    return os.cpu_count() or 1


def _convert_one(task: tuple[str, str, str | None]) -> tuple[str, str | None]:
    """Convert a single (input, output, format) task in a worker process.

    Returns ``(input_path, None)`` on success or ``(input_path, message)`` on
    failure, so one bad file never aborts the whole batch.
    """
    file_str, out_str, fmt = task
    try:
        score = _parse_input(Path(file_str))
        _write_output(score, Path(out_str), fmt)
        return (file_str, None)
    except Exception as exc:  # noqa: BLE001 - report per file, keep going
        return (file_str, str(exc) or exc.__class__.__name__)


# ---------------------------------------------------------------------------
# Subcommand implementations
# ---------------------------------------------------------------------------


def _run_convert(args: argparse.Namespace) -> None:
    inp = Path(args.input)
    out = Path(args.output)
    fmt = args.format

    if inp.is_dir():
        _run_batch(inp, out, fmt, args.jobs)
    else:
        score = _parse_input(inp)
        if fmt is None and out.suffix == "":
            out = out.with_suffix(_invert_ext(inp))
        _write_output(score, out, fmt)


def _run_batch(
    input_dir: Path,
    output_dir: Path,
    fmt: str | None,
    jobs: int = 0,
) -> None:
    files = _collect_input_files(input_dir)
    if not files:
        print(
            f"No supported files found in {input_dir}",
            file=sys.stderr,
        )
        return

    output_dir.mkdir(parents=True, exist_ok=True)

    # Build the (input, output, format) task list and pre-create output
    # subdirectories in the parent so workers never race on mkdir.
    tasks: list[tuple[str, str, str | None]] = []
    for file in files:
        try:
            relative = file.relative_to(input_dir)
        except ValueError:
            relative = Path(file.name)

        out_ext = _invert_ext(file) if fmt is None else f".{fmt}"
        out_path = (output_dir / relative).with_suffix(out_ext)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        tasks.append((str(file), str(out_path), fmt))

    workers = _resolve_jobs(jobs)
    if workers <= 1 or len(tasks) <= 1:
        results = [_convert_one(t) for t in tasks]
    else:
        # Conversion work is CPU-bound and lives in the Rust extension, so use
        # processes to get true parallelism without GIL contention.
        with ProcessPoolExecutor(max_workers=min(workers, len(tasks))) as pool:
            results = list(pool.map(_convert_one, tasks))

    failures = [(f, e) for f, e in results if e is not None]
    for file_str, error in failures:
        print(f"{file_str}: {error}", file=sys.stderr)

    print(f"Processed {len(files)} files", file=sys.stderr)

    # A batch where some files failed must not report success: automation
    # should be able to detect partial failures from the exit code.
    if failures:
        print(
            f"error: {len(failures)} of {len(files)} file(s) failed to convert",
            file=sys.stderr,
        )
        sys.exit(1)


def _run_transpose(args: argparse.Namespace) -> None:
    inp = Path(args.input)
    out = Path(args.output)
    score = _parse_input(inp)
    transposed = lytk.transpose(score, args.semitones)
    _write_output(transposed, out, args.format)


def _run_info(args: argparse.Namespace) -> None:
    inp = Path(args.input)
    score = _parse_input(inp)

    if score.title:
        print(f"Title:    {score.title}")
    if score.composer:
        print(f"Composer: {score.composer}")
    if score.subtitle:
        print(f"Subtitle: {score.subtitle}")
    if score.arranger:
        print(f"Arranger: {score.arranger}")
    if score.language:
        print(f"Language: {score.language}")

    parts = score.parts
    print(f"Parts:    {len(parts)}")
    for name in parts:
        print(f"  - {name}")


def _run_flatten(args: argparse.Namespace) -> None:
    text = lytk.flatten(
        args.input,
        args.output,
        include_paths=args.include or [],
        add_markers=not args.no_markers,
    )
    # The binding writes the file when --output is given; otherwise emit to stdout.
    if args.output is None:
        sys.stdout.write(text)


# ---------------------------------------------------------------------------
# Argument parser
# ---------------------------------------------------------------------------

def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="lytk",
        description="lytk — music notation conversion and augmentation toolkit.",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"lytk {_get_version()}",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    # -- convert -------------------------------------------------------------
    fmt_choices = ["ly", "xml", "abc"]
    if _has_midi:
        fmt_choices.append("midi")

    p_convert = sub.add_parser(
        "convert",
        help="Convert files between LilyPond, MusicXML, MXL, ABC, and MIDI formats.",
    )
    p_convert.add_argument("input", help="Input file or directory.")
    p_convert.add_argument("-o", "--output", required=True, help="Output file or directory.")
    p_convert.add_argument(
        "-f",
        "--format",
        choices=fmt_choices,
        default=None,
        help="Force output format (auto-detected from extension by default).",
    )
    p_convert.add_argument(
        "-j",
        "--jobs",
        type=int,
        default=0,
        help="Number of parallel worker processes for batch mode (0 = auto, one per CPU).",
    )
    p_convert.set_defaults(func=_run_convert)

    # -- transpose -----------------------------------------------------------
    p_transpose = sub.add_parser(
        "transpose",
        help="Transpose all pitches by a number of semitones.",
    )
    p_transpose.add_argument("input", help="Input file.")
    p_transpose.add_argument("-o", "--output", required=True, help="Output file.")
    p_transpose.add_argument(
        "-s",
        "--semitones",
        type=int,
        required=True,
        help="Semitones to transpose (positive = up, negative = down).",
    )
    p_transpose.add_argument(
        "-f",
        "--format",
        choices=fmt_choices,
        default=None,
        help="Force output format.",
    )
    p_transpose.set_defaults(func=_run_transpose)

    # -- info ----------------------------------------------------------------
    p_info = sub.add_parser(
        "info",
        help="Print score metadata (title, composer, parts, measures).",
    )
    p_info.add_argument("input", help="Input file.")
    p_info.set_defaults(func=_run_info)

    # -- flatten -------------------------------------------------------------
    p_flatten = sub.add_parser(
        "flatten",
        help="Recursively expand \\include directives into a single flat file.",
    )
    p_flatten.add_argument("input", help="Input LilyPond file.")
    p_flatten.add_argument(
        "-o",
        "--output",
        default=None,
        help="Output file (prints to stdout if omitted).",
    )
    p_flatten.add_argument(
        "-I",
        "--include",
        action="append",
        default=None,
        metavar="DIR",
        help="Extra directory to search for \\include files (repeatable).",
    )
    p_flatten.add_argument(
        "--no-markers",
        action="store_true",
        help="Suppress %% === BEGIN/END INCLUDE === comment markers.",
    )
    p_flatten.set_defaults(func=_run_flatten)

    return parser


def _get_version() -> str:
    try:
        from importlib.metadata import version

        return version("lytk")
    except Exception:
        return "0.0.0"


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> None:
    parser = _build_parser()
    args = parser.parse_args(argv)
    try:
        args.func(args)
    except Exception as exc:  # noqa: BLE001
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
