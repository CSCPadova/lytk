# CLI Reference

lytk provides a command-line tool (`lytk`) and an equivalent Python API. Both are
backed by the same Rust core — the CLI is the binary entry point, and the Python
package exposes the same operations as callable functions.

---

## Installation

### Rust CLI

```bash
git clone <repo>
cd lytk
cargo build --release
# Binary at target/release/lytk
# Optionally add to PATH:
export PATH="$PATH:$(pwd)/target/release"
```

### Python package

```bash
# Development install (requires Rust toolchain)
uv sync && maturin develop
```

---

## Subcommands

### `lytk convert`

Convert a file or directory between LilyPond, MusicXML, and MXL formats.

```
lytk convert <INPUT> -o <OUTPUT> [OPTIONS]
```

**Arguments:**

| Argument | Description |
|---|---|
| `<INPUT>` | Input file (`.ly`, `.ily`, `.xml`, `.musicxml`, `.mxl`) or directory |
| `-o`, `--output <OUTPUT>` | Output file or directory |
| `-f`, `--format <FORMAT>` | Force output format: `ly`, `xml` |
| `-j`, `--jobs <N>` | Parallel threads for batch mode (0 = auto, default) |

**Examples:**

```bash
# Single file conversions
lytk convert input.xml -o output.ly          # MusicXML → LilyPond
lytk convert input.ly  -o output.xml         # LilyPond → MusicXML
lytk convert input.mxl -o output.ly          # Compressed MXL → LilyPond

# Force output format when extension is ambiguous
lytk convert input.xml -o output.txt --format ly

# Batch convert a directory (auto-detect format per file)
lytk convert input_dir/ -o output_dir/

# Batch with explicit thread count
lytk convert input_dir/ -o output_dir/ -j 8
```

**Multi-movement output:**

When a LilyPond file contains multiple `\score` blocks, the convert command writes
separate files with `_01`, `_02`, … suffixes:

```bash
lytk convert multi_movement.ly -o out.xml
# Writes: out_01.xml, out_02.xml, …
```

**Notes:**
- Format is auto-detected from the file extension when `--format` is omitted.
- In batch mode, files are processed in parallel using rayon. Errors on individual
  files are reported to stderr without stopping the batch.
- Supported input extensions: `.ly`, `.ily`, `.xml`, `.musicxml`, `.mxl`

---

### `lytk flatten`

Recursively expand all `\include` directives in a LilyPond file, producing a single
self-contained flat file.

```
lytk flatten <INPUT> [OPTIONS]
```

**Arguments:**

| Argument | Description |
|---|---|
| `<INPUT>` | Input LilyPond file |
| `-o`, `--output <OUTPUT>` | Output file. Writes to stdout if omitted |
| `-I`, `--include-path <DIR>` | Extra include search directory. May be repeated |
| `--no-markers` | Suppress `% === BEGIN/END INCLUDE ===` comment markers |

**Examples:**

```bash
# Flatten to stdout
lytk flatten score.ly

# Flatten to a file
lytk flatten score.ly -o flat.ly

# With extra search paths (like LilyPond's -I flag)
lytk flatten score.ly -I ./lib -I /usr/share/lilypond/ly -o flat.ly

# Clean output without markers
lytk flatten score.ly --no-markers -o flat.ly
```

**Include resolution:**

1. Path is looked up relative to the **including file's** directory (not the root file).
2. If not found, tries appending `.ly`, then `.ily`.
3. Then searches each `-I` path in order, with the same extension fallbacks.
4. If still not found, exits with an error.

**Normalization (post-processing):**

After full expansion, the output is scanned for duplicate command lines:

| Command | Behaviour |
|---|---|
| `\version "X.Y.Z"` | Last occurrence kept; earlier ones removed; warning on stderr |
| `\language "X"` | Last occurrence kept; earlier ones removed; warning on stderr |
| `\header { }` | Multiple blocks → error (exit 1) |

**Comment markers:**

By default, each inlined file is wrapped with:

```lilypond
% === BEGIN INCLUDE: path/to/file.ly ===
…content…
% === END INCLUDE: path/to/file.ly ===
```

Pass `--no-markers` to suppress these and get clean output.

**Error conditions:**

- **File not found** — exits with error showing the unresolved path
- **Circular include** — exits with error showing the full ancestor chain
  (e.g. `a.ly -> b.ly -> a.ly`)
- **Multiple `\header` blocks** — exits with error

---

### `lytk transpose`

Transpose all pitches by a number of semitones.

```
lytk transpose <INPUT> -o <OUTPUT> --semitones <N> [OPTIONS]
```

**Arguments:**

| Argument | Description |
|---|---|
| `<INPUT>` | Input file |
| `-o`, `--output <OUTPUT>` | Output file |
| `-s`, `--semitones <N>` | Semitones to transpose (positive = up, negative = down) |
| `-f`, `--format <FORMAT>` | Force output format |

**Examples:**

```bash
lytk transpose input.ly --semitones 3 -o transposed.ly    # up a minor third
lytk transpose input.xml --semitones -5 -o transposed.xml  # down a perfect fourth
lytk transpose input.ly --semitones 12 -o up_octave.ly     # up an octave
```

**Notes:**
- Key signatures are updated to match the transposed tonic.
- All formats supported by `convert` are accepted as input/output.

---

### `lytk info`

Print score metadata — title, composer, parts, and measure counts.

```
lytk info <INPUT>
```

**Examples:**

```bash
lytk info input.xml
lytk info score.ly
```

**Sample output:**

```
Title:    Duetto for Cello and Bass
Composer: G. Rossini
Language: nederlands
Parts:    2
  - Cello (42 measures)
  - Contrabasso (42 measures)
```

---

## Python API

The Python package (`import lytk`) mirrors every CLI subcommand as a Python function.
All functions are backed by the same Rust core — there is no performance difference
between using the CLI and the Python API.

### Installation

```bash
# Development
uv sync && maturin develop
```

### Format conversion

```python
import lytk

# MusicXML → Score → LilyPond
score = lytk.from_musicxml("input.xml")
lytk.to_lilypond(score, "output.ly")

# LilyPond → Score → MusicXML
score = lytk.from_lilypond("input.ly")
lytk.to_musicxml(score, "output.xml")

# Parse from string
score = lytk.from_musicxml_string(xml_text)
score = lytk.from_lilypond_string(ly_text)

# Emit to string (without writing a file)
ly_text  = lytk.to_lilypond(score)
xml_text = lytk.to_musicxml(score)

# Override pitch language on emit
ly_text = lytk.to_lilypond(score, language="deutsch")
```

### MIDI

```python
score = lytk.from_midi("input.mid")
lytk.to_midi(score, "output.mid")
```

### Transforms

All transforms are pure functions — they return a new `Score` and do not modify
the original.

```python
# Transpose
score_up = lytk.transpose(score, semitones=3)
score_down = lytk.transpose(score, semitones=-5)

# Change pitch language
score_de = lytk.change_language(score, "deutsch")
score_en = lytk.change_language(score, "english")

# Invert around an axis pitch
score_inv = lytk.invert(score, step="C", alter=0, octave=4)

# Retrograde (reverse note order)
score_retro = lytk.retrograde(score)
```

### Score introspection

```python
score = lytk.from_musicxml("input.xml")

print(score.title)       # "Symphony No. 5"
print(score.composer)    # "Beethoven"
print(score.num_parts)   # 4
print(score.parts)       # ["Violin I", "Violin II", "Viola", "Cello"]
print(score.language)    # "nederlands" or None

# Serialise / deserialise
json_str = score.to_json()
score2   = lytk.Score.from_json(json_str)

d      = score.to_dict()   # Python dict via serde_json
score3 = lytk.Score.from_dict(d)
```

### Batch processing example

```python
import lytk
from pathlib import Path

input_dir  = Path("dataset/xml")
output_dir = Path("dataset/ly")
output_dir.mkdir(exist_ok=True)

for xml_file in input_dir.glob("*.xml"):
    try:
        score = lytk.from_musicxml(str(xml_file))
        score = lytk.transpose(score, semitones=2)
        out   = output_dir / xml_file.with_suffix(".ly").name
        lytk.to_lilypond(score, str(out))
    except Exception as e:
        print(f"Error processing {xml_file}: {e}")
```

---

## Comparison: CLI vs Python API

| Operation | CLI | Python |
|---|---|---|
| Convert file | `lytk convert in.xml -o out.ly` | `lytk.to_lilypond(lytk.from_musicxml("in.xml"), "out.ly")` |
| Transpose | `lytk transpose in.ly -s 3 -o out.ly` | `lytk.to_lilypond(lytk.transpose(score, 3), "out.ly")` |
| Flatten includes | `lytk flatten in.ly -o flat.ly` | *(not yet exposed in Python API)* |
| Metadata | `lytk info in.xml` | `score.title`, `score.parts`, etc. |
| Batch convert | `lytk convert dir/ -o out/ -j 8` | Custom loop with `concurrent.futures` |

The CLI uses rayon for automatic parallel batch processing. In Python, parallelism
requires the user to manage threads or processes (the GIL is released during Rust
calls, so `ThreadPoolExecutor` works for I/O-bound batches).
