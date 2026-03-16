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

Reference: `python-ly/` (output patterns)

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

Reference: `python-ly/` (tokenizer), `quickly/` (improved tokenizer)

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

Reference: MusicXML 4.0 specification

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

## 8. Criterion Benchmarks ✅

**Module:** `benches/`

Measure performance of hot paths:

- MusicXML parsing (single file + batch)
- LilyPond parsing (single file + batch)
- Transform application (transpose on large score)
- Round-trip conversion

Implemented:
- `benches/benchmarks.rs` — 25 Criterion benchmarks across 4 groups (parsing, emission, transforms, round-trips)
- `benches/bench_python.py` — 16 pytest-benchmark tests comparing lytk (Rust) vs python-ly (pure Python)
- python-ly used as the pure-Python baseline
- Measured speedups: transpose ~52×, language change ~40× over python-ly

## 9. Further Adapters

Lower priority:

- **ABC notation** — `src/adapters/abc_to_ir.rs`, `src/adapters/ir_to_abc.rs`
- **MEI** — `src/adapters/mei_to_ir.rs` (reference: `MEILER/`)
- **Humdrum** — `src/adapters/hum_to_ir.rs` (reference: `hum2ly/`, `lilypond-export/`)

## 10. LilyPond Parser — Extended Feature Support ✅

**Module:** `src/adapters/ly_to_ir.rs`

The LilyPond → IR parser now handles core notation plus the following extended
features, enabling round-trip fidelity (LilyPond → IR → LilyPond):

| Feature | LilyPond syntax to recognise | IR target | Status |
|---|---|---|---|
| Anacrusis (pickup) | `\partial <dur>` | `ScoreMetadata.partial_duration` | ✅ |
| Glissando | `\glissando`, `\once \override Glissando.style` | `Note.glissando`, `Note.glissando_line_type` | ✅ |
| Arpeggio | `\arpeggio`, `\arpeggioArrowUp/Down`, `\arpeggioBracket` | `Chord.arpeggio` | ✅ |
| After-grace | `\afterGrace { ... }` | `Note.after_grace` | ✅ |
| Paper block | `\paper { ... }` | `Score.page_layout` | ✅ |
| Coda / Segno marks | `\mark \markup { \musicglyph "scripts.coda" }` | `Direction.coda`, `Direction.segno` | ✅ |
| Da Capo / Dal Segno | `\mark "D.C."`, `\mark "D.S. al Coda"` | `Direction.da_capo`, `Direction.dal_segno` | ✅ |
| Slide | `\glissando` with `\override Glissando.style = #'trill` | `Note.slide` | ✅ |
| Lyrics | `\lyricsto`, `\lyricmode`, `\context Lyrics` | `Note.lyrics` | ✅ |
| `\set Staff.instrumentName` | `\set Staff.instrumentName="..."` | `Part.name` | ✅ |
| `\set Staff.midiInstrument` | `\set Staff.midiInstrument="..."` | `Part.midi_instrument` | ✅ |
| Named voices | `\context Voice = "name"` | Voice name for lyrics | ✅ |
| Staff variables | `varName = \new Staff { ... }` | Full `Part` preservation | ✅ |
| Chord names | `\chordmode { ... }` | `Measure.harmonies` | 🔲 |
| Figured bass | `\figuremode { <...> }` | `Measure.figured_bass` | 🔲 |

Implemented:
- 13 of 16 features fully parsing; 19 new tests (215 total)
- `\once \override Glissando.style = #'<style>` → dashed/dotted/wavy/trill
- `\arpeggioArrowUp/Down/Bracket` direction state tracking
- `\paper { ... }` block → `PageLayout` with page/margin/spacing fields
- `\mark` dispatcher: text strings for D.C./D.S., markup blocks for coda/segno glyphs
- `\set Staff.instrumentName/midiInstrument` → Part metadata
- `\lyricsto/\lyricmode/\context Lyrics` → lyrics attached to notes
- Staff variable definitions (`name = \new Staff { }`) preserve full Part metadata
- `\context Voice = "name"` for named voice tracking (lyrics attachment)
- `\cadenzaOn/Off`, `\melisma/End`, `\autoBeamOff`, `\dynamicUp/Down` gracefully handled

## 11. MusicXML 4.0 Completeness

Cross-reference of the MusicXML 4.0 schema (`musicxml/schema/`) against the
current parser (`src/adapters/mxml_to_ir.rs`) and emitter (`src/adapters/ir_to_mxml.rs`).

### 11a. Emitter gaps (`ir_to_mxml.rs`)

| Priority | Feature | Schema file | IR field | Status |
|---|---|---|---|---|
| HIGH | Accidental display (cautionary / forced / editorial) | `note.mod` | `Pitch.accidental: AccidentalDisplay` | ✅ |
| HIGH | Glissando notation | `note.mod` | `Note.glissando`, `Note.glissando_line_type` | ✅ |
| HIGH | Slide (portamento) notation | `note.mod` | `Note.slide` | ✅ |
| HIGH | Arpeggiate (up / down) | `note.mod` | `Chord.arpeggio: ArpeggioType::Up/Down` | ✅ |
| HIGH | Non-arpeggiate | `note.mod` | `Chord.arpeggio: ArpeggioType::NonArpeggio` | ✅ |
| HIGH | After-grace `steal-time-previous` | `note.mod` | `Note.after_grace` | ✅ |
| HIGH | Harmony / chord symbols (`<harmony>`) | `direction.mod` | `Measure.harmonies: Vec<Harmony>` | ✅ |
| HIGH | Page layout (`<defaults>`) | `layout.mod` | `Score.page_layout: PageLayout` | ✅ |
| MEDIUM | Coda / Segno marks | `direction.mod` | `Direction.coda`, `Direction.segno` | ✅ |
| MEDIUM | Da Capo / Dal Segno (`<sound>`) | `direction.mod` | `Direction.da_capo`, `Direction.dal_segno` | ✅ |
| MEDIUM | Figured bass (`<figured-bass>`) | `direction.mod` | `Measure.figured_bass: Vec<FiguredBass>` | ✅ |
| LOW | Print-object on notes | `common.mod` | `Note.print_object` | ✅ |
| MEDIUM | Grace note `slash` attribute | `note.mod` | `Note.grace_slash` | ✅ |
| MEDIUM | Notehead element | `note.mod` | `Note.notehead` | ✅ |
| MEDIUM | Measure `width` attribute | `common.mod` | `Measure.width` | ✅ |
| MEDIUM | Text direction font attributes | `direction.mod` | `TextDirection.font_style/font_weight` | ✅ |
| MEDIUM | Pedal `line` attribute | `direction.mod` | `PedalEvent.line` | ✅ |
| MEDIUM | Lyric `<elision>` element | `note.mod` | `LyricSyllable.elision` | ✅ |
| MEDIUM | MIDI instrument in `<score-part>` | `score.mod` | `Part.midi_channel/program/instrument` | ✅ |
| MEDIUM | Subtitle as `<credit>` | `score.mod` | `ScoreMetadata.subtitle` | ✅ |
| LOW | Extra creator metadata | `identity.mod` | `ScoreMetadata.extra` | ✅ |
| LOW | PartGroup number preservation | `score.mod` | `PartGroup.number` | ✅ |
| MEDIUM | Tempo text as `<words>` with `<metronome>` | `direction.mod` | `TempoDirection.text` | ✅ |
| LOW | Merged `<sound>` element (tempo + dacapo/dalsegno) | `direction.mod` | Single `<sound>` with all attrs | ✅ |

### 11b. Parser gaps (`mxml_to_ir.rs`)

| Priority | Feature | Schema file | Notes |
|---|---|---|---|
| MEDIUM | `<measure-style>` (multi-rest, slash, etc.) | `attributes.mod` | Needs `MeasureStyle` IR type |
| MEDIUM | `<print>` element (new-system, new-page, blank-page) | `layout.mod` | Needs `PrintDirective` IR type |
| MEDIUM | `<sound>` `tempo` on nested-in-voice elements | `direction.mod` | Currently only top-level sound |
| LOW | `<dashes>` / `<bracket>` spanners | `direction.mod` | Needs spanner tracking |
| LOW | Non-traditional key signatures (`<key-step>` / `<key-alter>`) | `attributes.mod` | Rare; no IR target |
| LOW | `<interchangeable>` / `<senza-misura>` time variants | `attributes.mod` | Rare edge case |
| LOW | `<movement-number>` / `<work-number>` | `score.mod` | Extend `ScoreMetadata` |
| LOW | Harmony `<inversion>` / `<function>` / `<frame>` | `direction.mod` | Extend `Harmony` struct |
| LOW | Figured bass figure `<extend>` | `direction.mod` | Extend `Figure` struct |
| LOW | `<part-symbol>` brace / bracket | `attributes.mod` | Cosmetic; extend `PartGroup` |

### 11c. Out of scope (v1)

These MusicXML elements have no planned IR representation for the initial
release — they are too hardware-specific, too rare, or not relevant to the
LilyPond round-trip use case:

- `<harp-pedals>` / `<accordion-registration>` — hardware-specific notation
- `<scordatura>` — tuning override notation
- `<percussion>` pictogram elements — complex symbol table
- `<image>` — embedded raster images
- `<listen>` / `<listening>` — performance instructions (MusicXML 4.0 new)
- `<staff-divide>` arrow — orchestral condensed score notation
