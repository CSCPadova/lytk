# Design

## Overview

lytk converts between symbolic music formats (MusicXML, LilyPond, MIDI, ABC) through a shared Internal Representation (IR). The IR is a tree of immutable value objects that can be cloned, serialized, and transformed. Transforms are composable, idempotent passes that produce new IR trees without mutating their input.

The Rust crate (`lytk`) is the core library. Python bindings are exposed via PyO3 under `lytk._core`, with a thin Python package at `src/lytk/` providing the public API.

## IR Tree Structure

The IR forms a tree rooted at `Score`:

```
Score
├── metadata: ScoreMetadata (title, composer, pitch_language, pitch_mode, …)
├── part_groups: Vec<PartGroup>
│   └── parts: (indices into Score.parts)
└── parts: Vec<Part>
    ├── info: PartInfo (id, name, abbreviation, midi_program, midi_channel)
    └── voices: Vec<Voice>
        └── measures: Vec<Measure>
            ├── attributes: MeasureAttributes (key, time, clef, divisions, transpose, staves)
            ├── notes: Vec<NoteEvent>  — Note, Rest, Chord, Spacer
            ├── directions: Vec<Direction>
            └── barline: Option<Barline>
```

### Leaf Value Objects

These are plain structs, not tree nodes:

- **Pitch** — step (0–6), alter (float, semitones), octave. Constructed via `Pitch::new()` or `Pitch::from_midi()`. Comparison and display are derived.
- **Duration** — numerator/denominator fraction plus dot count. `from_divisions(value, divisions, dots)` converts MusicXML divisions. `to_lilypond()` renders as `.ly` duration token.
- **Articulation** — `name: String` (e.g. `"staccato"`, `"accent"`), `placement: Placement`.
- **Ornament** — `name: String` (e.g. `"trill"`, `"mordent"`), `placement: Placement`.
- **Technical** — `name: String`, `value: String` (e.g. `name="fingering"`, `value="3"`).
- **DynamicMark** — `sign: String` (e.g. `"ff"`, `"mp"`), `placement: Placement`.
- **Wedge** — `wedge_type: String` (e.g. `"crescendo"`), `placement: Placement`.

Articulations, ornaments, technicals, dynamics, and wedges use `String` names rather than enums to stay extensible. The adapter is responsible for mapping format-specific names.

### NoteEvent Variants

`NoteEvent` is an enum with four variants:

- **Note** — pitch, duration, dots, voice number, staff, plus optional: stem direction, beam events, tie, slurs, articulations, ornaments, technicals, lyrics, grace/cue flags, tuplet display, accidental display, notehead.
- **Rest** — duration, dots, voice, staff, display step/octave.
- **Chord** — a `Vec<Note>` sharing a single duration.
- **Spacer** — duration, dots, voice.

### Direction

A `Direction` holds at most one of each: `dynamic`, `wedge`, `tempo`, `text`, `rehearsal`, `octave_shift`, `pedal`. Plus `placement` and `offset`.

### Barline

`Barline` has `style: BarlineType` (enum: Regular, Double, Final, RepeatForward, RepeatBackward, RepeatBoth, Dashed, Short, Tick, None), plus optional `repeat_direction`, `ending_number`, `ending_type`, `fermata`.

## Pitch Languages

`PitchLanguage` represents the 11 note-naming languages supported by LilyPond (nederlands, english, deutsch, italiano, français, español, português, norsk, suomi, svenska, vlaams). Each language maps step + alteration → note name string. `PitchMode` tracks absolute vs. relative pitch context.

Pitch language and mode are stored in `ScoreMetadata` and used at emit time, not during parsing.

## Adapter Pattern

All adapters implement one of two traits:

```rust
pub trait ToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score>;
    fn convert_str(&self, input: &str) -> Result<Score>;
}

pub trait FromIrAdapter {
    fn convert(&self, score: &Score) -> Result<String>;
    fn write(&self, score: &Score, path: &Path) -> Result<()>;
}
```

`AdapterError` is a single enum with variants for IO, XML parsing, ZIP handling, LilyPond parsing, format-specific errors, missing data, and unsupported features.

### MusicXML → IR

`MxmlToIrAdapter` converts MusicXML to IR in two phases:

1. **DOM construction** — quick-xml streaming events are collected into an `XmlNode` tree (element name, attributes, text content, children). This is a minimal DOM that avoids the complexity of a full XML DOM while providing random-access traversal.

2. **Tree walk** — the `XmlNode` tree is walked top-down:
   - `<score-partwise>` → `Score`
   - `<work>`, `<identification>`, `<movement-title>` → `ScoreMetadata`
   - `<part-list>` → `PartInfo` entries + `PartGroup` ranges
   - `<part>` → `Part`, with stateful `divisions` tracking across measures
   - `<measure>` → `Measure`, with notes sorted by voice number and chords merged
   - `<note>` → `Note`/`Rest`, with pitch, duration, notations, lyrics
   - `<direction>` → `Direction` with dynamics, wedges, tempo, text, rehearsal, pedal, octave-shift
   - `<barline>` → `Barline`

MXL archives (`.mxl`) are ZIP files. `mxl_zip::read_musicxml()` extracts the rootfile from `META-INF/container.xml` and returns the XML content.

## Transform Pattern

```rust
pub trait Transform {
    fn apply(&self, score: &Score) -> Score;
}
```

Transforms borrow a `&Score` and return a new owned `Score`. They never mutate in place. This enables:

- **Idempotency** — `T(T(x)) == T(x)` for well-behaved transforms.
- **Composition** — `apply_all(&[&dyn Transform], &Score)` chains transforms left-to-right.
- **Parallelism** — different `Score` objects can be transformed concurrently via rayon.

The dual API convention (following torchaudio):
- OOP: `Transpose::new(2).apply(&score) -> Score`
- Functional: `transpose(&score, 2) -> Score`

## Testing

- **Unit tests** — colocated with each module (`#[cfg(test)]`).
- **Fixture tests** — 143 MusicXML files from `musicxmlTestSuite/` are copied to `tests/fixtures/xml/` and parsed in `test_parse_all_fixtures`.
- **Round-trip tests** — planned: MusicXML → IR → LilyPond → IR → MusicXML, testing semantic equivalence (not byte equality).
- **Property tests** — `proptest` in dev-dependencies for transform idempotency and inverse-pair testing.
- **Benchmarks** — planned with `criterion`.

## Key Design Decisions

- **String-based names for articulations/dynamics/ornaments** rather than exhaustive enums. This keeps the IR extensible and avoids maintaining a fragile enum that must cover every MusicXML and LilyPond notation element.
- **Mini-DOM (`XmlNode`) for MusicXML parsing** rather than SAX-style streaming or a full DOM crate. This balances simplicity, random access, and memory use.
- **Singular `Direction` fields** (one dynamic, one wedge per direction) rather than vectors, matching the typical MusicXML structure where each `<direction>` contains one type of marking.
- **`divisions` tracked stateully** across measures within a part, matching MusicXML semantics where `<attributes>` can change divisions mid-part.
- **`Score` uses `Clone` for immutable transform pattern** — transforms clone the score and modify the clone. For large scores, copy-on-write or arena allocation is a future optimization.
