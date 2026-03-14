# Roadmap

What needs to be implemented, roughly in priority order.

## 1. IR → LilyPond Emitter ✅

**Module:** `src/adapters/ir_to_ly.rs`

Emit a `Score` as LilyPond `.ly` text. This completes the MusicXML → LilyPond pipeline.

Key tasks:
- Emit `\version`, `\header`, `\score` boilerplate
- Emit `\new Staff` / `\new Voice` structure from parts and voices
- Emit notes with correct pitch names in the active `PitchLanguage`
- Handle relative vs absolute pitch mode (`\relative` / `\absolute`)
- Emit durations, dots, ties, slurs, beams
- Emit articulations, dynamics, hairpins (wedges)
- Emit clef, key, time changes as inline `\clef`, `\key`, `\time`
- Emit barlines, repeats, endings (volta brackets)
- Emit tuplets (`\tuplet`)
- Emit grace notes (`\grace`, `\acciaccatura`, `\appoggiatura`)
- Emit lyrics (`\addlyrics` or `\lyricsto`)
- Emit directions: tempo, rehearsal, octave shifts, pedal, text
- Handle multi-staff instruments (piano grand staff)

Reference: `lytk-py/converters/ir_to_ly.py`

## 2. LilyPond → IR Parser ✅

**Module:** `src/adapters/ly_to_ir.rs`

Use the tree-sitter LilyPond grammar (already wrapped in `src/parser.rs`) to parse `.ly` files into the IR.

Key tasks:
- Walk tree-sitter CST nodes, mapping to IR types
- Handle `\relative` / `\absolute` pitch context
- Resolve pitch language from `\language` command
- Parse note/rest/chord/spacer tokens
- Parse `\new Staff`, `\new Voice`, `\new Score` structure
- Handle `\repeat volta`, `\alternative`
- Parse `\header` block → `ScoreMetadata`
- Handle `\include` directives (read and inline included files)
- Resolve variables (`myMelody = { ... }` → inline at usage)
- Handle `\transpose` context (transposing instrument parts)

This is the most complex adapter due to LilyPond's flexible syntax. Consider implementing incrementally: simple single-voice scores first, then multi-voice, then variables/includes.

Reference: `python-ly/` (tokenizer), `quickly/` (improved tokenizer), `lytk-py/converters/ly_to_ir.py`

## 3. IR → MusicXML Emitter ✅

**Module:** `src/adapters/ir_to_mxml.rs`

Emit a `Score` as MusicXML XML. This completes the LilyPond → MusicXML pipeline and enables round-trip testing.

Key tasks:
- Emit `<score-partwise>` with `<part-list>` and `<part>` elements
- Choose and emit `<divisions>` per part
- Emit `<attributes>` (key, time, clef, transpose)
- Emit `<note>` with `<pitch>`, `<duration>`, `<type>`
- Emit chords (`<chord/>` tag)
- Emit `<direction>` elements (dynamics, wedges, tempo, text, rehearsal, pedal, octave-shift)
- Emit `<barline>` with repeat and ending information
- Emit `<lyric>` elements
- Emit grace notes, cue notes, tuplets

Reference: `lytk-py/converters/ir_to_mxml.py`

## 4. Core Transforms ✅

**Module:** `src/transforms/` (one file per transform or small group)

Each transform implements the `Transform` trait. Planned transforms:

| Transform | Description |
|---|---|
| `Transpose` | Shift all pitches by N semitones, updating key signatures |
| `ChangeLanguage` | Change pitch language (e.g. `english` → `deutsch`) |
| `Invert` | Invert intervals around an axis pitch |
| `Retrograde` | Reverse note order within each voice |
| `RelToAbs` / `AbsToRel` | Convert between relative and absolute pitch mode |
| `AddBarlines` | Insert barlines based on time signature |
| `RemoveBarlines` | Strip barlines |
| `Reformat` | Normalize whitespace / indentation (LilyPond-specific) |
| `InlineVariables` | Resolve LilyPond variables to their values |
| `InlineIncludes` | Resolve `\include` files inline |

Start with `Transpose` and `ChangeLanguage` — they are most useful for data augmentation and exercise the pitch/language system.

## 5. CLI Implementation ✅

**Module:** `src/main.rs`

Replace the current stub with a real CLI using `clap`:

```
lytk convert input.xml -o output.ly          # single file
lytk convert input_dir/ -o output_dir/ -j8   # batch with 8 threads
lytk transpose input.ly --semitones 3 -o out.ly
lytk info input.xml                          # print score metadata
```

Key tasks:
- `convert` subcommand — detect input format, choose adapter pipeline, write output
- `--jobs` / `-j` flag for rayon thread count
- `--format` / `-f` to force output format
- Progress reporting for batch mode
- Error reporting with file paths

## 6. MIDI Adapter ✅

**Modules:** `src/adapters/midi_to_ir.rs`, `src/adapters/ir_to_midi.rs`

Behind the `midi` Cargo feature (uses `midly` crate).

Key tasks:
- Parse MIDI events → IR notes with pitch and approximate duration
- Quantize MIDI timing to musical durations
- Map MIDI channels to parts
- Emit IR → MIDI with correct timing, velocity, program changes

MIDI is lossy — it doesn't carry key signatures, articulations, lyrics, or notation details. The adapter should preserve what it can and mark unknowns.

## 7. Python Bindings ✅

**Modules:** `src/lib.rs` (PyO3 exports), `src/lytk/__init__.py`, `src/lytk/_core.pyi`

Expose the Rust API to Python:

```python
import lytk

score = lytk.from_musicxml("input.xml")
score = lytk.transpose(score, semitones=3)
lytk.to_lilypond(score, "output.ly")
```

Implemented:
- `Score` pyclass with metadata properties, JSON/dict serialisation, `__repr__`/`__str__`/`__eq__`
- Adapter functions: `from_musicxml`, `from_musicxml_string`, `from_lilypond`, `from_lilypond_string`, `to_lilypond`, `to_musicxml`, `from_midi`, `to_midi`
- Transform functions: `transpose`, `change_language`, `invert`, `retrograde`
- `_core.pyi` type stubs, `__init__.py` re-exports with MIDI feature guard
- 26 Python tests (pytest)

## 8. Criterion Benchmarks

**Module:** `benches/`

Measure performance of hot paths:

- MusicXML parsing (single file + batch)
- LilyPond parsing (single file + batch)
- Transform application (transpose on large score)
- Round-trip conversion

Profile Python baseline first (from `lytk-py/`), then gate Rust optimization on measured speedup.

## 9. Further Adapters

Lower priority:

- **ABC notation** — `src/adapters/abc_to_ir.rs`, `src/adapters/ir_to_abc.rs`
- **MEI** — `src/adapters/mei_to_ir.rs` (reference: `MEILER/`)
- **Humdrum** — `src/adapters/hum_to_ir.rs` (reference: `hum2ly/`, `lilypond-export/`)
