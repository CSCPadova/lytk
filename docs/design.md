# Design

## Overview

lytk converts between symbolic music formats (MusicXML, LilyPond, MIDI) through a
shared Internal Representation (IR). The IR is a tree of value objects that can be
cloned, serialised (serde JSON), and transformed. Transforms are composable,
pure-function passes that produce new IR trees without mutating their input.

The Rust crate (`lytk`) is the core library. Python bindings are exposed via PyO3
under `lytk._core`, with a thin Python package at `src/lytk/` providing the public API.

---

## IR Tree Structure

The IR forms a tree rooted at `Score`:

```
Score
├── metadata: ScoreMetadata (title, composer, subtitle, arranger,
│             pitch_language, partial_duration, extra)
├── page_layout: Option<PageLayout>
├── part_groups: Vec<PartGroup>  (brace/bracket groupings)
└── children: Vec<ScoreChild>   (ScoreChild::Part or ScoreChild::Score for movements)
    └── Part
        ├── part_id, name, abbreviation
        ├── midi_channel, midi_program, midi_instrument
        └── measures: Vec<Measure>
            ├── attributes: Option<MeasureAttributes>
            │   ├── key: Option<KeySignature>
            │   ├── time: Option<TimeSignature>
            │   ├── clefs: HashMap<u8, Clef>    (staff number → clef)
            │   ├── divisions: Option<u32>
            │   ├── transpose: Option<Transpose>
            │   └── staves: Option<u8>
            ├── voices: Vec<MeasureVoice>
            │   └── elements: Vec<VoiceElement>
            │       ├── Note    (pitch, duration, ties, slurs, articulations,
            │       │            beams, tuplet, lyrics, grace/cue flags, …)
            │       ├── Rest    (duration, display pitch, is_measure_rest)
            │       ├── Chord   (duration, Vec<Note>, arpeggio)
            │       ├── Forward (time advance)
            │       └── Backup  (time retreat)
            ├── directions: Vec<Direction>
            ├── harmonies: Vec<Harmony>
            ├── figured_basses: Vec<FiguredBass>
            ├── barline: Option<Barline>
            └── width: Option<f32>
```

### Leaf value objects

| Type | Fields |
|---|---|
| `Pitch` | `step: PitchStep`, `alter: Alter`, `octave: i32` |
| `Duration` | `base: Frac`, `dots: u8`, `tuplet_actual: u8`, `tuplet_normal: u8` |
| `Articulation` | `name: String`, `placement: Placement` |
| `Ornament` | `name: String`, `placement: Placement` |
| `Technical` | `name: String`, `value: String` |
| `DynamicMark` | `sign: String`, `placement: Placement` |
| `Wedge` | `wedge_type: String`, `placement: Placement` |
| `BeamEvent` | `beam_type: BeamType`, `number: u8` |
| `TupletDisplay` | `type_: StartStop`, `number: Option<u8>`, `bracket: Option<bool>` |
| `LyricSyllable` | `text: String`, `syllabic: SyllabicType`, `number: u8`, `extend: bool`, `elision: bool` |
| `Fermata` | `shape: String`, `placement: Placement` |
| `TieEvent` / `SlurEvent` | `tie_type / slur_type: StartStop`, `number: u8` |

Articulations, ornaments, technicals, and dynamics use `String` names rather than
exhaustive enums, keeping the IR extensible across MusicXML and LilyPond dialects.

### Note fields

`Note` carries all notation attached to a single pitch:

```rust
pub struct Note {
    pub pitch: Pitch,
    pub duration: Duration,
    pub voice: u8,
    pub staff: u8,
    // articulations & notation
    pub ties: Vec<TieEvent>,
    pub slurs: Vec<SlurEvent>,
    pub articulations: Vec<Articulation>,
    pub ornaments: Vec<Ornament>,
    pub technicals: Vec<Technical>,
    pub dynamics: Vec<DynamicMark>,
    pub wedges: Vec<Wedge>,
    pub text_directions: Vec<TextDirection>,
    pub beams: Vec<BeamEvent>,
    pub tuplet: Option<TupletDisplay>,
    pub fermata: Option<Fermata>,
    pub lyrics: Vec<LyricSyllable>,
    // flags
    pub is_grace: bool,
    pub grace_slash: bool,   // acciaccatura
    pub after_grace: bool,
    pub is_cue: bool,
    pub glissando: Option<StartStop>,
    pub slide: Option<StartStop>,
    pub glissando_line_type: Option<String>,
    pub stem_direction: String,
    pub notehead: String,
    pub print_object: bool,
    pub tremolo_marks: u8,
    pub two_note_tremolo: bool,
    pub tremolo_start: bool,
    pub no_auto_beam: bool,
}
```

### Direction

`Direction` holds a set of optional markings at a single musical position:

```rust
pub struct Direction {
    pub placement: Placement,
    pub offset: Option<i32>,
    pub dynamic: Option<DynamicMark>,
    pub wedge: Option<Wedge>,
    pub tempo: Option<TempoDirection>,
    pub text: Option<TextDirection>,
    pub rehearsal: Option<String>,
    pub octave_shift: Option<OctaveShift>,
    pub pedal: Option<PedalEvent>,
    pub coda: bool,
    pub segno: bool,
    pub da_capo: bool,
    pub dal_segno: bool,
}
```

### Barline

```rust
pub struct Barline {
    pub location: BarlineLocation,
    pub style: BarlineType,
    pub repeat_direction: Option<RepeatDirection>,
    pub ending_number: Option<String>,
    pub ending_type: Option<EndingType>,
    pub fermata: Option<Fermata>,
}
```

---

## Pitch Languages

`PitchLanguage` represents the 11 note-naming languages supported by LilyPond
(`nederlands`, `english`, `deutsch`, `italiano`, `français`, `español`, `português`,
`norsk`, `suomi`, `svenska`, `vlaams`). Each language maps `(step, alter)` → note name
string. `PitchMode` tracks absolute vs. relative pitch context.

Pitch language and mode are stored in `ScoreMetadata` and used at emit time, not during
parsing. This keeps the IR format-agnostic.

---

## Adapter Pattern

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

`AdapterError` is a single enum with variants for IO, XML, ZIP, LilyPond parsing,
format-specific, missing data, and unsupported features.

### MusicXML → IR

`MxmlToIrAdapter` converts in two phases:

1. **DOM construction** — quick-xml streaming events are collected into a minimal
   `XmlNode` tree (element name, attributes, text content, children). This provides
   random-access traversal without the weight of a full DOM crate.

2. **Tree walk** — `<score-partwise>` → `Score`, `<part>` → `Part`, `<measure>` →
   `Measure`, `<note>` → `Note`/`Rest`, `<direction>` → `Direction`, `<barline>` →
   `Barline`, `<harmony>` → `Harmony`, `<figured-bass>` → `FiguredBass`, etc.

MXL archives (`.mxl`) are ZIP files. `mxl_zip::read_musicxml()` extracts the rootfile
from `META-INF/container.xml` and returns the XML content.

### LilyPond → IR

`LyToIrAdapter` uses tree-sitter via `src/parser.rs` to parse `.ly` files into a CST,
then walks the CST with a stateful `WalkState`:

- `\relative` / `\absolute` pitch context tracking
- Pitch language from `\language` command
- `tuplet_stack` for nested tuplet ratio propagation
- `auto_beam_off` flag for `\autoBeamOff`/`\autoBeamOn`
- Variable definitions pre-parsed from position 0 and spliced in where used
- Positioned reading: every note sits at its absolute onset in a voice lane,
  every attribute/direction/barline is an event at a position
  (`ly_to_ir/timeline.rs`); simultaneous music overlays by position
- One bar-splitter at score assembly: a score-wide meter grid (anchored at the
  start, `\partial` and each `\time`), explicit barlines as extra boundaries,
  a `\cadenzaOn … \cadenzaOff` span as one free bar — LilyPond's model, where
  bar checks only check
- Post-processing: clef-aware auto-stem, tuplet-aware auto-beam grouping,
  lyrics attachment

### LilyPond Flatten

`ly_flatten::flatten()` processes a file textually (not via the IR pipeline):

1. Scan line by line, tracking block comments (`%{ %}`)
2. Match `\include "path"` and resolve the path (relative to including file,
   with `.ly`/`.ily` extension fallback, then `-I` paths)
3. Check for circular dependency via ancestor set
4. Recurse into included files, wrap with comment markers
5. Post-process: deduplicate `\version`/`\language` (keep last, warn), error on
   multiple `\header` blocks

---

## Transform Pattern

```rust
pub trait Transform {
    fn apply(&self, score: &Score) -> Score;
}
```

Transforms borrow `&Score` and return a new owned `Score`. They never mutate in place:

- **Idempotency** — `T(T(x)) == T(x)` for well-behaved transforms.
- **Composition** — `apply_all(&[&dyn Transform], &Score)` chains transforms left-to-right.
- **Parallelism** — different `Score` objects can be transformed concurrently via rayon.

The dual API convention (following torchaudio):
- OOP: `Transpose::new(2).apply(&score) -> Score`
- Functional: `transpose(&score, 2) -> Score`

Implemented transforms: `Transpose`, `ChangeLanguage`, `Invert`, `Retrograde`.

---

## Testing Strategy

- **Unit tests** — colocated with each module (`#[cfg(test)]`). 255 passing.
- **Fixture tests** — 143 MusicXML files from `musicxmlTestSuite/` in `tests/fixtures/xml/`.
- **Round-trip tests** — `LilyPond → IR → LilyPond` and `MusicXML → IR → MusicXML`,
  comparing semantic equivalence. Currently tested via `test_roundtrip_*` in the adapter tests.
- **CLI integration tests** — `tests/cli.rs` using `assert_cmd` + `predicates` + `tempfile`.
  15 passing.
- **Python tests** — `pytest tests/` (26 tests) for PyO3 binding correctness.
- **Criterion benchmarks** — `benches/benchmarks.rs`, 25 benchmarks.
- **pytest-benchmark** — `benches/bench_python.py`, cross-language comparison.

---

## Key Design Decisions

**String-based names for articulations/dynamics/ornaments** — avoids a fragile enum that
must cover every MusicXML and LilyPond notation element. The adapter maps format-specific
names.

**Mini-DOM (`XmlNode`) for MusicXML parsing** — balances simplicity, random access, and
memory use. Avoids SAX complexity or a full DOM crate dependency.

**`tuplet_stack` for nested tuplets** — the `WalkState` maintains a stack of active
`(actual, normal)` tuplet ratios. `push_voice_element` applies the innermost ratio before
recording the element's duration, ensuring measure tracking is correct for sextuplets and
other complex cases.

**Grace notes excluded from measure duration** — `push_voice_element` skips elapsed-time
advancement for grace notes (`is_grace: true`), preventing false measure flushes.

**Post-process auto-beam and auto-stem** — beaming and stem direction are not assigned
during LilyPond parsing (which would require two-pass lookahead) but in a post-processing
pass after the full measure is assembled. This allows clef-aware stem direction and
time-signature-aware beam grouping.

**`no_auto_beam` flag** — notes under `\autoBeamOff` are marked with `no_auto_beam: true`
during parsing. The post-processing beam pass skips these notes, preserving explicit `[]`
brackets while suppressing auto-beaming.

**`Score` uses `Clone` for immutable transform pattern** — transforms clone and modify.
For large scores, copy-on-write or arena allocation is a future optimisation.

**Flatten is textual, not IR-level** — `\include` expansion preserves every character of
the original source (comments, whitespace, formatting). Using the IR pipeline would
destroy this fidelity. The textual approach matches lyp and Lilybert's design.
