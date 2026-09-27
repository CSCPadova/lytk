//! Input modes and movements of the LilyPond reader: `\drums`, `\chords`,
//! `\figures`, `\lyrics`, `\fixed`, and a movement per top-level music
//! expression, as LilyPond 2.22 reads them.

use _core::adapters::ly_to_ir::{check, LyToIrAdapter};
use _core::ir::measure::ClefSign;
use _core::ir::note::VoiceElement;
use _core::ir::Score;

fn read(text: &str) -> Vec<Score> {
    LyToIrAdapter::new().read_str(text).expect("reads").scores
}

/// MIDI numbers of each part's notes, chords flattened.
fn midi(score: &Score) -> Vec<Vec<i32>> {
    score
        .parts()
        .iter()
        .map(|p| {
            p.measures
                .iter()
                .flat_map(|m| &m.voices)
                .flat_map(|v| &v.elements)
                .flat_map(|e| match e {
                    VoiceElement::Note(n) => vec![n.pitch.midi_number()],
                    VoiceElement::Chord(c) => {
                        c.notes.iter().map(|n| n.pitch.midi_number()).collect()
                    }
                    _ => vec![],
                })
                .collect()
        })
        .collect()
}

fn codes(text: &str) -> Vec<&'static str> {
    check(text, true).iter().map(|d| d.code).collect()
}

#[test]
fn drum_names_are_general_midi_keys_on_a_percussion_staff() {
    let src = "\\drums { bd4 sn <bd hh> hhc }";
    let score = &read(src)[0];
    assert_eq!(midi(score), [vec![36, 38, 36, 42, 42]]);
    let clefs = &score.parts()[0].measures[0]
        .attributes
        .as_ref()
        .unwrap()
        .clefs;
    assert_eq!(clefs[&1].sign, ClefSign::Percussion);
    assert_eq!(codes(src), Vec::<&str>::new());
    // A drum voice in a drum staff, and a drum-mode variable.
    let src = "d = \\drummode { bd4 sn }\n\\new DrumStaff \\new DrumVoice \\drummode { \\d tomh }";
    assert_eq!(midi(&read(src)[0]), [vec![36, 38, 50]]);
    assert_eq!(codes("\\drums { bd4 xyz }"), ["unrecognized-token"]);
}

#[test]
fn chord_figure_and_lyric_shorthands_make_no_notes() {
    let src = "<< \\chords { c2 g:7 } \\figures { <6>2 <6 4> } \\new Staff { e'2 d' } \
               \\lyrics { la la } >>";
    let score = &read(src)[0];
    assert_eq!(midi(score), [vec![64, 62]]);
    let m = &score.parts()[0].measures[0];
    assert_eq!(m.harmonies.len(), 2);
    assert_eq!(m.figured_bass.len(), 2);
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn fixed_pitches_are_absolute_an_octave_up_per_mark() {
    // c' is MIDI 60; `\fixed c'` puts unmarked notes in its octave.
    assert_eq!(
        midi(&read("\\fixed c' { c4 g b, d'' }")[0]),
        [vec![60, 67, 59, 86]]
    );
    // Inside `\relative`, a `\fixed` block is absolute, and after it the
    // relative reading goes on from before it.
    assert_eq!(
        midi(&read("\\relative c'' { c4 \\fixed c { c e } d }")[0]),
        [vec![72, 48, 52, 74]]
    );
    assert_eq!(
        midi(&read("m = \\fixed c'' { c4 d }\n\\score { \\m }")[0]),
        [vec![72, 74]]
    );
}

#[test]
fn each_top_level_music_expression_is_a_movement() {
    let first = |scores: &[Score]| scores.iter().map(|s| midi(s)[0][0]).collect::<Vec<_>>();
    // LilyPond makes a score of each, in order, beside `\score` blocks.
    let src = "{ c'1 }\n\\score { { d'1 } }\n\\new Staff { e'1 }\nm = { f'1 }\n\\m";
    assert_eq!(first(&read(src)), [60, 62, 64, 65]);
    // `\addlyrics` belongs to the expression before it.
    let scores = read("{ c'4 d' } \\addlyrics { la la }\n{ e'1 }");
    assert_eq!(scores.len(), 2);
    let lyrics: usize = scores[0]
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .map(|e| match e {
            VoiceElement::Note(n) => n.lyrics.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(lyrics, 2);
    // A book's scores and music are movements too, and its header the book's.
    let src = "\\book { \\header { title = \"B\" } \\score { { c'1 } } { d'1 } }";
    let scores = read(src);
    assert_eq!(first(&scores), [60, 62]);
    assert!(scores
        .iter()
        .all(|s| s.metadata.title.as_deref() == Some("B")));
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn notes_outside_braces_are_a_syntax_error() {
    // LilyPond: "syntax error, unexpected NOTENAME_PITCH".
    assert_eq!(codes("\\version \"2.24.0\"\nc'4 d' e'"), ["syntax-error"]);
}
