# Changelog

All notable changes to lytk are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and lytk uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Dated
engineering notes are in the [development log](devlog.md).

## [Unreleased]

## [0.6.0] - 2026-10-04

The typed IR. Every enum-like name in the IR is an enum; directions,
chord symbols and figured bass sit at one exact position; a note's
dynamics, hairpins and text live in the bar's directions with its voice;
and the JSON leaves defaults out, 4.7 times smaller, and says which shape
it is in. This changes the JSON and dict shape once (see Changed); the
navigation API and every writer's output stay as they were, except where
something was fixed (MusicXML octave signs, noteheads, pedals, hairpins
and metronome beats; stacked chord symbols; string numbers from
`to_lilypond_music`; see Fixed).

### Changed

- **The IR's JSON and dict shape (breaking).** `to_json` and `to_dict`
  leave out every value at its default (an empty list, `None`, `false`,
  0; 1 for a voice, staff, verse, beam level or tuplet ratio; `true` for
  `print_object`), so a score's JSON is 4.7 times smaller (174 bytes a
  note over the fixtures, from 810): read a key with its default
  (`syl.get("number", 1)`). The JSON says its shape, `"schema": 1`, and
  `from_json`/`from_dict` refuse JSON without it, which lytk 0.5 or
  earlier wrote. A direction, chord symbol or figured-bass figure has one
  position, `offset`, in whole notes from the start of its bar (`[1, 2]`:
  half a bar in): a direction's `offset_frac` is now `offset`, and the
  integer offsets are gone (a direction's in the MusicXML file's
  divisions, a chord symbol's and a figure's in 16ths). Fractions are
  read in lowest terms (`[2, 16]` is `[1, 8]`).
- **Dynamics, hairpins and text have one home** (breaking): the bar's
  directions. One written on a note (LilyPond's `c4\f`, a MusicXML
  `<notations><dynamics>`, an ABC `!f!`) is a direction at the note's
  onset with its `voice`; one between notes (a MusicXML `<direction>`)
  has no voice and belongs to its staff. A note's or rest's `dynamics`,
  `wedges` and `text_directions` are gone from the JSON and from Rust.
  Every writer writes them as before.
- **Typed IR.** Every enum-like name in the IR is an enum, written in
  JSON as the name it had (`"staccato"`, `"forward hook"`, `"quarter"`):
  articulations, ornaments, technicals and dynamics (with `Other` for a
  name outside their lists), stems, noteheads, glissando lines, beams,
  hairpins, tuplet numbers, fermatas, barline sides and voltas, tempo beat
  units, text styles, octave signs, pedals, time symbols, chord kinds and
  degrees, figure accidentals and part-group brackets. Hand-written JSON
  with a name outside a closed list is refused. A chord symbol's root and
  bass and a rest's display step are pitch steps (`"C"`); a part group's
  type is a LilyPond context (`"PianoStaff"`, as before). Rust:
  `ArticulationType`, `OrnamentType`, `TechnicalType`, `DynamicType`,
  `StemDirection`, `Notehead`, `LineType`, `BeamValue`, `WedgeType`,
  `ShowNumber`, `FermataShape`, `BarlineLocation`, `EndingType`,
  `NoteType`, `FontStyle`, `FontWeight`, `OctaveShiftType`, `PedalType`,
  `TimeSymbol`, `ChordKind`, `DegreeType`, `FigureAccidental` and
  `GroupSymbol`; `TimeSignature::{terms, numerator, denominator,
  is_compound}`.

### Fixed

- MusicXML octave signs keep their octave: an 8va (MusicXML's
  `type="down"`, the notes printed down) was read as an 8vb and written
  to LilyPond as `\ottava #-1`, and LilyPond's `\ottava #1` was written
  to MusicXML as an 8vb.
- The MusicXML writer writes every notehead (`arrow down`, `cluster`,
  `slashed` and 9 others were dropped), a pedal's `discontinue` (it was a
  `start`), a metronome's beat of a `long` or a 32nd (it was a quarter), a
  part group without a symbol (it got a bracket), and a hairpin going on
  past a line break (it was a new crescendo; MIDI and ABC ended it there).
- ABC and MIDI keep a metronome's beat of a `long`, `maxima` or 64th: ABC
  dropped it and MIDI played it as a quarter.
- `to_lilypond_music` writes string numbers (`c4\1`), as `to_lilypond`
  does.
- LilyPond figured bass `<4-->` is MusicXML's `flat-flat` (it was
  `double-flat`, which MusicXML doesn't have).
- Chord symbols stacked before one MusicXML note (several `<harmony>`
  without `<offset>`) change exactly at their share of it, and LilyPond
  figured bass keeps each figure's exact length: both were rounded to
  16th notes.
- A score read back from its own JSON equals it
  (`Score.from_json(score.to_json())`): page sizes came back a rounding
  step off.

## [0.5.0] - 2026-10-04

Notation you can trust. A review found that what the note boards didn't
measure was broken somewhere, so 0.5.0 measures it — lyrics, chord
symbols, dynamics and clefs at their places, stems and beams against the
engravers that wrote the files, LilyPond output against LilyPond's own
MIDI — and fixes what the measurements showed. MusicXML is read where
`<backup>` and `<forward>` place the notes and written valid; LilyPond
lyrics are sung where LilyPond sings them and written so that LilyPond
sings them back; stems and beams follow LilyPond's rules and are drawn only
where a source left them open; chord symbols keep their beat and their
extensions; the LilyPond reader no longer drops constructs without a word;
note arrays give the velocities and sounding pitches MIDI export plays; ABC,
Humdrum and MIDI keep much more of the notation; `retrograde` and
`transpose` keep keys right; and the CLI writes every movement.

### Fixed

- MusicXML is read where `<backup>` and `<forward>` place the notes, not
  in file order: a voice that enters mid-bar (MuseScore's `<backup/>`
  `<forward/>`) no longer starts on beat 1 and no longer overfills another
  voice (bars of 9/8 in 6/8 in real files). Chord symbols and figured bass
  sit at their beat, not at the start of the bar.
- A second `<attributes>` in a MusicXML bar (a clef change between notes)
  no longer erases the bar's key and time signature.
- MusicXML output always declares `<divisions>` (it was missing when a
  score had no explicit time, key or clef, as LilyPond without `\time` and
  every Humdrum input, so other programs timed every note wrong) and
  escapes text: a title or lyric with `&` or `<` made XML nothing could
  read. Dotted short notes keep their exact duration, and `<backup>` after
  a voice with grace notes no longer goes back past the bar.
- MusicXML input: entities (`&amp;`, `&#233;`) are decoded and line breaks
  inside text kept; ISO-8859-1 and UTF-16 files are read; a file without
  `<divisions>` (lytk 0.4.0 wrote some) is read with the divisions its
  notes imply.
- ABC, Humdrum and MIDI text in Latin-1 is decoded (ABC files were
  refused, Humdrum and MIDI text mangled).
- MuseScore 4's glyph names in part names read as accidentals:
  `BaccidentalFlat Trumpet` is `B♭ Trumpet`.
- Humdrum output writes a spacer as an invisible rest (`4ryy`) instead of
  null tokens, which mean "the note before goes on" and moved a voice that
  enters mid-bar back to beat 1, and gives each spine its own staff's clef.
  Humdrum input finds a pickup bar on its longest spine, not the leftmost.

- LilyPond input that was misread without a word: `\afterGrace c2 { d16 e }`
  (the main note was taken for the grace and the graces were timed),
  `\grace <d f>8` and chords inside grace blocks (a timed chord, or notes
  one after another), the marks of grace notes (beams, slurs), single-note
  and two-note `\repeat tremolo` (the first was lost with the note after
  it, the second read as a volta repeat), `\partial 8*3` (its multiplier),
  `\time 3,2 5/8` (the signature was dropped), text scripts
  (`c^"dolce"`), `^`/`_` on articulations, slurs, fermatas, dynamics and
  fingerings, `\once \stemUp` (it stayed on), `\slurUp`/`\slurDown`,
  `\mark "Intro"`, `\mark \markup`, `\mark \default`, `\textMark`,
  `\sectionLabel`, `\jump`, `\fine`, `\segnoMark`, `\codaMark`. `-_` is
  portato, not tenuto.
- The LilyPond writers write placement (`^.`, `_\fermata`, `^(`).
- Note arrays, piano rolls, event sequences, metrics and `Score.notes()`:
  a dynamic in one part no longer carries into the next part; dynamics
  written as MusicXML directions are played (every velocity was 64); grace
  chords take no time (they delayed every later note).
- Transposing instruments survive the Music tree, ABC (`transpose=`) and
  Humdrum (`*ITrd-1c-2`), and the Music-path LilyPond writer writes
  `\transposition`. Humdrum output writes key, meter, clef and
  transposition changes after the first bar (only the first bar's were
  written).
- `MusicDocument.to_score()` and `Score.to_music_document()` keep the
  LilyPond diagnostics.
- LilyPond chord names. The writer put the duration after the modifiers
  (`a:71` for `a1:7`), spread a bar's chords evenly whatever their beats,
  sized bars by the meter (after a pickup every chord came early), wrote
  roots in Dutch whatever the output language, a no-chord as a major chord,
  and dropped added and altered steps. Each chord is now written at its
  beat as `root duration :modifiers /bass` (`g4:7.9-`), in the output
  language, and a no-chord as a rest (LilyPond prints N.C.). The
  LilyPond-to-LilyPond route (the CLI's `convert a.ly -o b.ly`) dropped
  every chord symbol; it writes them in a ChordNames line. The reader
  takes a chord-mode rest as a no-chord, reads scaled lengths
  (`c1*3/4`), and a chord-mode block used as notes gives its roots only
  (the bass of `f4:maj7/e` was a second note, doubling the bar).
- Chord-symbol suffixes are read as the chord's steps and matched to the
  nearest MusicXML kind with degrees, in LilyPond (`c:9^7`, `c:m7+`,
  `c:3.5.9`) and lead-sheet text (ABC): `7#9`, `add9`, `13b9`, `m(maj7)`,
  `6/9`, `7sus4` were all read as major.
- ABC: a quoted string is a chord symbol only when it reads as one
  (`"Fine"` was an F major chord), `"N.C."` is a no-chord, double-flat and
  double-sharp roots are kept, and a staff whose bars all have two voices
  is written (it was dropped as silent).
- MusicXML: harmonies one after another before a note change during that
  note, as the MusicXML test suite describes (71g), instead of all sitting
  at its start; the functional kinds (Neapolitan, Italian, French, German,
  pedal, Tristan) are written back (they became `other`).

- Stems as LilyPond draws them. The middle line was found in semitones (A4
  counted as the treble middle line; bass and C clefs off too). A beam
  group now takes one direction from its farthest note, a chord from its
  outermost notes, two voices on a staff point up and down, grace notes
  up, and a whole note has none.
- Beams as LilyPond draws them: by meter (3/4 eighths as one group, 3/8 and
  2/8 the whole bar, 5/8 as 3+2, `3+2/8` by its terms, 6/8 and 6/4 by the
  dotted beat), by the beam's shortest note (sixteenths go by the beat in
  4/4), ended by rests, quarters and gaps (`6/8 c8 r d` and `2/2 c8 d4 e8`
  were beamed across), placed from where a pickup sits in its bar, with
  secondary beams and hooks (a dotted rhythm's sixteenth hooks back), and
  `c16[ d e f]` gets its second beam.
- LilyPond `\noBeam` and `\set autoBeaming = ##f` are read; a `\stemDown`
  written for one bar no longer lasts into the next.
- MusicXML: a file that beams decides every note's beaming, as MuseScore
  reads it (unbeamed notes stay unbeamed).
- ABC beams: notes written together are beamed, and beamed notes are
  written together (every note had a space after it). Humdrum `L`, `J`,
  `K`, `k` are read and written.
- `retrograde` turns beams round (it wrote `e''16] d''16 c''8[`, which
  LilyPond rejects).

- LilyPond output: church modes keep their tonic (D dorian was written
  `\key c \dorian`, two flats); a piano part's tempo is written once (it
  was in both staves and read back twice); `\tempo`, `\mark` and breaks
  sit at their beat, not at the bar's start; a rehearsal mark keeps its
  text (`\mark "Intro"` came back as A).
- The LilyPond-to-LilyPond route (the music tree) wrote a dynamic, hairpin
  or text that falls between notes after the note before it, which then
  carried it (a MusicXML `ff` on beat 2 landed on beat 1); it writes them
  as `<>\ff` at their moment, escapes their text and keeps rehearsal marks,
  D.C./D.S., segno, coda, ottavas and breaks (all were dropped).
- LilyPond input: `<>` takes no time and its marks are directions at that
  moment (it was an empty chord one beat long, pushing every later note).
- A clef change inside a bar keeps its place, through every reader and
  writer (it moved to the bar's start: the notes before it were read in
  the new clef).
- A MusicXML D.C. or D.S. with words was written twice by every writer.
- MusicXML output writes the placement of articulations, ornaments and
  slurs (`^.`, `_(`).
- ABC output: hairpin ends match their starts (a diminuendo was closed as a
  crescendo), clefs are written (`K:… clef=bass`, `[K:clef=…]`), and
  fingerings and bowings are written and read (`!1!`, `!upbow!`, `u`, `v`).
- Humdrum output: a voice absent from a bar, or ending early, rests there
  invisibly (null tokens carried its last note on, which kern reads as
  longer notes), every spine says its `*part` and `*staff`, and
  articulations and fermatas are written and read (`'`, `` ` ``, `^`,
  `^^`, `~`, `;`).
- MIDI output: parts with different programs never share a channel (a
  MuseScore file's flute and choir both asked for channel 1 and played as
  one instrument), and the title, composer and copyright are written (and
  read back).

- Lyrics from LilyPond are sung where LilyPond sings them (31 of 31 cases
  checked against LilyPond 2.22's own MIDI; 11 before):
  - slurs and ties are melismas whatever the beaming, manual beams under
    `\autoBeamOff`; chords too;
  - `__` extends the syllable before (it used two notes), `~` joins words
    on one note (`a‿b`), `_` in a word is a space, punctuation stays in its
    word (`don't` was two syllables), a lone `-` is a word, `la4` has no `4`;
  - `\skip`, `\repeat unfold`, `\set stanza`, `\set ignoreMelismata`,
    unquoted `\lyricsto mel`, a lyric variable after `\addlyrics`, and a
    Lyrics line without `\lyricsto` (placed by its durations);
  - two `\addlyrics` or `\lyricsto` lines are two verses (they were one,
    interleaved, or the second replaced the first), in source order;
  - `\lyricsto "s"` follows the voice named "s", not every voice of its
    staff; a `<< { } \\ { } >>` passage is new voices, which lyrics skip;
    music after a `\new Voice` stays in it; lyrics on a NullVoice go on the
    notes that start with its notes.
- LilyPond output of lyrics: one token per note under `\set
  ignoreMelismata = ##t`, so a slurred pair from MusicXML no longer loses
  its second syllable; a verse starting late stays in place; the lyrics'
  voice goes on through multi-voice bars (`<< { \voiceOne … } \new Voice {
  \voiceTwo … } >>`) instead of skipping them; another voice that sings,
  or a voice on another staff, sings on a NullVoice; elisions are written
  `a~b`, verse names as `\set stanza`, and any word LilyPond would misread
  (a digit anywhere: `dominant-11th`) is quoted. LilyPond sings 710 of 710
  syllables of the test files where lytk has them. The LilyPond-to-LilyPond
  route does the same with `\addlyrics`.
- MusicXML lyrics: elided syllables keep both words (`my‿a`), a verse named
  by `name` or a `number` such as `chorus` is its own verse (several were
  verse 1), `<extend type="stop"/>` ends an extender instead of starting
  one, a lyric on a later chord note is kept (on the chord's first), and the
  writer writes elisions and verse names.
- ABC lyrics: a verse starting mid-line keeps its leading `*`s, a verse
  with nothing on a line keeps its `w:` line (later verses were renumbered),
  every music line gets its `w:` lines, a `w:` line is sung on the music
  line above it (ABC 2.1; it went on from the last one, so lyrics under bar
  47 landed on bar 1), a word hyphenated over two `w:` lines stays one, and
  `- _ * ~ | % \` in a syllable are escaped.
- MIDI lyrics: a syllable after one ending in `-` continues its word
  (every syllable was a beginning or alone); a track carries one verse
  (verses were mixed note by note).
- Humdrum: `**text` (and `**silbe`) spines are read onto their kern spine
  and written, one per verse.
- LilyPond output: a dynamic LilyPond doesn't define (`pppppp`) is written
  `-#(make-dynamic-script "pppppp")` (`\pppppp` stopped LilyPond), and read
  back; `\sfp` and `\n` are read.

- `retrograde` moves key, meter, transposition and clef changes with
  their music (bar 1 kept the first key over what had been the last bar),
  starts each bar in the clef its music ended in and mirrors clef changes
  inside it, and turns words round (a word's last syllable begins it).
- `transpose` by semitones picks the key with fewer accidentals: G major up
  6 is D-flat (5 flats), not C-sharp (7 sharps).

- CLI: `convert` from LilyPond to LilyPond, the transforms, `abs2rel` and
  `rel2abs` write every movement (they kept the first, silently); the
  LilyPond reader's findings are printed on stderr; a folder conversion is
  refused when two inputs would write one output (`a.mid`, `a.midi`);
  `positions` counts no time for grace chords.

### Added

- CLI: `-I`/`--include-path` on every command that reads a score.
- `Note.lyric_syllables` in Python: each syllable's verse, name, syllabic,
  extender and elision (`Note.lyrics` gives the texts only). Rust:
  `LyricSyllable.name`.
- `Direction.clef`: a clef change inside the bar, at its position (a bar's
  opening clef stays in its attributes). `KeySignature::tonic()`,
  `RehearsalMark::lilypond_default(n)`.
- `Annotation::NoAutoBeam` in the Music tree (a note whose beaming the
  source decided), carried by the lift and the lowering with the source's
  beams; `VoiceElement::notes()`/`notes_mut()`.
- Rust: chord symbols as steps, `ir::harmony::{chord_steps,
  kind_and_degrees, ly_chord_steps, lead_sheet_chord_steps,
  parse_chord_suffix, lead_sheet_suffix, ly_chord_modifiers}`, and
  `Measure::content_length`.

### Changed

- `lytk diff` compares every note's onset, duration and sounding pitch,
  not only the pitches (`--json` adds `notes_equal`).
- Readers keep only the stems and beams a source states: LilyPond and
  MIDI input no longer carry inferred ones (in `to_dict`, navigation, the
  Music tree). The writers that spell them out (MusicXML, ABC, Humdrum)
  engrave what the source left open when they write, and the LilyPond
  writer writes a stem command only where the source's stem isn't the one
  LilyPond draws (it froze every inferred stem: `\stemDown a8[ b c d]`,
  `\stemDown` in the upper of two voices, `\stemNeutral` before rests).
- Note arrays (and everything built on them: piano rolls, event
  sequences, metrics, datasets, `Score.notes()`) give **sounding pitch**: a
  B♭ clarinet's written D is a C, as MIDI export plays it.
  `to_note_array(..., pitch="written")` gives written pitch.
- Note-array velocities are those `to_midi` plays: LilyPond's dynamics
  table (p 69, mf 86, f 95), and **90** without a dynamic (it was 64, on a
  different table). Dataset caches refresh by themselves (their key holds
  lytk's version).
- MusicXML output declares the MusicXML 4.0 document type (it said 3.0).
- LilyPond: an acciaccatura or appoggiatura is read as grace notes with a
  slur to their main note, and the writers write grace notes as `\grace`
  or `\slashedGrace` with their slurs: they wrote every grace as
  `\acciaccatura`/`\appoggiatura`, adding a slur that a MusicXML source
  doesn't have.

## [0.4.0] - 2026-09-28

LilyPond source you can inspect and edit, and datasets for curated
corpora. `\version` is read, compared and edited; LilyPond text is
tokenized (embedded Scheme as Scheme) and counted; includes are found on
the syntax tree, `flatten_string` flattens text, and every reader follows
includes when given `include_paths=`, with diagnostics pointing into the
caller's text. `lytk.info` gives a score's statistics. Datasets read JSON
Lines records as well as folders, keep ids and a corpus's own splits, skip
what they cannot read when asked, and cache by content. The Python API
reference is generated from the stubs.

### Added

- LilyPond `\version`: `lytk.lilypond_version(text)` reads the version the
  first `\version` statement states, from the parse tree (a commented-out
  one does not count), as a `lytk.LilyPondVersion`, which compares
  numerically (`2.24` equals `2.24.0`) and accepts what LilyPond 2.24
  accepts. `lytk.set_lilypond_version(text, version)` and
  `lytk.strip_lilypond_version(text)` edit the statements (a version given
  as a string is written as given, once checked).
  `Score.lilypond_version` and `MusicDocument.lilypond_version` give the
  version a LilyPond source stated. `to_lilypond(…, version=)` and
  `to_lilypond_music(…, version=)` write another `\version` than 2.24.0.
  An invalid `\version` is an `invalid-version` error. Rust:
  `ly_to_ir::{LilyPondVersion, lilypond_version, set_lilypond_version,
  strip_lilypond_version}` and `ScoreMetadata::lilypond_version`.
- `lytk.tokenize(text)`: the tokens of LilyPond text from the parse tree
  (`lytk.Token`: kind, text, character span, line, column, and whether it
  is Scheme). The kinds are comment, string, command, symbol, number,
  fraction, punctuation, and error for text the grammar cannot tokenize, so
  every character but whitespace is in exactly one token, broken input
  included. Embedded Scheme is tokenized as Scheme: `scheme` is the `#` or
  `$` that starts it, then its symbols, numbers, strings, comments,
  brackets and quotes, with `boolean`, `character` and `keyword` for
  Scheme's own; LilyPond inside it (`#{ … #}`) is tokenized as LilyPond.
  `lytk.strip_comments(text)` removes LilyPond's and Scheme's comments.
  Rust: `ly_to_ir::{tokenize, strip_comments, Token, TokenKind}`.
- `lytk.info(score)`: a score's metadata and counts as a dict, the library
  home of `lytk info --json`, which now calls it. Besides the parts and notes
  it counts voices, bars, the length in quarter notes, lyric syllables, chord
  symbols and grace notes, computed in Rust. `lytk.source_stats(text)`:
  bytes, lines, tokens, comments, Scheme expressions and error tokens of
  LilyPond text. Rust: `ir::stats::score_info`, `ly_to_ir::source_stats`.
- `lytk.from_lilypond_music_movements(path)`: every movement of a LilyPond
  file as a `MusicDocument`, as `from_lilypond_movements` gives them as
  scores; `lytk.from_lilypond_movements_string(text)`, every movement of
  LilyPond text.
- `lytk.flatten_string(text, *, base_dir=None, include_paths=None,
  add_markers=True)`: `flatten` for text. `lytk.from_abc_tunes_string(text)`:
  every tune of ABC text.
- `lytk.datasets.RecordsDataset`: a dataset over records holding music as
  text, `from_jsonl(path, …)` or `from_records(iterable, …)`, with
  `text_field`, `id_field`, `format` (LilyPond, MusicXML, ABC or `**kern`)
  and `split_field`. Items keep their ids and records (`record(i)`); with
  `split_field`, `split()` returns the records' own splits as
  `{value: Subset}` instead of re-shuffling them.
- Datasets: `on_error="raise" | "skip" | "warn"` (iteration leaves out an
  item that cannot be read and records it in `dataset.errors`; indexing
  still raises); `movements="all"` (each movement of a file or record is an
  item); `language`, `include_paths`, `strict` and MIDI `quantize` passed to
  the readers; `ids` on every dataset (a folder's are paths relative to it);
  `return_ids=True` on the torch and TensorFlow datasets and data loaders;
  `split(…, groups=…)` keeps the items of a group in one subset.
- `docs/python-api.md`, the Python API reference, generated from the stubs
  (`scripts/python_api.py`); a test fails when it is stale.
- `include_paths=` on every LilyPond reader and on `check_lilypond`: given
  (possibly empty), `\include`s are followed, relative to the file's
  directory, then the paths. A diagnostic in an included file is reported at
  its `\include`, naming the file; an include not found is an
  `ignored-include` warning. Without it, includes are still not followed.
  Rust: `LyToIrAdapter::with_include_paths`, `read_text` and `check_str`.

### Changed

- `flatten` finds includes on the parse tree: anywhere in a line, never in a
  comment or a string. It used to miss an include not at the start of a
  line, and after a one-line `%{ … %}` it took the rest of the file for a
  comment and followed no include in it.
- `flatten` keeps an include of LilyPond's own files (`english.ly`,
  `gregorian.ly`, …) as it is when no such file is found; it was a "not
  found" error. Rust: `flatten_str` takes `Option<&Path>` for its base
  directory, and `FlattenOpts::keep_missing` keeps any missing include.
- The dataset cache is keyed by the item's content, how it is read, lytk's
  version and the converter's arguments with their defaults filled in, and
  written atomically: an edited file is no longer served stale, and
  `to_note_arrays()` and `iter_representation("note_array")` share entries.
  Entries of earlier versions are not read again. A `Subset` uses its
  parent's cache (it bypassed it).

### Fixed

- `flatten` kept only the last `\language` line, so music written before it
  was read in the wrong language; every `\language` now stays.
- `flatten` refused a file with more than one `\header` block, a top-level
  one with a score's own included (`FlattenError::MultipleHeaders`, now
  gone). LilyPond merges top-level headers, and so does lytk's reader.
- The `ignored-include` warning for one of LilyPond's own files (other than a
  language file) no longer advises flattening, which would not follow it.

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

[Unreleased]: https://github.com/CSCPadova/lytk/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/CSCPadova/lytk/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/CSCPadova/lytk/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/CSCPadova/lytk/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/CSCPadova/lytk/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/CSCPadova/lytk/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/CSCPadova/lytk/releases/tag/v0.1.0
