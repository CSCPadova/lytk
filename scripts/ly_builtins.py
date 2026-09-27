"""Write src/adapters/ly_to_ir/builtins.rs, every command LilyPond defines,
and drums.rs, its drum-mode note names.

The reader warns about a command it neither handles nor finds defined in the
file (`unknown-command`); the commands LilyPond itself defines are not
unknown. Run from the repository root, with a LilyPond git checkout:

    python3 scripts/ly_builtins.py lilypond v2.26.0
"""

import re
import subprocess
import sys


def show(repo: str, tag: str, path: str) -> str:
    return subprocess.run(
        ["git", "-C", repo, "show", f"{tag}:{path}"],
        check=True, capture_output=True, text=True, errors="replace",
    ).stdout


def files(repo: str, tag: str, folder: str, suffix: str) -> list[str]:
    out = subprocess.run(
        ["git", "-C", repo, "ls-tree", "--name-only", f"{tag}:{folder}"],
        check=True, capture_output=True, text=True,
    ).stdout
    return [f"{folder}/{name}" for name in out.split() if name.endswith(suffix)]


def write_drums(repo: str, tag: str) -> None:
    """src/adapters/ly_to_ir/drums.rs: drum-mode names and the MIDI key each
    sounds, from `ly/drumpitch-init.ly` (`drumPitchNames`, `midiDrumPitches`)."""
    text = show(repo, tag, "ly/drumpitch-init.ly")
    names_part, pitches_part = text.split("midiDrumPitches")[0], text.split("midiDrumPitches")[1]
    names = dict(re.findall(r"\((\w+) \. (\w+)\)", names_part))
    alters = {"NATURAL": 0, "SHARP": 1, "FLAT": -1, "DOUBLE-SHARP": 2, "DOUBLE-FLAT": -2}
    pitches = {}
    for sym, octave, step, alter in re.findall(
        r"\((\w+) \. ,\(ly:make-pitch (-?\d+) (\d+) ([A-Z-]+)\)\)", pitches_part
    ):
        pitches[sym] = (int(octave), int(step), alters[alter])
    rows = []
    for name in sorted(names):
        if names[name] not in pitches:
            continue  # no MIDI sound (`tamtam`)
        o, st, al = pitches[names[name]]
        # LilyPond's octave 0 holds middle C, lytk's octave 4.
        rows.append(f'    ("{name}", {o + 4}, {st}, {al}),')
    with open("src/adapters/ly_to_ir/drums.rs", "w") as out:
        out.write(
            f"//! Drum-mode note names of LilyPond {tag.lstrip('v')} and the pitch whose MIDI\n"
            "//! key sounds each (General MIDI percussion): `ly/drumpitch-init.ly`. Generated:\n"
            f"//! `python3 scripts/ly_builtins.py lilypond {tag}`.\n\n"
            "/// (name, octave, step 0-6 from C, alteration in semitones), sorted by name.\n"
            f"pub(super) const DRUMS: &[(&str, i32, u8, i32)] = &[\n" + "\n".join(rows) + "\n];\n"
        )
    print(len(rows), "drum names")


def main(repo: str, tag: str) -> None:
    names: set[str] = set()
    # Identifiers assigned at the top level of the init files: music
    # functions, articulations, dynamics, property shorthands, spanners;
    # `"\\<" = …` defines `\<` (and `"f" = …` the dynamic). The paper defaults (indented, inside
    # `\paper`) hold the units, `\mm` and `\cm`; `\name Staff` defines the
    # context `\Staff` of `\layout` blocks.
    for path in files(repo, tag, "ly", ".ly"):
        indent = r"\s*" if path.endswith("paper-defaults-init.ly") else ""
        for line in show(repo, tag, path).splitlines():
            if m := re.match(indent + r"([A-Za-z](?:[-_]?[A-Za-z])*)\s*=", line):
                names.add(m[1])
            elif m := re.match(r'"((?:[^"\\\\]|\\\\.)+)"\s*=', line):
                name = m[1].replace("\\\\", "\\")
                names.add(name.removeprefix("\\"))
            elif m := re.match(r'\s*\\name "?([A-Za-z]\w*)', line):
                names.add(m[1])
    # Markup commands (`\bold`, `\column`, …) and accordion registers.
    for path in files(repo, tag, "scm", ".scm"):
        text = show(repo, tag, path)
        names.update(re.findall(r"\(define-markup(?:-list)?-command\s*\(([-\w]+)", text))
        names.update(re.findall(r"\(define-register-set\s+([-\w]+)", text))
    # The lexer's keywords and commands.
    names.update(re.findall(r'\{"(\w+)", [A-Z_]+\}', show(repo, tag, "lily/lily-lexer.cc")))
    names.update(re.findall(r'\\\\(include|version|maininput|sourcefileline|sourcefilename)',
                            show(repo, tag, "lily/lexer.ll")))
    write_drums(repo, tag)
    names = sorted(names)
    rows = "\n".join(f'    "{n.replace(chr(92), chr(92) * 2)}",' for n in names)
    with open("src/adapters/ly_to_ir/builtins.rs", "w") as out:
        out.write(
            f"//! The commands LilyPond {tag.lstrip('v')} defines, without their backslash:\n"
            "//! lexer keywords, the identifiers of its `ly/` init files and its markup\n"
            f"//! commands ({len(names)}). Generated: `python3 scripts/ly_builtins.py lilypond {tag}`.\n\n"
            "/// Sorted, for binary search.\n"
            f"pub(super) const BUILTINS: &[&str] = &[\n{rows}\n];\n"
        )
    print(len(names), "commands")


if __name__ == "__main__":
    main(*sys.argv[1:3])
