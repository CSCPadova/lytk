"""Write docs/python-api.md, the Python API reference, from the stubs.

Run `python scripts/python_api.py` after changing the stubs
(`src/lytk/_core.pyi`), `lytk.__all__` or the datasets module;
`tests/test_bindings.py` fails when the reference is out of date. Sections
follow the comments in `lytk.__all__`; signatures and docstrings come from the
stubs and from `src/lytk/datasets/base.py`, read with `ast`.
"""

from __future__ import annotations

import ast
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "python-api.md"

INTRO = """\
# Python API reference

*Generated from `src/lytk/_core.pyi` and `src/lytk/datasets/` by
`python scripts/python_api.py`; a test fails when this file is out of date.*

`import lytk` gives everything below; `lytk.__version__` is the version.

- **Errors.** Readers (`from_*`, `Score.from_json`/`from_dict`,
  `MusicDocument.from_json`, `flatten`) raise `lytk.ParseError`, a
  `ValueError`, when the input cannot be read, and `OSError` when a file
  cannot be opened. `strict=True` LilyPond readers raise
  `lytk.LilyPondSyntaxError`, a `ParseError`, when the reading has an error.
  Writers (`to_*`) raise `ValueError` when a score cannot be written. Any
  function may raise `lytk.InternalError`, a bug in lytk.
- **Positions** (`Diagnostic`, `HeaderField`, `Token`) are character
  offsets: `text[d.start:d.end]` is the span.
- **One LilyPond parse.** `from_lilypond_string` and
  `from_lilypond_music_string` (and their file twins) run the same parse,
  walk and assembly: the Music tree of `MusicDocument` is lifted from the
  `Score`. Whatever one accepts, the other accepts too.
"""


def all_sections(init: str) -> list[tuple[str, list[str]]]:
    """(title, names) in the order of `lytk.__all__`, one per comment."""
    block = re.search(r"^__all__ = \[\n(.*?)^\]", init, re.S | re.M)
    if block is None:
        raise SystemExit("no __all__ list in lytk/__init__.py")
    sections: list[tuple[str, list[str]]] = []
    for line in block.group(1).splitlines():
        line = line.strip()
        if line.startswith("#"):
            sections.append((line.lstrip("# "), []))
        elif m := re.fullmatch(r'"(\w+)",', line):
            if not sections:
                raise SystemExit(f"{m.group(1)} in __all__ before any section comment")
            sections[-1][1].append(m.group(1))
    extra = re.search(r"^__all__ \+= \[(.*?)\]", init, re.M)
    if extra:
        sections.append(("MIDI", re.findall(r'"(\w+)"', extra.group(1))))
    return sections


def definitions(source: str) -> dict[str, ast.stmt]:
    return {
        node.name: node
        for node in ast.parse(source).body
        if isinstance(node, (ast.FunctionDef, ast.ClassDef))
    }


def signature(fn: ast.FunctionDef, name: str) -> str:
    args = fn.args
    if args.args and args.args[0].arg in ("self", "cls"):
        args = ast.arguments(
            posonlyargs=args.posonlyargs,
            args=args.args[1:],
            vararg=args.vararg,
            kwonlyargs=args.kwonlyargs,
            kw_defaults=args.kw_defaults,
            kwarg=args.kwarg,
            defaults=args.defaults,
        )
    returns = f" -> {ast.unparse(fn.returns)}" if fn.returns else ""
    return f"{name}({ast.unparse(args)}){returns}"


def doc(node: ast.AST) -> str:
    text = ast.get_docstring(node) or ""
    return f"{text}\n\n" if text else ""


def decorators(fn: ast.FunctionDef) -> set[str]:
    return {ast.unparse(d) for d in fn.decorator_list}


def render_class(cls: ast.ClassDef, module: dict[str, ast.stmt]) -> str:
    # A private base of the same module is shown as the public class it
    # derives from, and its public members as the class's own.
    members: dict[str, ast.FunctionDef] = {}
    shown_bases = []
    for base in cls.bases:
        name = ast.unparse(base)
        private = module.get(name) if name.startswith("_") else None
        if isinstance(private, ast.ClassDef):
            shown_bases.extend(ast.unparse(b) for b in private.bases)
            members.update({m.name: m for m in private.body if isinstance(m, ast.FunctionDef)})
        else:
            shown_bases.append(name)
    members.update({m.name: m for m in cls.body if isinstance(m, ast.FunctionDef)})
    bases = ", ".join(shown_bases)
    out = f"### class `{cls.name}{f'({bases})' if bases else ''}`\n\n{doc(cls)}"
    # The constructor first, then the members in the order they are defined.
    for member in sorted(members.values(), key=lambda m: m.name != "__init__"):
        name = member.name
        if name == "__init__":
            member.returns = None
            out += f"#### `{signature(member, cls.name)}`\n\n{doc(member)}"
        elif name.startswith("_"):
            continue
        elif "property" in decorators(member):
            kind = f": {ast.unparse(member.returns)}" if member.returns else ""
            out += f"#### `{cls.name}.{name}{kind}`\n\n{doc(member)}"
        elif not any(d.endswith(".setter") for d in decorators(member)):
            prefix = "static " if "staticmethod" in decorators(member) else ""
            prefix = "class method " if "classmethod" in decorators(member) else prefix
            out += f"#### {prefix}`{signature(member, f'{cls.name}.{name}')}`\n\n{doc(member)}"
    return out


def render_entry(node: ast.stmt, module: dict[str, ast.stmt]) -> str:
    if isinstance(node, ast.ClassDef):
        return render_class(node, module)
    assert isinstance(node, ast.FunctionDef)
    return f"### `{signature(node, node.name)}`\n\n{doc(node)}"


def render() -> str:
    stubs = definitions((ROOT / "src/lytk/_core.pyi").read_text())
    sections = all_sections((ROOT / "src/lytk/__init__.py").read_text())
    out = INTRO
    missing = []
    for title, names in sections:
        out += f"\n## {title}\n\n"
        for name in names:
            if name not in stubs:
                missing.append(name)
                continue
            out += render_entry(stubs[name], stubs)
    if missing:
        raise SystemExit(f"no stub for {', '.join(missing)} in src/lytk/_core.pyi")
    # lytk.datasets: its own module, documented from the source.
    datasets = definitions((ROOT / "src/lytk/datasets/base.py").read_text())
    exported = re.findall(
        r'"(\w+)"', re.search(r"__all__ = \[(.*?)\]", (ROOT / "src/lytk/datasets/__init__.py").read_text(), re.S).group(1)
    )
    out += "\n## Datasets (`lytk.datasets`)\n\n"
    for name in exported:
        if name in datasets:
            out += render_entry(datasets[name], datasets)
    constants = [n for n in exported if n not in datasets]
    if constants:
        out += "### Constants\n\n"
        sys.path.insert(0, str(ROOT / "src"))
        import lytk.datasets as lds

        for name in constants:
            value = getattr(lds, name)
            shown = sorted(value) if isinstance(value, (set, frozenset)) else value
            out += f"- `{name}`: `{shown!r}`\n"
        out += "\n"
    return out.rstrip() + "\n"


if __name__ == "__main__":
    OUT.write_text(render())
    print(f"wrote {OUT.relative_to(ROOT)}")
