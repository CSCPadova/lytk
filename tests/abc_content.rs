//! ABC content beyond pitches and rhythm (epic I, phase A4): decorations,
//! chord symbols, annotations, slurs, tempo and lyrics — read as ABC 2.1
//! says, and kept through the writer.

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::ir_to_abc::IrToAbcAdapter;
use _core::adapters::{FromMusicAdapter, ToIrAdapter};
use _core::ir::articulation::{StartStop, SyllabicType};
use _core::ir::note::{Note, VoiceElement};
use _core::ir::score::Score;

fn read(abc: &str) -> Score {
    AbcToIrAdapter::new()
        .convert_str(abc)
        .unwrap_or_else(|e| panic!("{e}\n{abc}"))
}

/// Read, write and read again.
fn again(score: &Score) -> Score {
    let abc = IrToAbcAdapter::new()
        .convert_music(&_core::ir::lift::lift_to_music(score))
        .unwrap();
    read(&abc)
}

/// The notes of the first part, in order (chords by their first note).
fn notes(score: &Score) -> Vec<Note> {
    score.parts()[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter_map(|e| match e {
            VoiceElement::Note(n) if !n.is_grace => Some((**n).clone()),
            VoiceElement::Chord(c) => c.notes.first().cloned(),
            _ => None,
        })
        .collect()
}

fn marks(n: &Note) -> Vec<String> {
    let mut out: Vec<String> = n.articulations.iter().map(|a| a.name.clone()).collect();
    out.extend(n.ornaments.iter().map(|o| o.name.clone()));
    out.extend(n.dynamics.iter().map(|d| d.sign.clone()));
    out.extend(n.wedges.iter().map(|w| w.wedge_type.clone()));
    if n.fermata.is_some() {
        out.push("fermata".to_string());
    }
    out
}

#[test]
fn decorations_become_marks() {
    let abc = "X:1\nL:1/4\nK:C\n!p! C !<(! D E !<)!!f! F|.G !fermata!A !accent!B !trill!c|\n";
    for score in [read(abc), again(&read(abc))] {
        let n = notes(&score);
        let m: Vec<Vec<String>> = n.iter().map(marks).collect();
        assert_eq!(m[0], ["p"]);
        assert_eq!(m[1], ["crescendo"]);
        assert!(
            m[3].contains(&"f".to_string()) && m[3].contains(&"stop".to_string()),
            "{m:?}"
        );
        assert_eq!(m[4], ["staccato"]);
        assert_eq!(m[5], ["fermata"]);
        assert_eq!(m[6], ["accent"]);
        assert_eq!(m[7], ["trill-mark"]);
    }
}

#[test]
fn chord_symbols_and_annotations() {
    let abc = "X:1\nL:1/4\nK:C\n\"Am7\"A B \"G/B\"c \"^dolce\"d|\n";
    for score in [read(abc), again(&read(abc))] {
        let m = &score.parts()[0].measures[0];
        let h: Vec<(String, String, Option<String>, i32)> = m
            .harmonies
            .iter()
            .map(|h| {
                (
                    h.root.step.clone(),
                    h.kind.clone(),
                    h.bass.as_ref().map(|b| b.step.clone()),
                    h.offset,
                )
            })
            .collect();
        assert_eq!(h.len(), 2, "{h:?}");
        assert_eq!((h[0].0.as_str(), h[0].1.as_str()), ("A", "minor-seventh"));
        assert_eq!(h[1].2.as_deref(), Some("B"));
        assert!(h[1].3 > h[0].3);
        let words: Vec<String> = m
            .directions
            .iter()
            .filter_map(|d| d.text.as_ref().map(|t| t.text.clone()))
            .collect();
        assert_eq!(words, ["dolce"]);
    }
}

#[test]
fn slurs_tempo_and_lyrics() {
    // `_` holds "la" over F.
    let abc = "X:1\nL:1/4\nQ:1/4=132\nK:C\n(C D E) F|G2 A2|\nw: la-la la_ glo-ry\n";
    for score in [read(abc), again(&read(abc))] {
        let n = notes(&score);
        let slur = |k: usize, t: StartStop| n[k].slurs.iter().any(|s| s.slur_type == t);
        assert!(slur(0, StartStop::Start) && slur(2, StartStop::Stop));
        let tempo = score.parts()[0].measures[0]
            .directions
            .iter()
            .find_map(|d| d.tempo.as_ref().and_then(|t| t.per_minute));
        assert_eq!(tempo, Some(132.0));
        let sung: Vec<(String, SyllabicType)> = n
            .iter()
            .map(|n| {
                n.lyrics
                    .first()
                    .map_or((String::new(), SyllabicType::Single), |l| {
                        (l.text.clone(), l.syllabic)
                    })
            })
            .collect();
        assert_eq!(sung[0], ("la".to_string(), SyllabicType::Begin));
        assert_eq!(sung[1], ("la".to_string(), SyllabicType::End));
        assert_eq!(sung[2].0, "la");
        assert!(n[2].lyrics[0].extend);
        assert_eq!(sung[3].0, "", "held, no syllable");
        assert_eq!(sung[4], ("glo".to_string(), SyllabicType::Begin));
        assert_eq!(sung[5], ("ry".to_string(), SyllabicType::End));
    }
}
