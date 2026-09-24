# Import / Export Feature Matrix

Overview of supported features for each format adapter. Each section covers
what the adapter can parse (import) and emit (export).

## Format support at a glance

Every format reads/writes through the shared IR, so any input converts to any
output. The library has two IR layers — Layer 2 (`Score`, measure-based) and
Layer 1 (`MusicDocument`, the Music tree the ML representations consume).

| Format | Extensions | Import | Export | Notes |
|---|---|:---:|:---:|---|
| LilyPond | `.ly` `.ily` | ✅ | ✅ | Score + Music-tree paths; all 12 LilyPond pitch languages |
| MusicXML | `.xml` `.musicxml` | ✅ | ✅ | The most complete adapter |
| Compressed MusicXML | `.mxl` | ✅ | ✅ | ZIP handled natively |
| MIDI | `.mid` `.midi` | ✅ | ✅ | Lossy (no slurs/articulations/lyrics) |
| ABC | `.abc` | ✅ | ✅ | Core subset (headers, notes, chords, ties, repeats) |
| Humdrum (`**kern`) | `.krn` | ✅ | ✅ | Core `**kern`; spine rearrangement (`*^`/`*v`) unsupported |
| MEI | `.mei` | 🔲 | 🔲 | Planned |

**Python:** `from_lilypond` · `from_musicxml` · `from_midi` · `from_abc` ·
`from_humdrum` (+ `_string` variants) → `Score`; `to_lilypond` · `to_musicxml` ·
`to_midi` · `to_abc` · `to_humdrum` ← `Score`. ML representations (`to_note_array`, `to_piano_roll`,
`to_event_sequence`, `compute_metrics`) operate on a `MusicDocument`
(`score.to_music_document()`).

---

## MusicXML

### Import (MusicXML → IR) — `src/adapters/mxml_to_ir.rs`

| Feature | Status | Notes |
|---|---|---|
| Notes with pitch | ✅ | Step, alter, octave |
| Rests (regular, measure) | ✅ | Display-step/octave preserved |
| Chords | ✅ | `<chord/>` grouping |
| Durations (standard, dotted) | ✅ | Division-based calculation |
| Tuplets / time-modification | ✅ | Actual/normal notes, tuplet display |
| Key signatures | ✅ | Fifths + mode (Major, Minor, church modes) |
| Time signatures | ✅ | Beats, beat-type, compound, symbol |
| Clef changes | ✅ | G, F, C, Percussion, Tab with octave-change |
| Transpose | ✅ | Diatonic, chromatic, octave-change |
| Articulations | ✅ | staccato, tenuto, accent, marcato, etc. |
| Ornaments | ✅ | trill, mordent, turn, tremolo, wavy-line |
| Dynamics | ✅ | ppp through fff, sfz, fp, sf |
| Wedges (hairpins) | ✅ | Crescendo, diminuendo, stop |
| Slurs | ✅ | Start/stop with number, placement |
| Ties | ✅ | Start/stop (sound + notation level) |
| Beams | ✅ | Begin, continue, end, hooks |
| Grace notes | ✅ | Regular, slash (acciaccatura), steal-time |
| Cue notes | ✅ | `<cue>` element |
| Lyrics | ✅ | Syllabic, text, extend, elision |
| Tempo (metronome + sound) | ✅ | Beat-unit, BPM, text label |
| Text directions (words) | ✅ | With font-style, font-weight |
| Rehearsal marks | ✅ | Text content |
| Octave shifts (ottava) | ✅ | Up, down, stop; size |
| Pedal markings | ✅ | Start, stop, change; line attribute |
| Coda / Segno marks | ✅ | |
| Da Capo / Dal Segno | ✅ | Via `<sound>` attributes |
| Barlines | ✅ | Regular, double, final, dashed, dotted, tick, short |
| Repeats | ✅ | Forward, backward |
| Volta endings | ✅ | Number, type |
| Multi-voice | ✅ | Voice number mapping |
| Multi-staff | ✅ | Staff number, `<staves>` |
| Part groups | ✅ | Group-name, group-symbol, number |
| Harmony / chord symbols | ✅ | Root, kind, bass, degrees |
| Figured bass | ✅ | Figures, parentheses |
| Page layout / defaults | ✅ | Page dimensions, margins, staff size |
| Glissando | ✅ | Start/stop, line-type |
| Slide (portamento) | ✅ | Start/stop |
| Arpeggio / non-arpeggiate | ✅ | Direction (up/down) |
| Notehead | ✅ | Custom notehead types |
| Stem direction | ✅ | Up/down |
| Print-object | ✅ | On notes |
| Measure width | ✅ | Width hint |
| MXL archives | ✅ | ZIP extraction via container.xml |
| MIDI instrument metadata | ✅ | Channel, program, name from `<score-part>` |
| Subtitle | ✅ | Via `<credit>` elements |
| Layout breaks | ✅ | Page, system breaks via `<print>` |
| Multi-measure rests | ✅ | `<multiple-rest>` |
| Technical indications | ✅ | Fingering, string number, etc. |
| Accidental display | ✅ | Cautionary, editorial, forced |
| Fermata | ✅ | Normal, angled, square; inverted |
| `<measure-style>` | 🔲 | Multi-rest, slash notation |
| `<dashes>` / `<bracket>` | 🔲 | Spanners |
| Non-traditional keys | 🔲 | Key-step / key-alter |
| Harmony inversion/frame | 🔲 | Extended chord symbols |

### Export (IR → MusicXML) — `src/adapters/ir_to_mxml.rs`

| Feature | Status | Notes |
|---|---|---|
| Notes with pitch | ✅ | Step, alter, octave |
| Rests (regular, measure) | ✅ | Display-step/octave, measure="yes" |
| Chords | ✅ | `<chord/>` grouping |
| Durations (standard, dotted) | ✅ | Auto-computed divisions |
| Tuplets / time-modification | ✅ | Actual/normal, tuplet display |
| Key signatures | ✅ | Fifths + mode |
| Time signatures | ✅ | Beats (compound), beat-type, symbol |
| Clef changes | ✅ | All clef types, octave-change |
| Transpose | ✅ | Diatonic, chromatic, octave-change |
| Articulations | ✅ | All standard types |
| Ornaments | ✅ | Including tremolo with marks count |
| Dynamics | ✅ | Both direction-level and note-level |
| Wedges (hairpins) | ✅ | Crescendo, diminuendo, stop |
| Slurs | ✅ | Start/stop with number |
| Ties | ✅ | Sound + notation level |
| Beams | ✅ | All beam types |
| Grace notes | ✅ | Regular, slash="yes", steal-time-previous |
| Cue notes | ✅ | `<cue>` element |
| Lyrics | ✅ | Syllabic, text, extend, elision |
| Tempo (metronome + sound) | ✅ | Text as `<words>`, metronome, `<sound tempo>` |
| Text directions (words) | ✅ | With font-style, font-weight |
| Rehearsal marks | ✅ | |
| Octave shifts (ottava) | ✅ | |
| Pedal markings | ✅ | With line attribute |
| Coda / Segno marks | ✅ | |
| Da Capo / Dal Segno | ✅ | Merged `<sound>` element |
| Barlines | ✅ | All styles |
| Repeats | ✅ | Forward, backward |
| Volta endings | ✅ | Number, type |
| Multi-voice | ✅ | Backup between voices |
| Multi-staff | ✅ | Staff number on notes |
| Part groups | ✅ | Group-name, group-symbol, number preserved |
| Harmony / chord symbols | ✅ | Root, kind, bass, degrees, offset |
| Figured bass | ✅ | Figures, parentheses, duration |
| Page layout / defaults | ✅ | Scaling, page-layout, system-layout |
| Glissando | ✅ | Start/stop, line-type |
| Slide (portamento) | ✅ | Start/stop |
| Arpeggio / non-arpeggiate | ✅ | Direction (up/down) |
| Notehead | ✅ | Custom notehead types |
| Stem direction | ✅ | |
| Print-object on notes | ✅ | |
| Measure width | ✅ | |
| MIDI instrument in score-part | ✅ | Score-instrument + midi-instrument |
| Subtitle as credit | ✅ | `<credit>` with credit-type |
| Extra creator metadata | ✅ | Arbitrary creator types |
| Layout breaks | ✅ | `<print>` with new-page/new-system |
| Multi-measure rests | ✅ | `<multiple-rest>` in measure-style |
| Technical indications | ✅ | |
| Accidental display | ✅ | Cautionary, editorial, forced |
| Fermata | ✅ | Shape, inverted |
| Instrument changes | ✅ | Mid-part `<sound>` with midi-instrument |
| Staff lines (non-5) | ✅ | staff-details |

---

## LilyPond

### Import (LilyPond → IR) — `src/adapters/ly_to_ir.rs`

| Feature | Status | Notes |
|---|---|---|
| Notes with pitch | ✅ | All 12 LilyPond pitch languages, every spelling LilyPond accepts |
| Rests (r, R, s) | ✅ | Regular, measure, spacer |
| Chords (`< >`) | ✅ | |
| Durations (standard, dotted) | ✅ | LilyPond numeric durations |
| Tuplets | ✅ | `\tuplet`, `\times` |
| Key signatures | ✅ | `\key` with 9 modes |
| Time signatures | ✅ | `\time` |
| Clef changes | ✅ | All standard clefs + octave variants |
| Articulations | ✅ | `-. -- -> -^ -! -_` etc. |
| Ornaments | ✅ | `\trill \mordent \prall \turn \reverseturn` |
| Dynamics | ✅ | `\ppp` through `\fff` |
| Wedges (hairpins) | ✅ | `\< \> \!` |
| Slurs | ✅ | `( )` grouping |
| Ties | ✅ | `~` |
| Grace notes | ✅ | `\grace \appoggiatura \acciaccatura \afterGrace` |
| Tempo | ✅ | `\tempo` with text/BPM/both |
| Rehearsal marks | ✅ | `\mark` |
| Coda / Segno | ✅ | Via `\mark \markup` with glyph |
| Da Capo / Dal Segno | ✅ | Via `\mark` text |
| Barlines | ✅ | `\bar` types |
| Repeats | ✅ | `\repeat volta N` |
| Multi-voice | ✅ | `<< \\\\ >>` syntax |
| Multi-staff | ✅ | `\new Staff`, `\new PianoStaff` |
| Variables | ✅ | Definition + resolution |
| `\include` | ✅ | File inlining |
| `\transpose` | ✅ | Transposing instrument context |
| Relative mode | ✅ | `\relative` pitch context |
| Glissando | ✅ | `\glissando` with style override |
| Arpeggio | ✅ | `\arpeggio` with direction |
| Slide | ✅ | Via glissando trill style |
| Anacrusis | ✅ | `\partial` |
| Paper block | ✅ | `\paper { }` → page layout |
| `\set Staff.instrumentName` | ✅ | Part name from `\set` property |
| `\set Staff.midiInstrument` | ✅ | MIDI instrument from `\set` property |
| `\context Voice = "name"` | ✅ | Named voices for lyrics attachment |
| Lyrics | ✅ | `\lyricsto`, `\lyricmode`, `\context Lyrics`, `\addlyrics` |
| Staff variables | ✅ | `staffX = \new Staff { ... }` with full part metadata |
| `\cadenzaOn/Off` | ✅ | Free-time span collapses to one `senza_misura` measure (both hands, score-wide) |
| `\melisma/End` | ✅ | Gracefully skipped |
| `\autoBeamOff/On` | ✅ | Gracefully skipped |
| `\dynamicUp/Down` | ✅ | Gracefully skipped |
| Harmony/chord names | ✅ | `\chordmode` (root, quality, bass; language-aware) |
| Figured bass | ✅ | `\figuremode` (figures, accidentals incl. natural & double) |

### Export (IR → LilyPond) — `src/adapters/ir_to_ly.rs`

| Feature | Status | Notes |
|---|---|---|
| Notes with pitch | ✅ | All 12 languages, in the spelling LilyPond uses; relative/absolute |
| Rests (r, R, s) | ✅ | |
| Chords (`< >`) | ✅ | With arpeggio support |
| Durations (standard, dotted) | ✅ | |
| Tuplets | ✅ | `\tuplet actual/normal` |
| Key signatures | ✅ | |
| Time signatures | ✅ | |
| Clef changes | ✅ | All types with octave shifts |
| Articulations | ✅ | All standard LilyPond articulations |
| Ornaments | ✅ | `\trill \mordent \prall \turn` etc. |
| Dynamics | ✅ | Note-attached |
| Wedges (hairpins) | ✅ | `\< \> \!` |
| Slurs | ✅ | `( )` |
| Ties | ✅ | `~` |
| Grace notes | ✅ | `\acciaccatura \appoggiatura \afterGrace` |
| Tempo | ✅ | `\tempo` with text/BPM |
| Text directions | ✅ | `\markup` |
| Rehearsal marks | ✅ | `\mark` |
| Coda / Segno | ✅ | Glyph markup |
| Da Capo / Dal Segno | ✅ | Text mark |
| Barlines | ✅ | `\bar` types |
| Repeats | ✅ | `\repeat volta` |
| Volta endings | ✅ | `\alternative` |
| Multi-voice | ✅ | `<< \\\\ >>` |
| Multi-staff | ✅ | Staff groups, PianoStaff |
| Part names | ✅ | `instrumentName` |
| Glissando | ✅ | With style override |
| Arpeggio | ✅ | Direction commands |
| Slide | ✅ | |
| Anacrusis | ✅ | `\partial` |
| Paper block | ✅ | Page dimensions, margins |
| Header block | ✅ | Title, composer, etc. |
| Harmony (ChordNames) | ✅ | Separate `\chordmode` variable |
| Figured bass | ✅ | Separate `\figuremode` variable |
| MIDI instrument | ✅ | `\set Staff.midiInstrument` |
| Score block | ✅ | `\layout { } \midi { }` |
| Fermata | ✅ | `\fermata` |
| Octave shifts | ✅ | `\ottava` |
| Pedal | ✅ | `\sustainOn \sustainOff` |
| Lyrics | ✅ | Score path: `\new Lyrics \lyricsto`; Music path: `\addlyrics` |

---

## MIDI

### Import (MIDI → IR) — `src/adapters/midi_to_ir.rs`

MIDI support is always compiled (the `midi` feature gate was removed in Epic 4).

| Feature | Status | Notes |
|---|---|---|
| Notes with pitch | ✅ | Note-on/note-off pairs |
| Rests | ✅ | Gaps between notes |
| Chords | ✅ | Simultaneous notes on same channel |
| Duration quantization | ✅ | Snap to nearest standard duration |
| Dotted durations | ✅ | In quantization candidates |
| Triplets | ✅ | 3:2 ratio in quantization |
| Key signatures | ✅ | MIDI meta events |
| Time signatures | ✅ | MIDI meta events (default 4/4) |
| Clef | ✅ | Default treble assigned |
| Tempo | ✅ | Tempo meta events → TempoDirection |
| Multi-track (Format 1) | ✅ | One part per track |
| Channel splitting (Format 0) | ✅ | Channels → parts |
| Program changes | ✅ | → `Part.midi_program` |
| Dynamics / velocity | ✅ | Velocity quantized to nearest dynamic; mark emitted on band change |
| Articulations | 🔲 | Not preserved (MIDI lossy) |
| Slurs / ties | 🔲 | Not preserved |
| Grace notes | 🔲 | Not preserved |
| Lyrics | 🔲 | Not preserved |
| Repeats | 🔲 | Must be pre-expanded |

### Export (IR → MIDI) — `src/adapters/ir_to_midi.rs`

MIDI support is always compiled (the `midi` feature gate was removed in Epic 4).

| Feature | Status | Notes |
|---|---|---|
| Notes | ✅ | Note-on/note-off pairs |
| Rests | ✅ | Represented as gaps (no events) |
| Chords | ✅ | All notes on/off simultaneously |
| Durations | ✅ | Converted to ticks |
| Tuplet durations | ✅ | Ratio preserved as ticks |
| Key signatures | ✅ | MIDI meta events |
| Time signatures | ✅ | MIDI meta events |
| Tempo | ✅ | MIDI meta tempo events (default 120 BPM) |
| Grace notes | ✅ | Short-duration notes before main |
| Multi-part (Format 1) | ✅ | Conductor track + one per part |
| Program changes | ✅ | From `Part.midi_program` |
| Channel assignment | ✅ | From `Part.midi_channel` |
| Configurable TPQ | ✅ | Default 480 ticks/quarter |
| Dynamics | ✅ | Dynamic marks → MIDI velocity ladder; running velocity persists |
| Articulations | 🔲 | Not emitted |
| Slurs / ties | 🔲 | Not emitted |
| Repeats | 🔲 | Must be expanded before export |

---

## ABC

A core subset of ABC notation. Conversion goes through the Layer-1 Music tree
(`ToMusicAdapter` / `FromMusicAdapter`), so the Python `to_abc(score)` lifts the
score internally.

### Import (ABC → IR) — `src/adapters/abc_to_ir.rs`

| Feature | Status | Notes |
|---|---|---|
| Tune headers | ✅ | `X` `T` `C` `M` `L` `K` `Q` |
| Notes with pitch | ✅ | Octave marks (`,` / `'`), explicit accidentals |
| Default unit length | ✅ | `L:` rule; inferred from `M:` when absent |
| Durations (fractional) | ✅ | `a2`, `a/2`, `a3/2` |
| Rests | ✅ | `z`, `x` |
| Chords | ✅ | `[CEG]` |
| Ties | ✅ | `-` |
| Bar lines + repeats | ✅ | `|`, `||`, `|:`, `:|` |
| Key signatures | ✅ | Tonic + mode → fifths (incl. church modes) |
| Time signatures | ✅ | `M:` (incl. `C`/`C|`) |
| Tempo | ✅ | `Q:` |
| Multi-voice (`V:`) | ✅ | ABC 2.1 §4.1 — header/body `V:id`, inline `[V:id]`, `name=`; each voice → a Part |
| MIDI instrument | 🔲 | ABC has **no standard** instrument field — the `%%MIDI program N` directive is a non-standard `abc2midi` stylesheet extension, so it is **not** parsed (see Export note) |
| Tuplets | ✅ | `(p`, `(p:q`, `(p:q:r`; bare `(p` uses the ABC default ratios |
| Grace notes | ✅ | `{ab}`, `{/a}` (acciaccatura) |
| Chord symbols `"…"` | 🔲 | Skipped gracefully |
| Decorations / inline fields | 🔲 | Skipped gracefully |

### Export (IR → ABC) — `src/adapters/ir_to_abc.rs`

| Feature | Status | Notes |
|---|---|---|
| Tune headers | ✅ | `X` `T` `C` `M` `L` `K` |
| Notes with pitch | ✅ | Body emitted at `L:1/8` |
| Durations | ✅ | Relative to the unit length |
| Rests | ✅ | |
| Chords | ✅ | `[…]` |
| Ties | ✅ | |
| Bar lines + repeats | ✅ | Regular bar lines are derived from the running meter (the IR only stores *explicit* barlines), and the body wraps every 4 bars |
| Tuplets | ✅ | `(p:q:r`, one group per `p` notes so a run never crosses a bar |
| Grace notes | ✅ | `{…}` / `{/…}`; carry no metrical time |
| Key / meter | ✅ | |
| Multi-voice (`V:`) | ✅ | ≥2 parts/staves emit `V:n name="…"` blocks (ABC 2.1 §4.1); polyphony is lossless |
| MIDI instrument | 🔲 | **Deliberately not emitted.** ABC has no standard instrument field; the only convention, `%%MIDI program N`, is a non-standard `abc2midi` directive, not part of the ABC 2.1 standard. Emitting it would produce output other ABC tools ignore or reject, so instrument identity is dropped on `→ ABC` (a format limitation, not a bug). It is preserved across LilyPond ↔ MusicXML ↔ MIDI. |
| Key-aware accidental re-spelling | 🔲 | v1 carries only explicit accidentals (self-consistent on round-trip) |

---

## Humdrum (`**kern`)

Core `**kern` in both directions, through the Layer-1 Music tree. One spine per
part/voice, with `.` padding on the time slices a spine does not sound.

### Import (kern → IR) — `src/adapters/humdrum_to_ir.rs`

| Feature | Status | Notes |
|---|---|---|
| Pitch + octave | ✅ | `c`/`cc`/`C`/`CC`, `#`/`-`/`n` accidentals |
| Durations (recip) | ✅ | `4`, `2.`, `12` (tuplets folded in), `0`/`00` (breve/longa), `N%M` |
| Rests | ✅ | `r` |
| Chords | ✅ | Space-separated subtokens |
| Ties / slurs | ✅ | `[`, `_`, `]` and `(`/`)` |
| Grace notes | ✅ | `q` (acciaccatura) / `Q` |
| Barlines + repeats | ✅ | `=`, `=||`, `=:|!|:` |
| Key / time / clef | ✅ | `*k[…]`, `*M4/4`, `*clefG2` |
| Instrument name | ✅ | `*I"…` |
| Reference records | ✅ | `!!!OTL`, `!!!COM` → title / composer |
| Fermata | ✅ | `;` |
| Spine rearrangement | 🔲 | `*^` / `*v` raise a clear error rather than mis-parsing |

### Export (IR → kern) — `src/adapters/ir_to_humdrum.rs`

| Feature | Status | Notes |
|---|---|---|
| Pitch / duration / rests / chords | ✅ | |
| Tuplets | ✅ | Ratio folded into the recip (`12` = triplet eighth) |
| Grace notes | ✅ | Each gets its own data record, `.` in the other spines |
| Ties / slurs / fermata | ✅ | |
| Barlines + repeats | ✅ | |
| Key / time / clef / instrument | ✅ | Tandem interpretations |
| Multi-voice | ✅ | One spine per voice |

## MEI (planned)

Not yet implemented.
