# Changelog

All notable changes to lytk are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and lytk uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Dated
engineering notes are in the [development log](devlog.md).

## [Unreleased]

### Added

- LilyPond `\version`: `lytk.lilypond_version(text)` reads the version the
  first `\version` statement states, from the parse tree (a commented-out
  one does not count), as a `lytk.LilyPondVersion`, which compares
  numerically (`2.24` equals `2.24.0`) and accepts what LilyPond 2.24
  accepts. `lytk.set_lilypond_version(text, version)` and
  `lytk.strip_lilypond_version(text)` edit the statements.
  `Score.lilypond_version` and `MusicDocument.lilypond_version` give the
  version a LilyPond source stated. `to_lilypond(…, version=)` and
  `to_lilypond_music(…, version=)` write another `\version` than 2.24.0.
  An invalid `\version` is an `invalid-version` error. Rust:
  `ly_to_ir::{LilyPondVersion, lilypond_version, set_lilypond_version,
  strip_lilypond_version}` and `ScoreMetadata::lilypond_version`.

## [0.3.0] - 2026-09-27

LilyPond input you can trust. Readers raise `lytk.ParseError` instead of
crashing, `lytk.check_lilypond` and `lytk check` report diagnostics, and
`strict=True` rejects what LilyPond rejects. Strings and headers are read
as LilyPond reads them, pitch-language files are honoured, and top-level
music, `\book`, drum mode and `\fixed` follow LilyPond. The inputs that made
lytk panic, hang or run out of memory, found by fuzzing and a hunt, are
fixed, and all of LilyPond's 2,626 regression tests and snippets are read.

### Added

- `lytk.__version__`, the crate's version (`lytk --version` prints it).

- `lytk.LytkError`, the base of the errors lytk raises, and
  `lytk.InternalError`: a Rust panic inside any function or method that
  reads, writes or transforms music now raises `InternalError` (an ordinary
  `Exception`, carrying the panic's message and source location) and prints
  nothing. It used to escape as pyo3's `PanicException`, a `BaseException`
  that `except Exception` does not catch, with its message on stderr.
- `lytk.ParseError`, raised by every reader (`from_*`, `Score.from_json`,
  `Score.from_dict`, `MusicDocument.from_json`, `flatten`) when its input
  cannot be read. It subclasses both `LytkError` and `ValueError`, so
  `except ValueError` keeps working. I/O failures stay `OSError`.
- Diagnostics for LilyPond: `lytk.check_lilypond(text, *, semantic=False)`
  returns `lytk.Diagnostic` objects (severity, code, message, line, column
  and a character span), from the parse tree alone by default (about 0.1 ms
  per file), from a full reading with `semantic=True`. Errors are input
  LilyPond rejects: syntax errors, unclosed brackets, a `>>` without its
  `<<`, what is left of a Scheme expression missing its `(`, `c3`, a tuplet
  or multiplier with a zero term, plain text. Warnings are input lytk reads
  around: unknown commands, includes, unknown languages, dropped music,
  `\midi`-only scores, values it cannot represent. The codes are listed in
  `docs/import-export.md`.
- `strict=False` on every LilyPond reader: `strict=True` raises
  `lytk.LilyPondSyntaxError` (a `ParseError`) when the reading has an error,
  with every diagnostic in its `diagnostics` attribute.
  `Score.diagnostics` and `MusicDocument.diagnostics` hold them either way
  (they are not part of `to_dict`/`to_json`).
- `lytk check FILE… [--semantic] [--json]`: the diagnostics of LilyPond
  files; exit status 1 on an error.
- Rust: `diagnostics::{Diagnostic, Severity}`; `LyToIrAdapter::read_str` and
  `read_file` return a `LyReading` (every movement and the diagnostics);
  `ly_to_ir::check` and `ly_to_ir::read_source`.
- `Score.header` and `MusicDocument.header`: every header field as a dict
  (`title`, `subtitle`, `composer`, `arranger`, `lyricist`, then the others by
  key), and `MusicDocument.lyricist`.
- `lytk.header_fields(text)`: every `\header` field of LilyPond text as a
  `HeaderField` (key, value as text, character span of the whole `key = value`,
  and the `\score` block it is in), from the parse tree alone, so a field can
  be read and cut without reading the music. Rust: `ly_to_ir::header_fields`.
- LilyPond drum mode: `\drums { bd4 sn }`, `\drummode`, `\new DrumStaff`
  and `\new DrumVoice` read LilyPond's 128 drum names as their General MIDI
  keys (`bd` is key 36) on a percussion staff, which MIDI export puts on
  channel 10. A word that is no drum name is `unrecognized-token`.
- LilyPond `\fixed c' { … }`: absolute pitches an octave up per mark of the
  reference pitch, also inside `\relative` and in a variable.
- LilyPond `\book` and `\bookpart`: their scores and music are movements,
  and their `\header` is the book's.

### Changed

- Each top-level LilyPond music expression is a movement, as LilyPond makes
  a score of each: `{ c'1 } { d'1 }` is two movements, and music beside
  `\score` blocks takes its place among them. It was dropped
  (`dropped-music`), or merged into one movement. A music variable used at
  the top level (`\m`) is read; it was dropped. Notes outside braces at the
  top level (`c'4 d'`) are a `syntax-error`, as in LilyPond; they were a
  warning.
- The shorthands `\chords { … }`, `\figures { … }` and `\lyrics { … }` are
  chord names, figured bass and lyrics; their contents were read as notes.

- The version has one source, `Cargo.toml` (`pyproject.toml` declares it
  dynamic); the release workflow fails when the tag is not that version, and
  CI runs the Python tests on 3.10 to 3.13.
- Readers raise `lytk.ParseError` where they raised `ValueError` (a subclass,
  so existing handlers still catch it). A `.ly` file that is not UTF-8 is a
  `ParseError`; it was an `OSError`.
- LilyPond markup is text, not music: `c4 -\markup \bold a8 d4` has two notes,
  as LilyPond reads it (`a8` is the markup's word), and the words of a
  top-level `\markup`, of `\tempo \markup …` or of a `\set`/`\override`
  markup value no longer become notes. Top-level `\layout` and `\midi` blocks
  are no longer read as music either.
- A UTF-8 byte-order mark is whitespace anywhere in LilyPond input, as in
  LilyPond; the grammar reported one past the start as a syntax error.
- `\time 0/4` is ignored like `\time 1/0`, as LilyPond does.
- LilyPond strings are decoded as LilyPond reads them, in every context
  (headers, lyrics, `\tempo`, markup, `\with`, `\mark`, `\set`, context and
  voice names, `\language`): `\"`, `\\`, `\n`, `\t` and `\'`, any other
  backslash kept. A string used to end at its first escape:
  `texidoc = "Some \"doc\" here"` read as `Some `.
- A header value given as `\markup` is its plain text
  (`composer = \markup { \bold "J. S." Bach }` is `J. S. Bach`), and `#"…"`
  its string; both were dropped. Markup text at a note keeps its words
  (`c4^\markup { \italic dolce }`), not only its strings.
- Headers are scoped: a `\score`'s `\header` belongs to that movement only (it
  leaked into the next ones), and the top-level `\header` fills every
  movement's unset fields, wherever it stands.
- The LilyPond writer writes every header field (it wrote none unless a
  title, composer, arranger or poet was set), the others sorted by key and
  quoted when they are no identifier; the Music path writes the poet and the
  other fields too. Text directions at notes are written
  (`^\markup { "dolce" }`); they were lost. The Music path quotes lyrics as
  the Score path does.
- `\include "english.ly"` and LilyPond's other language files set the pitch
  language, as `\language` does (`arabic.ly`: Italian names); the files of
  pitch names lytk cannot read (`makam.ly`, `persian.ly`, `bagpipe.ly`, …)
  raise `unknown-language`. `\language` is honoured inside `\score` and
  music, not only at the top level, and an unknown name keeps the language in
  force instead of resetting it to Dutch.
- The LilyPond reader refuses input past its bounds with a `ParseError` (a
  `ValueError`: "… lytk refuses input this large") instead of hanging, running out of
  memory or crashing: more than 500,000 notes, rests and chords once repeats
  and variables are expanded; music longer than 100,000 whole notes or with
  100,000 bars; a duration multiplier above 16,777,216 (`R1*1000000000`);
  music nested deeper than 2,000 levels; a pitch outside octaves -128..127; a
  staff group of more than 255 staves. LilyPond's own 2,626 regression tests
  and documentation snippets are all read.
- Constructs whose numbers the IR cannot hold are dropped, and the music
  around them is read: a time signature with a 0 or 256+ denominator
  (`\time 3/256` used to become 3/0), a tuplet with a 0 term or one above
  255 (its notes are read unscaled), a measure length with a 0 denominator,
  a duration multiplier with a 0 or oversized denominator.
- A written duration that is not a power of two up to 1024 (`c3`, `c2048`)
  reads as a quarter note, as LilyPond reads its "not a duration".
- `\repeat unfold` inside `\relative` repeats the same pitches, as LilyPond
  does: each copy used to be read relative to the previous one, climbing.
- `Score.from_dict`, `Score.from_json` and `MusicDocument.from_json` refuse
  values no reader makes, with a `ValueError`: a 0 time-signature
  denominator or tuplet term, a negative or oversized duration, an octave
  outside -128..127, an alteration beyond ±4, time-signature beats above
  10,000.
- `transpose` takes at most ±127 semitones, `invert` an axis in octaves
  -128..127 with an alteration of at most ±4, and an interval number is
  1..99; other values are a `ValueError` (they overflowed in Rust).
- MusicXML export: when no `divisions` up to 65535 represents every duration
  exactly (several coprime tuplets, e.g. 3, 5, 7, 9, 11 and 13 in different
  voices), 10080 is used and other durations are rounded. The least common
  multiple used to overflow, or be truncated to 16 bits and write wrong
  durations.

### Fixed

- Inputs that panicked: `\time N/0`, `\time 3/256`, `\tuplet 0/N`,
  `\times N/0`, `#(ly:make-moment N 0)`, 256 or more dots on a figured-bass
  figure, a zero-length note (`c4*0`) in LilyPond export, 256 or more staves
  in a PianoStaff or in `MusicDocument.to_score()`, coprime durations whose
  positions overflowed, note arrays of long music.
- Inputs that hung or exhausted memory: `s1*N`, `R1*N` and `\skip 1*N` for
  a huge N, nested `\repeat unfold`, variables that double one another, a
  variable redefined in terms of itself and used inside `\relative`, a tiny
  `measureLength`, an over-long note (bar lines were laid to its end and
  thrown away), a relative passage climbing hundreds of octaves (every
  writer wrote each octave mark out), and nesting of `\tuplet`, `\relative`
  or `\repeat` a few hundred levels deep, which overflowed the stack. The
  reader now walks on a thread with a stack of its own.
- `\ottava` with a number past what a shift is (`#2147483647`) panicked; it
  is clamped to three octaves. `\ottava #2` read as size 16 instead of 15 (a
  15ma in MusicXML), and the Music-path LilyPond writer wrote an 8va as
  `\ottava #8` and an 8vb as an 8va.
- A `**kern` note with 256 or more dots panicked.
- Music longer than 100,000 whole notes from MusicXML, ABC or `**kern` (one
  MusicXML note of 25 million whole notes) was accepted, and the ABC writer
  tied it over every bar line until an allocation aborted the process. The
  three readers now refuse it, as the LilyPond and MIDI readers do, and the
  ABC writer refuses music of more than 100,000 bars.
- Time that grew with the square of the input: meter changes (reading and
  lowering; 100,000 notes with 40,000 meter changes took 24 s through every
  writer, now 5 s), long runs of tuplets in ABC output, long runs of grace
  notes in the Music-path LilyPond output, and many diagnostics on one long
  line.
- A tremolo `:N` past 1024 (`c1:2147483648`) is not read as one: it gave
  31 tremolo marks, and the LilyPond writer's shift overflowed. The writers
  cap the marks of hand-made IR at 10.
- `lytk.flatten` on a diamond of includes (a file including the next one
  twice, level after level) doubled its output at every level until memory
  ran out. It now stops with a `ValueError` after 10,000 includes or 64 MiB
  of output.
- A LilyPond context whose music follows a nested `\new` or a mode
  (`\new Staff \new Voice { … }` at the top level,
  `\new Staff \drummode { … }`, `\new Staff \fixed c' { … }`) left its
  music to be read as a separate expression.
- LilyPond part ids after a variable definition started at `P2`.
- The MusicXML writer panicked on figured bass at the beat of a grace note.

## [0.2.0] - 2026-09-27

MIDI and ABC conversion rebuilt and checked against independent references
instead of lytk's own writers: ABC against the ABC 2.1 standard, MIDI import
against the LilyPond sources of its test files (following MuseScore's import
pipeline), MIDI and LilyPond export against LilyPond itself. Performed
(played-in) MIDI is read too: beat tracking, a Viterbi onset search, the
hands of a one-track piano, swing and staccato.

### Added

- MIDI export writes the sustain pedal (CC64), lyric events and instrument
  names, and plays transposing instruments at sounding pitch (`<transpose>`).
- `Note.velocity`: the velocity a note was played with, read from and written
  to MusicXML's `<note dynamics>`, and used by MIDI export and note arrays.
- `to_midi(…, unfold_repeats=False)` and `to_midi_bytes(…, unfold_repeats=False)`
  keep repeats in written order.
- ABC reading:
  - broken rhythm (`A>B`, `A<B`, `A>>B`);
  - whole-bar rests `Z`, `Z4` and invisible ones `X`, and `x` as a skip;
  - `&` overlays, each layer a voice of its own;
  - `[V:]` anywhere in a line, and `L:`/`M:` per voice.
- ABC reading and writing carry dynamics, hairpins, articulations,
  ornaments, fermatas, slurs, chord symbols (`"Am7"`), text annotations
  (`"^dolce"`), tempo (`Q:`) and lyrics (`w:`, with verses). These used to
  be skipped on reading and missing on writing.
- Reading played MIDI:
  - Triplets are found in played files too.
  - A piano played on one track is split into two hands, by MuseScore's
    cost model (hand span, textures, a busy hand) plus how far a hand
    moves.
  - A file without a key signature gets the key its notes are in, and is
    spelled in it.
  - Karaoke files (`.kar`) have their words read as lyrics.
  - `from_midi(…, quantize=16)` and `from_midi_bytes(…, quantize=16)` set
    the shortest value played music is snapped to.
  - Played onsets are placed by a Viterbi search that weighs how far a
    point is against how strong it is in the meter: a quarter played 40
    ticks late stays on the beat, one sloppy beat in a triplet passage stays
    triplets, a rolled chord is one chord, 6/8 reads in eighths.
  - A performance that drifts from the file's tempo (rubato, a late start)
    is read on its tracked beats.
  - Swung eighths are read straight and marked "Swing"; `swing=True` or
    `False` in `from_midi` and `from_midi_bytes` forces it either way.
  - A played note lengthened by 30 % or more to one written value is a
    staccato (not under the sustain pedal).
- `from_abc_tunes()` reads every tune of an ABC file, and `lytk convert`
  writes each tune of one (`NAME_02.EXT`, …), as it does LilyPond movements.

### Changed

- MIDI export plays a score as LilyPond's MIDI does, and matches LilyPond's
  own MIDI of the test pieces note for note:
  - LilyPond's dynamics table and instrument equalizer (p 69, mf 86, f 95;
    90 without a dynamic, where it was 80);
  - hairpins that ramp to the next dynamic;
  - staccato, staccatissimo and portato lengths, and accent velocities;
  - grace notes just before the beat;
  - two voices sounding one key play it once.
- MIDI export plays repeats out with their endings.
- A pickup or irregular bar is written as a time signature of its length
  (MuseScore's convention).
- Each part gets one MIDI channel for all its staves, so a piano's sustain
  pedal holds both hands. Percussion uses channel 10, and past 15 parts
  channels are shared by program.
- `Part.midi_channel` is 1–16 (0 = not set) everywhere, as in MusicXML.
- MIDI files written by LilyPond or lytk read their dynamics with LilyPond's
  table.
- MIDI reading is rebuilt for MIDI written by notation programs (LilyPond,
  MuseScore, lytk). It reads the notation back instead of snapping each
  length:
  - Tuplets (triplets, quintuplets, septuplets), with nothing drifting and no
    64th-rest gaps between legato notes.
  - Bars follow every time signature; a short first one is read as a pickup.
    Key signatures stay where they are.
  - A keyboard's two tracks are one two-staff part; channel 10 is a
    percussion part.
  - LilyPond's grace notes and staccatos are recognised.
  - Notes keep their velocity, and a dynamic mark goes where a part's level
    changes.
  - The sustain pedal becomes pedal marks, and lyric events become lyrics.
  - MIDI format 2 gives a clear error.
- Lowering a `MusicDocument` to a score keeps volta repeats as repeats with
  their endings instead of writing them out.
- `**kern` writing marks repeats (`:|!`, `!|:`), and reading understands
  `:|!|:` as the end of one repeat and the start of the next.

### Fixed

- LilyPond export: every MusicXML acid-test fixture now compiles with
  LilyPond 2.22, and LilyPond's MIDI of lytk's output matches lytk's own MIDI
  on 11664 of 11705 notes (was 6371):
  - full-bar rests in 3/4 (`R1*3/4`) and other lengths no single value
    writes are written as a scaled whole, not as `R4`;
  - additive meters are written `\compoundMeter #'((3 2 8))` (and read back);
  - bar checks, breaks, keys and marks are written inside a repeat's
    alternatives, not between them (LilyPond counted them as more
    alternatives and dropped the extra ones);
  - a one-element tuplet (`\tuplet 4/2 { r1 } la4`) closes before the next
    note;
  - graces ending a bar are written after its bar check, joined to the next
    bar's graces (LilyPond aborted);
  - lyric syllables LilyPond would misread (`0/0/1`, `2nd`, braces) are
    quoted;
  - transposing instruments write `\transposition`, and chord names are no
    longer performed in LilyPond's MIDI.
- LilyPond reading: `\transposition` sets the instrument's transposition
  (its pitch was read as a note); quoted lyric syllables are kept;
  `\set stanza = "1."` is no syllable; an empty alternative after the first
  (`\alternative { { d'1 } { } }`) repeats as LilyPond plays it.
- MIDI import (second review): a MuseScore piano export (program change on the
  top staff only) is one grand staff; each voice keeps its own dynamic level;
  a last staccato is recognised by LilyPond's half-second cap; a 128th at low
  resolution is no grace; a grace inside a triplet is found; files whose notes
  would hold more than 500,000 bars are refused.
- LilyPond export: marks placed by position — dynamics, hairpins, text,
  pedals from a `\new Dynamics` staff, and every mark read from MIDI —
  land on their notes; they all went on their bar's first note (a hairpin
  became `\<\!`).
- MIDI import of notation files: a run of staccatos as LilyPond plays them
  (each 4 louder) is read as staccatos throughout, not only its first and
  last notes; a dotted note followed by a rest a third its length keeps the
  rest.
- MIDI import of played files: a note 4 louder than its neighbour by chance
  is no longer read as a staccato (with a doubled last note and an extra
  bar) — that rule is how LilyPond and lytk write a staccato, and now applies
  to notation files only.
- MIDI export: a lone first ending closed by a repeat sign plays once.
- MusicXML → IR: an ending that also opens a repeat (45e) starts that repeat.
- ABC: a tune whose first line opens with `[V:S]` has no extra empty voice.
- Retrograde keeps the final bar line last.
- ABC reading:
  - The key signature now applies to notes: modes, explicit accidentals,
    `exp`, `none` and Highland pipes. `K:D` read F as F♮.
  - Accidentals last to the end of the bar (`%%propagate-accidentals`,
    `I:propagate-accidentals`), and a tied note keeps its accidental over the
    bar line.
  - `-8`/`+8` clefs and `octave=` transpose.
  - Inline `[K:]`, `[M:]` and `[L:]` are read.
  - Chord lengths and ties inside chords are read, and a tie on the last note
    of a tuplet is kept.
- ABC reading no longer makes notes out of lyric lines (`w:`), symbol lines,
  `+:` continuations, decorations or free text after the tune. It reads the
  first tune of a file only, and an out-of-range meter (`M:256/4`) no longer
  hangs.
- ABC writing:
  - Naturals and every other accidental a standard reader needs are written.
  - Every key is named: G♯, D♯ and A♯ minor came out as `K:Cm`.
  - A voice keeps its own key and meter, and chord ties are written.
  - No blank line inside a tune: readers stopped after the first voice.
- MIDI export:
  - Bars last as long as their music, so a pickup is no longer followed by
    silence.
  - Tuplets no longer drift.
  - MusicXML dynamics set velocities.
  - MusicXML channels are no longer off by one, so drums stay on channel 10.
  - Tied chords sound once.
- LilyPond reading:
  - A chord's `~` ties every note.
  - Articulations and ties inside a chord (`<dis-4-!>`) are kept.
  - Plain-pitch music in a variable used inside `\relative` is read
    relatively: chopin_n's cadenza was an octave low.
- Reading ABC and `**kern`, and writing them:
  - A note that crosses a bar line is tied over it instead of being copied
    into both bars.
  - All parts share one bar grid, and repeat bar lines land on the right
    side of the bar.
  - A piano staff keeps its second staff's clefs, chord names, figures and
    bar lines, and a piano staff inside a staff group is no longer written
    twice.
- ABC endings (`[1`, `[2`, `|1`, `:|2`) are read as endings; they became one
  chord. A short first bar is read as a pickup, and any other bar that
  doesn't fill its meter keeps its length.
- ABC reading: `(5` in a compound meter is five in the time of three;
  additive meters (`2+3+2/8`, `(2+3+2)/8`) keep their length and groups; a
  body `L:` or `M:` no longer changes the other voices.
- ABC writing keeps repeats and endings (they were dropped), writes no
  doubled bar lines (`| |`), and starts a pickup as a short first bar.
- LilyPond writing adds `\partial` for a pickup read from ABC or `**kern`.
- Repeats:
  - A bar line that ends one section and starts the next (`::`, a `|:`
    right after an ending) keeps both signs.
  - A repeat with endings but no start sign is kept (MusicXML 45b, ABC
    `C D|1 E F:|2 G A|]`).
  - An ending the tune ends in is closed.
- ABC writing keeps a staff's inner voices as `&` layers instead of dropping
  them, writes spacers as `x`, and ties a note over a bar line instead of
  overfilling the bar. The ABC of the MusicXML test files reads back with
  212 of 20,841 notes wrong, down from 6,702.
- ABC writing:
  - The last ending closes with `||`, so readers don't carry it on.
  - A backward repeat read from MusicXML is written `:|`, not `|]`.
  - Repeats of three or more passes play them all.
  - An irregular bar stays one bar, and a score without a meter keeps its
    bar lines.
- ABC reading without a meter (`M:none`, no `M:`) keeps each bar as
  written; notes were tied across invented whole-note bars.
- Turning a `MusicDocument` back into a score:
  - Directions stay where they are in the bar; they moved to beat 1.
  - A piano lowered twice keeps its two staves.
  - A slur that ends and restarts on a note cut by a bar line, and a
    tremolo on it, are kept.
  - More than 255 voices no longer hang it.
- MusicXML writing declares a piano group nested in a bracket group in the
  part-list; the part was missing there.
- LilyPond writing:
  - `\partial` opens the first staff even when a later bar has two voices.
  - A pickup that isn't one note value is written as LilyPond reads it
    (`\partial 2.`, `\partial 8*5`).
  - Retrograde gives the reversed music the right pickup.
- LilyPond reading: parts inside staff groups get automatic beams and stems,
  and stems follow each staff's own clef.
- LilyPond writing groups grace notes that follow each other
  (`\acciaccatura { a16 b16 }`); two grace commands in a row made LilyPond
  abort (fixtures 24a, 24e, 61f and imported MIDI).
- Tempos read from MIDI are whole beats a minute where the writer truncated
  them, and LilyPond writing prints whole numbers; it rejected
  `\tempo 4 = 99.0001`.
- MIDI writing keeps a staccato or portato over a tied note; the tie used to
  undo the shortening.
- MIDI reading:
  - A MuseScore piano (both staves' tracks named after the part) is two
    staves again, with the left hand's pedal.
  - Grace groups lead to the right note.
  - 128th-note runs are no longer taken for grace notes.
  - A staccato's extra velocity no longer adds dynamic marks.
  - Each staff keeps its own dynamic level.
  - A last note played at full length stays unmarked.
  - Files with far-off events are refused rather than exhausting memory.
- ABC reading:
  - `Z2` is two bars.
  - An `x` inside a tuplet takes the tuplet's time.
  - Broken rhythm reaches into a tuplet just closed.
  - A byte-order mark, a `[V:` inside a comment, `L:0` and huge meters no
    longer derail it.
  - A draft tune without `K:` no longer fails the whole file.
- Repeats:
  - An ending marked `discontinue`, or followed by a new repeat, ends there.
  - MusicXML's `times=` on a backward repeat counts.
  - An ending that plays only the first time keeps its repeat sign.
- Note arrays keep a tie that continues into a bar of several voices, and no
  longer join a later note of the same pitch to it.
- LilyPond writing of a `MusicDocument`:
  - Graces in a row form one group, and a group is written without doubled
    braces (the reader lost its notes).
  - Ties on single chord notes are written.
- LilyPond writing of a score groups mixed grace kinds into one group.
- Lowering a piano keeps staff-2 directions on staff 2, and a bar line at
  the very start no longer closes the first bar.
- MusicXML writing gives nested part-groups distinct numbers.
- Turning a score into a `MusicDocument` keeps chord symbols, puts
  directions where they are in the bar rather than at its start, and marks
  bars that aren't their meter's length (`Music::Partial`). LilyPond
  writing of a `MusicDocument` no longer turns a chord symbol into a note.

## [0.1.0] - 2026-09-25

First public release.

### Added

- Readers and writers for LilyPond, MusicXML, compressed MusicXML (`.mxl`),
  MIDI, ABC and Humdrum `**kern`, all through one internal representation.
- A LilyPond reader for real files: variables, `\relative`, piano scores with
  several voices per staff, repeats and voltas, cadenzas, lyrics, chord names
  and figured bass. (It does not follow `\include`: `lytk flatten` inlines
  includes first.) The LilyPond writer supports all 12
  note-name languages.
- Transforms: transpose (by semitones, interval or target key), invert,
  retrograde, and note-name language changes.
- Machine-learning encodings, each with its inverse: note arrays, piano rolls
  and Performance-RNN event sequences. Also objective metrics and DLPack
  support.
- Folder datasets with deterministic splits and caching, plus padded data
  loaders for PyTorch and TensorFlow, and generation-evaluation metrics
  (`lytk[eval]`).
- The `lytk` command: `convert`, `transpose`, `invert`, `retrograde`,
  `change-language`, `abs2rel`, `rel2abs`, `info`, `positions`, `bundle`,
  `diff`, `batch` and `flatten`. Folder conversion runs in parallel.
- Round-trip fidelity checks in CI for every format. Their baselines can only
  go up.
- Prebuilt abi3 wheels for Linux, macOS and Windows, for CPython 3.10 and
  newer.
- MIT licence.

[Unreleased]: https://github.com/CSCPadova/lytk/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/CSCPadova/lytk/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/CSCPadova/lytk/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/CSCPadova/lytk/releases/tag/v0.1.0
