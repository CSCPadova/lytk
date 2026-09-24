# Command line

`pip install lytk` installs the `lytk` command (also runnable as
`python -m lytk.cli`). `lytk --help` lists the commands and `lytk COMMAND --help`
shows each one's options.

| Command | What it does |
|---|---|
| [`convert`](#convert) | Convert a file, or a whole folder, between formats |
| [`transpose`](#transpose) | Transpose by semitones, by an interval, or to a key |
| [`invert`](#invert) | Mirror every pitch around an axis |
| [`retrograde`](#retrograde) | Play the music backwards |
| [`change-language`](#change-language) | Change the LilyPond note-name language |
| [`abs2rel`, `rel2abs`](#abs2rel-and-rel2abs) | Rewrite LilyPond with relative or absolute octaves |
| [`info`](#info) | Show metadata and parts |
| [`positions`](#positions) | Each bar's start and length, as JSON |
| [`bundle`](#bundle) | Write each part to its own file |
| [`diff`](#diff) | Compare two scores by what they sound |
| [`batch`](#batch) | Run a JSON list of conversion jobs |
| [`flatten`](#flatten) | Inline every `\include` of a LilyPond file |

## Formats and streams

Formats are taken from file extensions:

| Format | Extensions | `--format` / `--from` |
|---|---|---|
| LilyPond | `.ly` `.ily` | `ly` |
| MusicXML | `.xml` `.musicxml` | `xml` |
| Compressed MusicXML | `.mxl` | `mxl` |
| MIDI | `.mid` `.midi` | `midi` |
| ABC | `.abc` | `abc` |
| Humdrum `**kern` | `.krn` `.kern` | `krn` |

`--format`/`-f` overrides the output format and `--from` the input format.
Every command reads stdin when the input is `-` and writes stdout when the
output is `-`. There is no extension to go by then, so stdin needs `--from`
and stdout needs `--format`:

```bash
cat score.ly | lytk convert - -o - --from ly -f xml > score.xml
```

Errors are one line on stderr (`error: …`) with exit status 1. Invalid
arguments exit with status 2 and a usage message.

When both the input and the output are LilyPond, `convert` and the transforms
go through lytk's Music tree rather than its measure-based score. That keeps the
source's structure (its contexts, and `\repeat` and `\alternative` blocks)
instead of rebuilding it from measures.

## convert

```
lytk convert INPUT -o OUTPUT [-f FORMAT] [--from FORMAT] [-j N]
```

```bash
lytk convert score.xml -o score.ly
lytk convert score.ly -o score.mxl          # compressed MusicXML (a ZIP)
lytk convert score.mid -o score.xml
lytk convert score.xml -o score.txt -f ly   # extension says nothing: force it
```

**A folder.** When `INPUT` is a directory, every score in it (recursively) is
converted into the `OUTPUT` directory, keeping subfolders. Without `-f`,
LilyPond files become MusicXML and everything else becomes LilyPond. Files are
converted in parallel, `-j N` workers (default one per CPU); a file that fails
is reported and skipped, and the exit status is 1 if any failed.

```bash
lytk convert corpus/ -o out/ -f midi -j 8
```

**Several movements.** A LilyPond file with several `\score` blocks writes the
first movement to `OUTPUT` and the others next to it, as `NAME_02.EXT`,
`NAME_03.EXT`, …. Several movements can't go to stdout.

## transpose

```
lytk transpose INPUT -o OUTPUT (-s N | -i INTERVAL | --to-key KEY) [-f FORMAT] [--from FORMAT]
```

Give exactly one of:

- `-s`/`--semitones N`: chromatic, by N semitones (negative is down).
- `-i`/`--interval NAME`: diatonic, by a named interval, so spelling follows
  the interval: `M3`, `m3`, `P5`, `A4`, `d5`, `P8`; a leading `-` goes down
  (`-m2`).
- `--to-key KEY`: so the tonic becomes `KEY` (`D`, `Bb`, `F#`), moving the
  nearest way.

Key signatures and chord symbols follow, and pitches are re-spelled for the new
key.

```bash
lytk transpose song.ly -o up.ly -s 3
lytk transpose song.xml -o song-in-d.xml --to-key D
```

## invert

```
lytk invert INPUT -o OUTPUT [-a AXIS] [-f FORMAT] [--from FORMAT]
```

Mirrors every pitch around `AXIS` (default `c4`, middle C). The axis is a note
letter, optional accidentals (`s` or `#` sharp, `f` flat) and an octave:
`c4`, `fs3`, `bf5`.

## retrograde

```
lytk retrograde INPUT -o OUTPUT [-f FORMAT] [--from FORMAT]
```

Reverses the music in time; ties, slurs and tuplets are reversed with it.

## change-language

```
lytk change-language INPUT -o OUTPUT -l LANGUAGE [-f FORMAT] [--from FORMAT]
```

Sets the note-name language of LilyPond output: `nederlands`, `english`,
`deutsch`, `norsk`, `suomi`, `svenska`, `italiano`, `catalan`, `espanol`,
`portugues`, `vlaams` or `français`. Names are written the way LilyPond spells
them in that language.

## abs2rel and rel2abs

```
lytk abs2rel INPUT.ly -o OUTPUT
lytk rel2abs INPUT.ly -o OUTPUT
```

Rewrite a LilyPond file with `\relative` octave marks, or with absolute ones.
Parts with several staves or voices keep absolute octaves where relative ones
would be ambiguous.

## info

```
lytk info INPUT [--json]
```

```
Title:    Pitches and accidentals
Parts:    1
  - MusicXML Part (28 measures)
```

`--json` prints title, subtitle, composer, arranger, lyricist, language,
`part_count`, `note_count`, and for each part its id, name, abbreviation,
number of measures, staves, MIDI program and MIDI instrument.

## positions

```
lytk positions INPUT [--from FORMAT]
```

Prints each part's bars as JSON, with offsets in quarter notes:

```json
{"unit": "quarter", "parts": [{"id": "P1", "measures": [{"number": 1, "start": 0.0, "duration": 4.0}]}]}
```

A bar lasts as long as its longest voice, so pickups and short bars count as
written, and grace notes take no time. These are musical positions from the
notated durations, not graphical ones.

## bundle

```
lytk bundle INPUT -o DIR [-f FORMAT] [--from FORMAT]
```

Writes each part to its own file, `DIR/<input-name>_<part>.<ext>`, in `FORMAT`
(default `xml`). Each file keeps the score's title and other metadata.

## diff

```
lytk diff A B [--json] [--from FORMAT]
```

Compares two scores by what they sound: the number of parts, the number of
notes and the multiset of pitches. It exits with status 0 when they match and
1 when they differ, so it can gate a pipeline. `--json` prints `equal`,
`parts`, `note_count` and `pitch_multiset_equal`.

```bash
lytk convert song.ly -o song.xml && lytk diff song.ly song.xml
```

## batch

```
lytk batch JOBS.json [-j N] [--report FILE]
```

Runs a JSON array of jobs:

```json
[
  {"in": "a.xml", "out": "out/a.ly"},
  {"in": "b.ly", "out": "out/b.mid", "transpose": -2},
  {"in": "c.mid", "out": "out/c.xml", "interval": "M3"},
  {"in": "d.dat", "out": "out/d.krn", "from": "xml"}
]
```

`in` and `out` (or `input` and `output`) are required. `format` and `from`
override the formats; `transpose` (semitones) or `interval` transposes first.
Jobs run in parallel (`-j`, default one per CPU) and independently. A failing
job doesn't stop the others, but the exit status is 1. `--report FILE` (or `-`)
writes one `{"input", "output", "ok", "error"?}` entry per job.

## flatten

```
lytk flatten INPUT.ly [-o OUTPUT] [-I DIR ...] [--no-markers]
```

Inlines every `\include`, recursively, into one self-contained file (stdout
without `-o`). Includes are looked up next to the file that includes them, then
with `.ly` and `.ily` appended, then in each `-I`/`--include-path` directory.
A missing file and an include cycle are errors; the cycle is shown
(`a.ly -> b.ly -> a.ly`).

Repeated `\version` and `\language` lines are merged, keeping the last one and
warning on stderr. More than one `\header` block is an error. Each inlined file
is wrapped in `% === BEGIN INCLUDE: … ===` / `% === END INCLUDE: … ===`
comments; `--no-markers` leaves them out.

## From Python

Everything above is also available as Python functions (`lytk.from_musicxml`,
`lytk.transpose`, `lytk.to_lilypond`, …); see the README's quick start. The
command itself is a Typer app, `lytk.cli.app`.
