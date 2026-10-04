"""The ``lytk`` command line.

Converts, transforms and inspects scores in LilyPond, MusicXML (``.xml`` and
compressed ``.mxl``), MIDI, ABC and Humdrum ``**kern``. Every command takes
``-`` for stdin/stdout; reading stdin needs ``--from`` and writing stdout needs
``--format``, since there is no file extension to go by.

The work is done by the compiled extension (``lytk._core``); this module only
parses arguments, picks the reader and writer, and reports errors.
"""

from __future__ import annotations

import functools
import json
import os
import re
import sys
from concurrent.futures import ProcessPoolExecutor
from enum import Enum
from fractions import Fraction
from pathlib import Path
from typing import Annotated, Any, Callable, Optional

import typer

import lytk

app = typer.Typer(
    name="lytk",
    help=(
        "Convert, transform and inspect symbolic music: LilyPond, MusicXML, "
        "MIDI, ABC and Humdrum **kern."
    ),
    no_args_is_help=True,
    add_completion=False,
    pretty_exceptions_enable=False,
)


class Format(str, Enum):
    """A score format, as given to ``--format`` / ``--from``."""

    ly = "ly"
    xml = "xml"
    mxl = "mxl"
    midi = "midi"
    abc = "abc"
    krn = "krn"


_EXT_FORMAT = {
    ".ly": "ly",
    ".ily": "ly",
    ".xml": "xml",
    ".musicxml": "xml",
    ".mxl": "mxl",
    ".mid": "midi",
    ".midi": "midi",
    ".abc": "abc",
    ".krn": "krn",
    ".kern": "krn",
}
_FORMAT_EXT = {"ly": ".ly", "xml": ".xml", "mxl": ".mxl", "midi": ".mid", "abc": ".abc", "krn": ".krn"}
# Spellings accepted for a format in a batch-job file.
_FORMAT_NAMES = {
    **{f.value: f.value for f in Format},
    "ily": "ly",
    "lilypond": "ly",
    "musicxml": "xml",
    "mid": "midi",
    "kern": "krn",
    "humdrum": "krn",
}
_FORMATS = "ly|xml|mxl|midi|abc|krn"


class CliError(Exception):
    """A user-facing error: reported as ``error: <message>``, exit status 1."""


# ---------------------------------------------------------------------------
# Reading and writing
# ---------------------------------------------------------------------------


def _value(fmt: Format | str | None) -> str | None:
    return fmt.value if isinstance(fmt, Format) else fmt


def _input_format(path: str, forced: Format | str | None = None) -> str:
    if forced is not None:
        return _value(forced)  # type: ignore[return-value]
    if path == "-":
        raise CliError(f"reading from stdin (`-`) requires --from <{_FORMATS}>")
    suffix = Path(path).suffix.lower()
    try:
        return _EXT_FORMAT[suffix]
    except KeyError:
        raise CliError(f"unsupported input format: {suffix or path}") from None


def _output_format(path: str, forced: Format | str | None = None) -> str:
    if forced is not None:
        return _value(forced)  # type: ignore[return-value]
    if path == "-":
        raise CliError(f"writing to stdout (`-`) requires --format <{_FORMATS}>")
    suffix = Path(path).suffix.lower()
    try:
        return _EXT_FORMAT[suffix]
    except KeyError:
        raise CliError(f"cannot infer output format from {suffix or path!r}; use --format") from None


def _parse_bytes(data: bytes, fmt: str, include: list[str] | None = None) -> lytk.Score:
    if fmt in ("xml", "mxl"):
        return lytk.from_musicxml_bytes(data)  # plain or zipped
    if fmt == "midi":
        return lytk.from_midi_bytes(data)
    text = data.decode("utf-8")
    if fmt == "ly":
        return lytk.from_lilypond_string(text, include_paths=include)
    parse = {"abc": lytk.from_abc_string, "krn": lytk.from_humdrum_string}
    return parse[fmt](text)


def _report(source: str, diagnostics: list[Any]) -> None:
    """What reading LilyPond found, on stderr: ``FILE:LINE:COLUMN: SEVERITY:
    MESSAGE [CODE]``, as ``lytk check`` prints it."""
    name = "<stdin>" if source == "-" else source
    for d in diagnostics:
        _note(f"{name}:{d}")


def read_score(path: str, forced: Format | str | None = None, include: list[str] | None = None) -> lytk.Score:
    """Read one score from a file, or stdin for ``-``; LilyPond's findings go
    to stderr. ``include``: directories to search for ``\\include`` files."""
    fmt = _input_format(path, forced)
    if path == "-":
        score = _parse_bytes(sys.stdin.buffer.read(), fmt, include)
    elif not Path(path).is_file():
        raise CliError(f"no such file: {path}")
    elif fmt == "ly":
        score = lytk.from_lilypond(path, include_paths=include)
    else:
        read = {
            "xml": lytk.from_musicxml,
            "mxl": lytk.from_musicxml,
            "midi": lytk.from_midi,
            "abc": lytk.from_abc,
            "krn": lytk.from_humdrum,
        }
        score = read[fmt](path)
    _report(path, score.diagnostics)
    return score


def _read_movements(path: str, forced: Format | None, include: list[str] | None = None) -> list[lytk.Score]:
    """Every movement: one per ``\\score`` block or top-level music expression
    of a LilyPond file, one per tune of an ABC file."""
    fmt = _input_format(path, forced)
    if path == "-" and fmt == "ly":
        movements = lytk.from_lilypond_movements_string(sys.stdin.buffer.read().decode("utf-8"), include_paths=include)
    elif path == "-" or fmt not in ("ly", "abc"):
        return [read_score(path, forced, include)]
    elif not Path(path).is_file():
        raise CliError(f"no such file: {path}")
    elif fmt == "ly":
        movements = lytk.from_lilypond_movements(path, include_paths=include)
    else:
        movements = lytk.from_abc_tunes(path)
    if not movements:
        return [read_score(path, forced, include)]
    # Every movement has the file's findings: report them once.
    _report(path, movements[0].diagnostics)
    return movements


def _read_music_movements(path: str, include: list[str] | None = None) -> list[lytk.MusicDocument]:
    """Every movement of a LilyPond file as a Music tree."""
    if path == "-":
        text = sys.stdin.buffer.read().decode("utf-8")
        scores = lytk.from_lilypond_movements_string(text, include_paths=include)
        docs = [s.to_music_document() for s in scores]
    elif not Path(path).is_file():
        raise CliError(f"no such file: {path}")
    else:
        docs = lytk.from_lilypond_music_movements(path, include_paths=include)
    if not docs:
        raise CliError(f"no music in {path}")
    _report(path, docs[0].diagnostics)
    return docs


def _write_movements(items: list[bytes], output: str, ext: str) -> None:
    """Write one movement to ``output``, several as ``output``, then
    ``NAME_02.EXT``, ``NAME_03.EXT``, … beside it."""
    if len(items) == 1:
        write_bytes(output, items[0])
        return
    if output == "-":
        raise CliError(f"{len(items)} movements parsed; cannot write several movements to stdout, use a file path")
    first = Path(output)
    for i, data in enumerate(items, start=1):
        path = first if i == 1 else first.with_name(f"{first.stem}_{i:02d}{first.suffix or ext}")
        write_bytes(str(path), data)
        _note(f"Wrote movement {i} → {path}")
    _note(f"note: input contains {len(items)} movements; movements 2+ were written with _NN suffixes")


def render(score: lytk.Score, fmt: str) -> bytes:
    """A score in the given format, as bytes."""
    if fmt == "mxl":
        return lytk.to_mxl_bytes(score)
    if fmt == "midi":
        return lytk.to_midi_bytes(score)
    emit = {"ly": lytk.to_lilypond, "xml": lytk.to_musicxml, "abc": lytk.to_abc, "krn": lytk.to_humdrum}
    return emit[fmt](score).encode("utf-8")


def write_bytes(path: str, data: bytes) -> None:
    """Write to a file, or stdout for ``-``."""
    if path == "-":
        sys.stdout.flush()
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
    else:
        Path(path).write_bytes(data)


def write_score(score: lytk.Score, path: str, forced: Format | str | None = None) -> None:
    write_bytes(path, render(score, _output_format(path, forced)))


def _ly_to_ly(inp: str, out: str, fmt: Format | None, from_: Format | None) -> bool:
    """Both ends LilyPond? Those go through the Music tree, which keeps the
    source's contexts and structure instead of rebuilding it from measures."""
    try:
        return _input_format(inp, from_) == "ly" and _output_format(out, fmt) == "ly"
    except CliError:
        return False


def _transform(
    inp: str,
    out: str,
    fmt: Format | None,
    from_: Format | None,
    apply: Callable[[Any], Any],
    include: list[str] | None = None,
) -> None:
    """Read, apply a transform (they take a Score or a MusicDocument) to every
    movement, write."""
    if _ly_to_ly(inp, out, fmt, from_):
        docs = _read_music_movements(inp, include)
        items = [lytk.to_lilypond_music(apply(d)).encode("utf-8") for d in docs]
        _write_movements(items, out, ".ly")
        return
    out_fmt = _output_format(out, fmt)  # fail before parsing
    items = [render(apply(s), out_fmt) for s in _read_movements(inp, from_, include)]
    _write_movements(items, out, _FORMAT_EXT[out_fmt])


def _note(message: str) -> None:
    typer.echo(message, err=True)


# ---------------------------------------------------------------------------
# Command registration: one clean error line instead of a traceback
# ---------------------------------------------------------------------------


def _command(name: str | None = None) -> Callable[[Callable[..., None]], Callable[..., None]]:
    def register(fn: Callable[..., None]) -> Callable[..., None]:
        @functools.wraps(fn)
        def run(*args: Any, **kwargs: Any) -> None:
            try:
                fn(*args, **kwargs)
            except typer.Exit:
                raise
            except Exception as exc:  # bindings raise ValueError/OSError/…
                _note(f"error: {exc}")
                raise typer.Exit(1) from None
            except BaseException as exc:  # a Rust panic surfaces as BaseException
                if isinstance(exc, (KeyboardInterrupt, SystemExit)):
                    raise
                _note(f"error: internal error: {exc}")
                raise typer.Exit(1) from None

        app.command(name=name)(run)
        return fn

    return register


def _version(value: bool) -> None:
    if value:
        typer.echo(f"lytk {lytk.__version__}")
        raise typer.Exit()


@app.callback()
def _root(
    version: Annotated[
        bool,
        typer.Option("--version", callback=_version, is_eager=True, help="Show the version and exit."),
    ] = False,
) -> None:
    pass


Input = Annotated[str, typer.Argument(help="Input file (`-` for stdin).", show_default=False)]
Output = Annotated[str, typer.Option("--output", "-o", help="Output file (`-` for stdout).", show_default=False)]
FormatOpt = Annotated[
    Optional[Format],
    typer.Option("--format", "-f", help="Output format (default: from the extension; required for stdout)."),
]
FromOpt = Annotated[
    Optional[Format],
    typer.Option("--from", help="Input format (default: from the extension; required for stdin)."),
]
Jobs = Annotated[int, typer.Option("--jobs", "-j", help="Parallel workers (0 = one per CPU).")]
IncludeOpt = Annotated[
    Optional[list[str]],
    typer.Option("--include-path", "-I", help="Directory to search for LilyPond \\include files (repeatable)."),
]


# ---------------------------------------------------------------------------
# convert
# ---------------------------------------------------------------------------


@_command()
def convert(
    input: Input,
    output: Output,
    format: FormatOpt = None,
    from_: FromOpt = None,
    jobs: Jobs = 0,
    include_path: IncludeOpt = None,
) -> None:
    """Convert between LilyPond, MusicXML, MXL, MIDI, ABC and Humdrum **kern.

    Given a directory, converts every score in it (recursively) into the output
    directory, in parallel; the exit status is non-zero if any file fails.
    A LilyPond file with several \\score blocks, or an ABC file with several
    tunes, writes the first to the output path and the others next to it, as
    NAME_02.EXT, NAME_03.EXT, … What reading LilyPond finds goes to stderr.
    """
    if input != "-" and Path(input).is_dir():
        _run_batch(Path(input), Path(output), _value(format), jobs, include_path)
        return
    _transform(input, output, format, from_, lambda m: m, include_path)


def _resolve_jobs(jobs: int) -> int:
    """Worker count (``0`` = one per CPU)."""
    return jobs if jobs > 0 else (os.cpu_count() or 1)


def _run_isolated(fn: Callable[[], None]) -> str | None:
    """Run a task; its error message, or None. A failing (or panicking) task
    fails alone instead of stopping the batch."""
    try:
        fn()
    except (KeyboardInterrupt, SystemExit):
        raise
    except BaseException as exc:  # noqa: BLE001 - includes Rust panics
        return str(exc) or exc.__class__.__name__
    return None


def _convert_file(task: tuple[str, str, str | None, list[str] | None]) -> tuple[str, str | None]:
    """Worker: convert one (input, output, format, include paths) task."""
    src, dst, fmt, include = task
    return src, _run_isolated(lambda: write_score(read_score(src, include=include), dst, fmt))


def _map_parallel(fn: Callable[[Any], Any], tasks: list[Any], jobs: int) -> list[Any]:
    workers = min(_resolve_jobs(jobs), len(tasks))
    if workers <= 1:
        return [fn(t) for t in tasks]
    # The work is CPU-bound and runs in the extension: processes, not threads.
    with ProcessPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(fn, tasks))


def _run_batch(
    input_dir: Path, output_dir: Path, fmt: str | None, jobs: int = 0, include: list[str] | None = None
) -> None:
    files = sorted(p for p in input_dir.rglob("*") if p.is_file() and p.suffix.lower() in _EXT_FORMAT)
    if not files:
        _note(f"No supported files found in {input_dir}")
        return
    tasks: list[tuple[str, str, str | None, list[str] | None]] = []
    writes: dict[Path, Path] = {}
    clashes: list[str] = []
    for file in files:
        # Without --format, LilyPond becomes MusicXML and everything else LilyPond.
        ext = _FORMAT_EXT[fmt] if fmt else (".xml" if _EXT_FORMAT[file.suffix.lower()] == "ly" else ".ly")
        out = (output_dir / file.relative_to(input_dir)).with_suffix(ext)
        if out in writes:
            clashes.append(f"{writes[out]} and {file} would both write {out}")
        writes[out] = file
        tasks.append((str(file), str(out), fmt, include))
    if clashes:
        raise CliError("; ".join(clashes))
    for _, out, _, _ in tasks:
        Path(out).parent.mkdir(parents=True, exist_ok=True)  # before the workers start

    failures = [(src, err) for src, err in _map_parallel(_convert_file, tasks, jobs) if err]
    for src, err in failures:
        _note(f"{src}: {err}")
    _note(f"Processed {len(files)} files")
    if failures:
        raise CliError(f"{len(failures)} of {len(files)} file(s) failed to convert")


# ---------------------------------------------------------------------------
# Transforms
# ---------------------------------------------------------------------------


@_command()
def transpose(
    input: Input,
    output: Output,
    semitones: Annotated[
        Optional[int], typer.Option("--semitones", "-s", help="Chromatic: N semitones (negative = down).")
    ] = None,
    interval: Annotated[
        Optional[str], typer.Option("--interval", "-i", help="Diatonic, by a named interval: M3, m3, P5, A4, -m2, P8.")
    ] = None,
    to_key: Annotated[
        Optional[str], typer.Option("--to-key", help="So the tonic becomes this key (nearest way): D, Bb, F#.")
    ] = None,
    format: FormatOpt = None,
    from_: FromOpt = None,
    include_path: IncludeOpt = None,
) -> None:
    """Transpose. Give exactly one of --semitones, --interval, --to-key."""
    modes = [m is not None for m in (semitones, interval, to_key)]
    if sum(modes) != 1:
        raise CliError("specify exactly one of --semitones, --interval, --to-key")
    if semitones is not None:
        apply: Callable[[Any], Any] = lambda m: lytk.transpose(m, semitones)
    elif interval is not None:
        apply = lambda m: lytk.transpose_interval(m, interval)
    else:
        apply = lambda m: lytk.transpose_to_key(m, to_key)
    _transform(input, output, format, from_, apply, include_path)


_AXIS = re.compile(r"^\s*([a-gA-G])([sf#]*)\s*(-?\d+)\s*$")


def parse_axis(text: str) -> tuple[str, int, int]:
    """``c4``, ``fs3``, ``bf5``, ``c#4`` → (step, alter, octave). ``s``/``#``
    raise, ``f`` lowers, and may repeat."""
    m = _AXIS.match(text)
    if not m:
        raise CliError(f"invalid axis pitch {text!r}: expected a note letter, accidentals and an octave (e.g. c4, fs3)")
    step, accidentals, octave = m.groups()
    alter = sum(1 if c in "s#" else -1 for c in accidentals.lower())
    return step.upper(), alter, int(octave)


@_command()
def invert(
    input: Input,
    output: Output,
    axis: Annotated[str, typer.Option("--axis", "-a", help="Axis pitch: c4 (middle C), fs3, bf5.")] = "c4",
    format: FormatOpt = None,
    from_: FromOpt = None,
    include_path: IncludeOpt = None,
) -> None:
    """Invert: mirror every pitch around an axis pitch."""
    step, alter, octave = parse_axis(axis)
    apply = lambda m: lytk.invert(m, step=step, alter=alter, octave=octave)  # noqa: E731
    _transform(input, output, format, from_, apply, include_path)


@_command()
def retrograde(
    input: Input, output: Output, format: FormatOpt = None, from_: FromOpt = None, include_path: IncludeOpt = None
) -> None:
    """Retrograde: play the music backwards."""
    _transform(input, output, format, from_, lytk.retrograde, include_path)


@_command(name="change-language")
def change_language(
    input: Input,
    output: Output,
    language: Annotated[
        str, typer.Option("--language", "-l", help="nederlands, english, deutsch, italiano, français, …")
    ],
    format: FormatOpt = None,
    from_: FromOpt = None,
    include_path: IncludeOpt = None,
) -> None:
    """Change the LilyPond note-name language (it shows in LilyPond output)."""
    _transform(input, output, format, from_, lambda m: lytk.change_language(m, language), include_path)


def _repitch(input: str, output: str, relative: bool, include: list[str] | None) -> None:
    if input != "-" and _EXT_FORMAT.get(Path(input).suffix.lower()) != "ly":
        raise CliError("abs2rel/rel2abs need a LilyPond (.ly) input")
    items = [lytk.to_lilypond(s, relative=relative).encode("utf-8") for s in _read_movements(input, "ly", include)]
    _write_movements(items, output, ".ly")


@_command()
def abs2rel(input: Input, output: Output, include_path: IncludeOpt = None) -> None:
    """Rewrite a LilyPond file with \\relative octave marks.

    Multi-staff and multi-voice parts keep absolute octaves where relative ones
    would be ambiguous.
    """
    _repitch(input, output, relative=True, include=include_path)


@_command()
def rel2abs(input: Input, output: Output, include_path: IncludeOpt = None) -> None:
    """Rewrite a LilyPond file with absolute octave marks (no \\relative)."""
    _repitch(input, output, relative=False, include=include_path)


# ---------------------------------------------------------------------------
# Inspection
# ---------------------------------------------------------------------------


def _pitches(score: lytk.Score) -> list[int]:
    """Every sounding pitch (grace notes and chord members included), sorted."""
    return sorted(n.midi for part in score.iter_parts() for n in part.notes)


def _sounding(score: lytk.Score) -> list[tuple[int, int, int]]:
    """Every note as played: (onset, duration, sounding pitch), sorted."""
    rows = lytk.to_note_array(score.to_music_document())[:, :3].tolist()
    return sorted(tuple(r) for r in rows)


def _json(data: Any) -> None:
    typer.echo(json.dumps(data, indent=2, ensure_ascii=False))


@_command()
def info(
    input: Annotated[str, typer.Argument(help="Input file.", show_default=False)],
    as_json: Annotated[bool, typer.Option("--json", help="Machine-readable JSON.")] = False,
    include_path: IncludeOpt = None,
) -> None:
    """Show a score's metadata, counts and parts (``lytk.info``)."""
    data = lytk.info(read_score(input, include=include_path))
    if as_json:
        _json(data)
        return
    for label, key in [
        ("Title", "title"),
        ("Composer", "composer"),
        ("Subtitle", "subtitle"),
        ("Arranger", "arranger"),
        ("Language", "language"),
    ]:
        if data[key]:
            typer.echo(f"{label + ':':<10}{data[key]}")
    typer.echo(f"{'Parts:':<10}{data['part_count']}")
    for p in data["parts"]:
        typer.echo(f"  - {p['name'] or p['id']} ({p['measures']} measures)")


def _is_grace(e: Any) -> bool:
    """A grace note, or a chord of grace notes: no time."""
    notes = getattr(e, "notes", None)
    if notes is not None:
        return bool(notes) and all(n.is_grace for n in notes)
    return bool(getattr(e, "is_grace", False))


def _length(elements: list[Any]) -> Fraction:
    return sum((Fraction(*e.duration_fraction) for e in elements if not _is_grace(e)), Fraction(0))


@_command()
def positions(input: Input, from_: FromOpt = None, include_path: IncludeOpt = None) -> None:
    """Each part's bars as JSON: number, start and length in quarter notes.

    These are musical positions from the notated durations, not graphical ones.
    A bar lasts as long as its longest voice, so pickups and short bars count
    as written.
    """
    score = read_score(input, from_, include_path)
    parts = []
    for part in score.iter_parts():
        start = Fraction(0)
        bars = []
        for m in part.measures:
            length = max((_length(v.elements) for v in m.voices), default=Fraction(0))
            bars.append({"number": m.number, "start": float(start * 4), "duration": float(length * 4)})
            start += length
        parts.append({"id": part.part_id, "measures": bars})
    _json({"unit": "quarter", "parts": parts})


def _dict_parts(children: list[dict[str, Any]]) -> list[dict[str, Any]]:
    parts: list[dict[str, Any]] = []
    for child in children:
        if "Part" in child:
            parts.append(child["Part"])
        elif "PartGroup" in child:
            parts.extend(_dict_parts(child["PartGroup"].get("children", [])))
    return parts


def _part_label(part: dict[str, Any], index: int) -> str:
    """A filename-safe label: the part's name or id, else ``partNN``."""
    label = re.sub(r"[^A-Za-z0-9]", "_", part.get("name") or part.get("part_id") or "").strip("_")
    return label or f"part{index:02d}"


@_command()
def bundle(
    input: Input,
    output: Annotated[str, typer.Option("--output", "-o", help="Output directory (created if missing).")],
    format: Annotated[Format, typer.Option("--format", "-f", help="Format of each part file.")] = Format.xml,
    from_: FromOpt = None,
    include_path: IncludeOpt = None,
) -> None:
    """Write each part to its own file: DIR/<input-stem>_<part>.<ext>."""
    score = read_score(input, from_, include_path)
    whole = score.to_dict()
    parts = _dict_parts(whole["children"])
    if not parts:
        raise CliError("no parts to export")
    out_dir = Path(output)
    out_dir.mkdir(parents=True, exist_ok=True)
    stem = "score" if input == "-" else Path(input).stem
    for i, part in enumerate(parts, start=1):
        single = lytk.Score.from_dict({**whole, "children": [{"Part": part}]})
        path = out_dir / f"{stem}_{_part_label(part, i)}{_FORMAT_EXT[format.value]}"
        write_score(single, str(path), format)
        _note(f"Wrote part {i} → {path}")


@_command()
def diff(
    a: Annotated[str, typer.Argument(help="First score (`-` for stdin).", show_default=False)],
    b: Annotated[str, typer.Argument(help="Second score.", show_default=False)],
    as_json: Annotated[bool, typer.Option("--json", help="Machine-readable JSON.")] = False,
    from_: Annotated[
        Optional[Format], typer.Option("--from", help="Input format of both (required for stdin).")
    ] = None,
    include_path: IncludeOpt = None,
) -> None:
    """Compare two scores by what they sound: parts, and every note's onset,
    duration and sounding pitch (dynamics, notation and layout aside).

    Exits with status 1 when they differ, so it can gate a pipeline.
    """
    sa, sb = read_score(a, from_, include_path), read_score(b, from_, include_path)
    pa, pb = _pitches(sa), _pitches(sb)
    na, nb = _sounding(sa), _sounding(sb)
    parts_a, parts_b = sa.num_parts, sb.num_parts
    same_pitches = pa == pb
    same_notes = na == nb
    equal = same_notes and parts_a == parts_b
    if as_json:
        _json(
            {
                "equal": equal,
                "parts": {"a": parts_a, "b": parts_b},
                "note_count": {"a": len(na), "b": len(nb)},
                "pitch_multiset_equal": same_pitches,
                "notes_equal": same_notes,
            }
        )
    else:
        typer.echo(f"parts:          {parts_a} vs {parts_b}")
        typer.echo(f"notes:          {len(na)} vs {len(nb)}")
        typer.echo(f"pitches equal:  {str(same_pitches).lower()}")
        typer.echo(f"timing equal:   {str(same_notes).lower()}")
        typer.echo("scores are semantically equal" if equal else "scores differ")
    if not equal:
        raise typer.Exit(1)


# ---------------------------------------------------------------------------
# Batch jobs
# ---------------------------------------------------------------------------


def _job_format(value: Any) -> str | None:
    if value is None:
        return None
    try:
        return _FORMAT_NAMES[str(value).lower()]
    except KeyError:
        raise CliError(f"unknown format {value!r}") from None


def _do_job(job: dict[str, Any]) -> None:
    score = read_score(job["input"], _job_format(job.get("from")))
    if job.get("interval") is not None:
        score = lytk.transpose_interval(score, str(job["interval"]))
    elif job.get("transpose") is not None:
        score = lytk.transpose(score, int(job["transpose"]))
    out = Path(job["output"])
    out.parent.mkdir(parents=True, exist_ok=True)
    write_score(score, str(out), _job_format(job.get("format")))


def _run_job(job: dict[str, Any]) -> dict[str, Any]:
    """Worker: run one job, report its outcome."""
    error = _run_isolated(lambda: _do_job(job))
    result = {"input": job["input"], "output": job["output"], "ok": error is None}
    if error is not None:
        result["error"] = error
    return result


def _load_jobs(file: Path) -> list[dict[str, Any]]:
    try:
        spec = json.loads(file.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise CliError(f"invalid batch job file: {exc}") from None
    if not isinstance(spec, list):
        raise CliError("invalid batch job file: expected a JSON array of jobs")
    jobs = []
    for i, job in enumerate(spec, start=1):
        if not isinstance(job, dict):
            raise CliError(f"invalid batch job file: job {i} is not an object")
        src, dst = job.get("in", job.get("input")), job.get("out", job.get("output"))
        if not isinstance(src, str) or not isinstance(dst, str):
            raise CliError(f"invalid batch job file: job {i} needs `in` and `out` paths")
        jobs.append({**job, "input": src, "output": dst})
    return jobs


@_command()
def batch(
    file: Annotated[Path, typer.Argument(help="JSON job file.", show_default=False)],
    jobs: Jobs = 0,
    report: Annotated[
        Optional[str], typer.Option("--report", help="Write a JSON report of every job here (`-` for stdout).")
    ] = None,
) -> None:
    """Run a JSON list of jobs: {"in", "out", "format"?, "from"?, "transpose"?, "interval"?}.

    Jobs run in parallel and independently: one failing job doesn't stop the
    others, but makes the exit status non-zero.
    """
    spec = _load_jobs(file)
    if not spec:
        _note(f"no jobs in {file}")
        return
    results = _map_parallel(_run_job, spec, jobs)
    for r in results:
        if not r["ok"]:
            _note(f"{r['input']}: {r['error']}")
    if report is not None:
        write_bytes(report, (json.dumps(results, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))
    _note(f"Processed {len(results)} jobs")
    failed = sum(not r["ok"] for r in results)
    if failed:
        raise CliError(f"{failed} of {len(results)} job(s) failed")


# ---------------------------------------------------------------------------
# check
# ---------------------------------------------------------------------------


@_command()
def check(
    files: Annotated[list[str], typer.Argument(help="LilyPond files (`-` for stdin).", show_default=False)],
    semantic: Annotated[
        bool,
        typer.Option("--semantic", help="Read the files too, and report what the reader drops or cannot represent."),
    ] = False,
    as_json: Annotated[bool, typer.Option("--json", help="Write the diagnostics as a JSON list.")] = False,
) -> None:
    """Report the syntax errors of LilyPond files (and, with --semantic, what a
    reading reports), one per line as FILE:LINE:COLUMN: SEVERITY: MESSAGE [CODE].

    Exits with 1 when a file has an error; warnings alone exit with 0.
    """
    found = []
    for name in files:
        text = sys.stdin.buffer.read().decode("utf-8") if name == "-" else Path(name).read_text(encoding="utf-8")
        found += [(name, d) for d in lytk.check_lilypond(text, semantic=semantic)]
    if as_json:
        keys = ("severity", "code", "message", "line", "column", "start", "end")
        rows = [{"file": name, **{k: getattr(d, k) for k in keys}} for name, d in found]
        write_bytes("-", (json.dumps(rows, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))
    else:
        for name, d in found:
            print(f"{name}:{d}")
    if any(d.severity == "error" for _, d in found):
        raise typer.Exit(1)


# ---------------------------------------------------------------------------
# flatten
# ---------------------------------------------------------------------------


@_command()
def flatten(
    input: Annotated[str, typer.Argument(help="LilyPond file.", show_default=False)],
    output: Annotated[Optional[str], typer.Option("--output", "-o", help="Output file (default: stdout).")] = None,
    include_path: Annotated[
        Optional[list[str]],
        typer.Option("--include-path", "-I", help="Extra directory to search for includes (repeatable)."),
    ] = None,
    no_markers: Annotated[bool, typer.Option("--no-markers", help="Leave out the BEGIN/END INCLUDE comments.")] = False,
) -> None:
    """Inline every \\include, recursively, into one self-contained file.

    Repeated \\version and \\language lines are merged (the last one wins, with a
    warning); more than one \\header block is an error.
    """
    text = lytk.flatten(input, output, include_paths=include_path or [], add_markers=not no_markers)
    if output is None:
        write_bytes("-", text.encode("utf-8"))


def main() -> None:
    """Console-script entry point."""
    app()


if __name__ == "__main__":
    main()
