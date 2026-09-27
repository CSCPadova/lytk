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
| MIDI | `.mid` `.midi` | ✅ | ✅ | Export plays like LilyPond's MIDI; import is being rebuilt (see below) |
| ABC | `.abc` | ✅ | ✅ | ABC 2.1 pitches and rhythm; repeats, lyrics, decorations in progress |
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

### Import (MIDI → IR) — `src/adapters/midi_to_ir/`

Rebuilds notation from MIDI written by notation programs (LilyPond, MuseScore,
lytk), following MuseScore's import pipeline, simplified: quantize positions,
find tuplets and grace notes, group chords, separate voices, then bar
everything with the crate's one bar-splitter. Performed (played-in) MIDI —
most onsets off every grid — is read by its own path: beats tracked where the
playing drifts, onsets placed by a Viterbi search that weighs distance
against where the meter expects a note of that length, hands split, swing
straightened.

| Feature | Status | Notes |
|---|---|---|
| Notes with pitch | ✅ | Note-on/note-off pairs; overlapping same-key notes pair first-in first-out; spelled in the key in force |
| Timing | ✅ | Onsets and ends snap to one grid per beat: plain (to 64ths) or tuplet (3, 5, 6, 7, 10, 12 a beat), whichever fits exactly; nothing drifts |
| Durations, ties | ✅ | Written values from the positions; notes across bar lines tied |
| Tuplets | ✅ | Triplets, quintuplets, septuplets (and 16th/32nd kinds), bracketed a beat at a time |
| Rests | ✅ | Only where a voice is silent; legato gaps (a third of the note or less) are closed |
| Staccato | ✅ | Notation files: as LilyPond and lytk play it (half length, 4 louder than the notes around a run). Played files: a note lengthened by 30 % or more to one written value (to the next onset or the beat after its sound), unless the pedal holds it |
| Grace notes | ✅ | LilyPond's and lytk's (9/40 of their value, before the beat) |
| Chords, voices | ✅ | Notes starting and ending together are a chord; up to 4 voices a staff, nearest in pitch, numbered from the top |
| Time signatures | ✅ | At their positions; bars re-anchor at each; a first time signature shorter than the next is a pickup |
| Key signatures | ✅ | At their positions, in every part |
| Clef | ✅ | Treble or bass from the staff's range; percussion clef on channel 10 |
| Parts | ✅ | One per track (per channel when a track mixes them); a keyboard's two tracks (LilyPond's `upper:`/`lower:`, lytk's `Name 1`/`Name 2`, "RH"/"LH") are one two-staff part |
| Program changes | ✅ | → `Part.midi_program` and the GM instrument name |
| Dynamics | ✅ | Each note keeps its velocity (`Note.velocity`); a dynamic mark where a part's level changes (LilyPond's table for files LilyPond or lytk wrote) |
| Tempo | ✅ | Tempo events at their positions, whole beats a minute where a writer truncated them |
| Pedal, lyrics | ✅ | CC64 → pedal marks (start, stop, change); lyric events → lyrics on the top note starting there |
| Format 2 | 🔲 | Refused with a clear error |
| Played MIDI | ✅ | Onsets placed by a Viterbi search: distance plus a cost for points weaker in the meter than the note's length expects, plain (to 32nds, or `quantize=`) or triplet beats (two onsets off the 16th grid), a rolled chord as one. Beats tracked when the playing drifts from the file's tempo (rubato, a late start). A one-track piano splits into hands by MuseScore's cost model. Swung eighths (3:2 to 2:1) straightened and marked "Swing" (`swing=`; a 3:1 shuffle reads dotted unless asked). A file without a key signature gets one (Krumhansl–Kessler profiles); karaoke text events are lyrics. A performance far from the file's tempo from the start is not re-timed, and played quarter-note triplets read as syncopations |

### Export (IR → MIDI) — `src/adapters/ir_to_midi.rs`

Plays the score as LilyPond's MIDI performers do, so `ly → MIDI` matches
LilyPond's own MIDI of the test pieces note for note (onsets, pitches, and
99.9 % of note-offs).

| Feature | Status | Notes |
|---|---|---|
| Timing | ✅ | 384 ticks a quarter (`with_divisions`); exact positions, so tuplets don't drift; a bar lasts as long as its music |
| Pickups, irregular bars | ✅ | Written as a time signature of the bar's length, then the meter (MuseScore's convention; LilyPond can't say `\partial` in MIDI) |
| Repeats | ✅ | Played out with their endings (MuseScore's rules); `to_midi(…, unfold_repeats=False)` keeps the written order. D.C./D.S. jumps are not followed yet |
| Ties | ✅ | A tied chain sounds once, chords note by note |
| Grace notes | ✅ | Before the beat, 9/40 of their written length, cutting the note before (LilyPond) |
| Dynamics | ✅ | LilyPond's table (p 69, mf 86, f 95; 90 without a dynamic) and instrument equalizer; hairpins ramp to the next dynamic; MusicXML direction dynamics count |
| Per-note velocity | ✅ | `Note.velocity` (from MusicXML `<note dynamics>`; MIDI import sets it in phase C) wins |
| Articulations | ✅ | Staccato, staccatissimo, portato shorten; accent, marcato add velocity (`ly/script-init.ly`) |
| Unisons | ✅ | Two voices on one key play it once (LilyPond's MIDI walker) |
| Tracks and channels | ✅ | Conductor track + one track per staff; one channel per part; percussion on channel 10; past 15 parts, channels are shared by program |
| Sustain pedal | ✅ | CC64 from pedal directions |
| Lyrics | ✅ | Lyric events (first verse) |
| Transposing instruments | ✅ | `<transpose>` applied: sounding pitch |
| Tempo, key, time | ✅ | Conductor track; tempo changes at their position (120 BPM if none) |
| Slurs, ornaments, fermatas | 🔲 | Not performed |

---

## ABC

ABC 2.1. Conversion goes through the Layer-1 Music tree (`ToMusicAdapter` /
`FromMusicAdapter`), so the Python `to_abc(score)` lifts the score internally.
The reader and the writer are checked against an independent ABC 2.1 player
(`tests/abc_standard.rs`), not only against each other.

### Import (ABC → IR) — `src/adapters/abc_to_ir.rs`

| Feature | Status | Notes |
|---|---|---|
| Tune | ✅ | The first tune of a file (`from_abc_tunes` reads them all, with the file header before the first `X:` applied to each); an empty line ends a tune |
| Headers | ✅ | `X` `T` `C` `M` `L` `K` `V`; `Q` kept as metadata only |
| Key signatures | ✅ | Tonic, all modes, explicit accidentals (`K:D Phr ^f`), `exp`, `none`, `HP`/`Hp`, applied to every unmarked note |
| Accidentals | ✅ | Last to the end of the bar, in the same octave by default (abcm2ps, abc2svg); `%%propagate-accidentals` / `I:propagate-accidentals` change it; a tied note keeps its accidental over the bar line |
| Clefs, octave | ✅ | `clef=` (and `-8`/`+8`, which transpose), `octave=` on `K:` and `V:` |
| Inline fields | ✅ | `[K:]`, `[M:]`, `[L:]`, and `[V:]` anywhere in a line |
| Meters | ✅ | `C`, `C|`, additive `2+3+2/8` and `(2+3+2)/8`; `M:`/`L:` in the body apply to their voice only |
| Durations | ✅ | `a2`, `a/2`, `a3/2`; broken rhythm `>`, `<`, `>>`; default unit from the header `M:` |
| Rests | ✅ | `z`; `x` as an invisible skip; `Z`, `Z4` whole-bar rests, `X` invisible ones |
| Chords | ✅ | `[CEG]2`; length of the first note (`[C2E2G2]`); ties on single notes (`[C-E]`) or all (`[CE]-`) |
| Ties, tuplets, grace notes | ✅ | `-`; `(p`, `(p:q`, `(p:q:r` (`(5` in 6/8 is 5 in the time of 3); `{ab}`, `{/a}` |
| Multi-voice (`V:`) | ✅ | Header/body `V:id`, inline `[V:id]`, `name=`; each voice → a Part |
| Overlays (`&`) | 🟡 | Each layer is a voice of its own for that bar; multi-bar `(&`…`&)` not read |
| Bar lines + repeats | ✅ | `|`, `||`, `|]`, `|:`, `:|`, `::`, endings `[1`, `|1`, `:|2`. A bar line always ends a bar: a short first bar is the pickup, and any other bar keeps its length |
| Decorations | ✅ | `!p!`…`!ffff!`, `!sfz!`; hairpins `!<(!`/`!<)!`/`!>(!`/`!>)!` (and `!crescendo(!`…); `.` `!>!` `!tenuto!` `!wedge!` `!breath!`; `T` `M` `P` `~` and their `!…!` names; `H`/`!fermata!`. Bowings, segno and coda are skipped |
| Chord symbols, annotations | ✅ | `"Am7"`, `"F#m7b5"`, `"G/B"` → chord symbols; `"^text"`/`"_text"` (and `<`, `>`, `@`) → words |
| Slurs, tempo | ✅ | `(`…`)`, nested; `Q:1/4=120`, `Q:"Allegro" 3/8=80`, old `Q:120` |
| Lyrics | ✅ | `w:` under the notes since the last `w:` (`-`, `_`, `*`, `~`, `\-`, `|`); consecutive `w:` lines are verses; `W:` kept as metadata |
| MIDI instrument | 🔲 | ABC has **no standard** instrument field — `%%MIDI program N` is a non-standard `abc2midi` extension and is not parsed |

### Export (IR → ABC) — `src/adapters/ir_to_abc.rs`

| Feature | Status | Notes |
|---|---|---|
| Headers | ✅ | `X` `T` `C` `M` `L:1/8` `K`; every key named (`K:G#m`); keys past 7 sharps/flats as `K:C` with explicit accidentals |
| Accidentals | ✅ | The score's own spelling; an accidental wherever a reader under either ABC rule would otherwise sound something else (after a mid-bar key change, every note states its own) |
| Durations, rests, chords, ties | ✅ | Chord ties on all notes (`[CE]2-`) or some (`[C-E]2`) |
| Tuplets, grace notes | ✅ | `(p:q:r`; `{…}` / `{/…}` |
| Multi-voice (`V:`) | ✅ | One `V:` block per part/staff; a voice's own `K:`/`M:` when it differs from the header |
| Bar lines + repeats | ✅ | Derived from the meter plus explicit bar lines; `|:`/`:|` with `[1`/`[2` endings; a pickup as a short first bar; an irregular bar keeps its length; a note across a bar line is tied over it |
| Inner voices | ✅ | A bar's voices as `&` layers (ABC 2.1 §7.4); voices running across bar lines as the richest one |
| Spacers | ✅ | `x` |
| Decorations, slurs | ✅ | Dynamics, hairpins, articulations, ornaments, fermatas, slurs; a direction's dynamic or hairpin goes on the next note |
| Chord symbols, words, tempo | ✅ | `"Am7"`; `"^dolce"`; `Q:` in the header, `[Q:]` inside |
| Lyrics | ✅ | A `w:` line under each music line, per verse (`*` under a note without a syllable, `_` while one is held) |
| MIDI instrument | 🔲 | **Deliberately not emitted.** ABC has no standard instrument field; `%%MIDI program N` is a non-standard `abc2midi` directive, so instrument identity is dropped on `→ ABC` (a format limit, not a bug). It is kept across LilyPond ↔ MusicXML ↔ MIDI. |

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
| Barlines + repeats | 🟡 | `=N`, `==`; repeats as `:|!`, `!|:`; endings not written (kern needs `*>` expansion lists) |
| Key / time / clef / instrument | ✅ | Tandem interpretations |
| Multi-voice | ✅ | One spine per voice |

## MEI (planned)

Not yet implemented.
