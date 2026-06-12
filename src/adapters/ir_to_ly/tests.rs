use super::*;
use crate::ir::articulation::{
    Articulation, LyricSyllable, Placement, SlurEvent, StartStop, SyllabicType, TieEvent,
};
use crate::ir::duration::Duration;
use crate::ir::measure::{Clef, KeyMode, KeySignature, Measure, MeasureAttributes, TimeSignature};
use crate::ir::note::{Note, Rest, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{Score, ScoreChild};
use crate::ir::voice::Voice;
use num::rational::Ratio;
use std::collections::HashMap;

use emit::{attachments_to_ly, chord_to_ly, rest_to_ly};
use maps::{clef_to_ly, duration_to_ly, figure_to_ly, harmony_kind_to_ly, key_to_ly, pitch_to_ly};

fn make_note(step: PitchStep, octave: i32, dur: Duration) -> Note {
    Note::new(Pitch::new(step, octave), dur)
}

fn make_simple_score() -> Score {
    // C4 quarter, D4 quarter, E4 quarter, F4 quarter
    let notes: Vec<VoiceElement> = vec![
        VoiceElement::Note(Box::new(make_note(PitchStep::C, 4, Duration::quarter()))),
        VoiceElement::Note(Box::new(make_note(PitchStep::D, 4, Duration::quarter()))),
        VoiceElement::Note(Box::new(make_note(PitchStep::E, 4, Duration::quarter()))),
        VoiceElement::Note(Box::new(make_note(PitchStep::F, 4, Duration::quarter()))),
    ];
    let voice = Voice {
        number: 1,
        elements: notes,
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        key: Some(KeySignature {
            fifths: 0,
            mode: KeyMode::Major,
        }),
        time: Some(TimeSignature::default()), // 4/4
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default()); // treble
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.name = "Piano".to_string();
    part.measures.push(measure);

    let mut score = Score::new();
    score.metadata.title = Some("Test".to_string());
    score.children.push(ScoreChild::Part(part));
    score
}

#[test]
fn test_emit_simple_score() {
    let score = make_simple_score();
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    assert!(ly.contains("\\version \"2.24.0\""));
    assert!(ly.contains("\\language \"nederlands\""));
    assert!(ly.contains("title = \"Test\""));
    assert!(ly.contains("\\key c \\major"));
    assert!(ly.contains("\\time 4/4"));
    assert!(ly.contains("\\clef \"treble\""));
    // Check notes are present (absolute mode: C4 = c')
    assert!(ly.contains("c'"));
    assert!(ly.contains("d'"));
    assert!(ly.contains("e'"));
    assert!(ly.contains("f'"));
    assert!(ly.contains("\\score {"));
    assert!(ly.contains("\\layout { }"));
    assert!(ly.contains("\\midi { }"));
}

#[test]
fn test_emit_duration_formats() {
    assert_eq!(duration_to_ly(&Duration::whole()), "1");
    assert_eq!(duration_to_ly(&Duration::half()), "2");
    assert_eq!(duration_to_ly(&Duration::quarter()), "4");
    assert_eq!(duration_to_ly(&Duration::eighth()), "8");
    assert_eq!(duration_to_ly(&Duration::sixteenth()), "16");
    assert_eq!(duration_to_ly(&Duration::dotted(Ratio::new(1, 4), 1)), "4.");
    assert_eq!(
        duration_to_ly(&Duration::dotted(Ratio::new(1, 4), 2)),
        "4.."
    );
}

#[test]
fn test_emit_pitch_absolute() {
    // C4 = c', D5 = d'', B3 = b
    let c4 = Pitch::new(PitchStep::C, 4);
    let d5 = Pitch::new(PitchStep::D, 5);
    let b3 = Pitch::new(PitchStep::B, 3);
    let c3 = Pitch::new(PitchStep::C, 3);

    let lang = PitchLanguage::Nederlands;

    assert_eq!(pitch_to_ly(&c4, lang, None, PitchMode::Absolute), "c'");
    assert_eq!(pitch_to_ly(&d5, lang, None, PitchMode::Absolute), "d''");
    assert_eq!(pitch_to_ly(&b3, lang, None, PitchMode::Absolute), "b");
    assert_eq!(pitch_to_ly(&c3, lang, None, PitchMode::Absolute), "c");
}

#[test]
fn test_emit_pitch_with_alter() {
    let fsharp4 = Pitch::with_alter(PitchStep::F, Ratio::new(1, 1), 4);
    let bflat3 = Pitch::with_alter(PitchStep::B, Ratio::new(-1, 1), 3);

    assert_eq!(
        pitch_to_ly(
            &fsharp4,
            PitchLanguage::Nederlands,
            None,
            PitchMode::Absolute
        ),
        "fis'"
    );
    assert_eq!(
        pitch_to_ly(
            &bflat3,
            PitchLanguage::Nederlands,
            None,
            PitchMode::Absolute
        ),
        "bes"
    );
}

#[test]
fn test_emit_rest_types() {
    assert_eq!(rest_to_ly(&Rest::new(Duration::quarter())), "r4");
    assert_eq!(rest_to_ly(&Rest::measure_rest(Duration::whole())), "R1");
    let mut spacer = Rest::new(Duration::half());
    spacer.is_spacer = true;
    assert_eq!(rest_to_ly(&spacer), "s2");
}

#[test]
fn test_emit_key_signatures() {
    assert_eq!(
        key_to_ly(
            &KeySignature {
                fifths: 0,
                mode: KeyMode::Major
            },
            PitchLanguage::Nederlands
        ),
        "\\key c \\major"
    );
    assert_eq!(
        key_to_ly(
            &KeySignature {
                fifths: 2,
                mode: KeyMode::Major
            },
            PitchLanguage::Nederlands
        ),
        "\\key d \\major"
    );
    assert_eq!(
        key_to_ly(
            &KeySignature {
                fifths: -3,
                mode: KeyMode::Minor
            },
            PitchLanguage::Nederlands
        ),
        "\\key c \\minor"
    );
}

#[test]
fn test_part_var_names_unique_for_zero_based_ids() {
    // P0 and P1 both mapped to "pA" (index_to_alpha(0) == index_to_alpha(1)),
    // so one part's variable shadowed the other on compile.
    let p0 = crate::ir::Part::new("P0");
    let p1 = crate::ir::Part::new("P1");
    assert_ne!(
        helpers::part_var_name(&p0),
        helpers::part_var_name(&p1),
        "0-based part ids must map to distinct LilyPond variables"
    );
}

#[test]
fn test_header_strings_escaped() {
    let mut score = Score::default();
    score.metadata.title = Some(r#"The "Great" Fugue"#.to_string());
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains(r#"title = "The \"Great\" Fugue""#),
        "embedded quotes must be escaped, got:\n{ly}"
    );
}

#[test]
fn test_emit_key_signature_matches_language() {
    // The tonic must be spelled in the emitted \language: a Nederlands "fis"
    // under \language "english" does not compile and was silently dropped on
    // re-parse.
    assert_eq!(
        key_to_ly(
            &KeySignature {
                fifths: 6,
                mode: KeyMode::Major
            },
            PitchLanguage::Nederlands
        ),
        "\\key fis \\major"
    );
    assert_eq!(
        key_to_ly(
            &KeySignature {
                fifths: 6,
                mode: KeyMode::Major
            },
            PitchLanguage::English
        ),
        "\\key fs \\major"
    );
    // Canonical Deutsch spelling ("ees", same as note emission; LilyPond's
    // deutsch.ly accepts it as an alias of "es").
    assert_eq!(
        key_to_ly(
            &KeySignature {
                fifths: -3,
                mode: KeyMode::Major
            },
            PitchLanguage::Deutsch
        ),
        "\\key ees \\major"
    );
}

#[test]
fn test_emit_clef() {
    use crate::ir::measure::ClefSign;
    assert_eq!(clef_to_ly(&Clef::default()), "\\clef \"treble\"");
    assert_eq!(
        clef_to_ly(&Clef {
            sign: ClefSign::F,
            line: 4,
            octave_change: 0
        }),
        "\\clef \"bass\""
    );
    assert_eq!(
        clef_to_ly(&Clef {
            sign: ClefSign::G,
            line: 2,
            octave_change: -1
        }),
        "\\clef \"treble_8\""
    );
}

#[test]
fn test_emit_attachments() {
    let mut note = make_note(PitchStep::C, 4, Duration::quarter());
    note.ties.push(TieEvent {
        tie_type: StartStop::Start,
    });
    note.slurs.push(SlurEvent {
        slur_type: StartStop::Start,
        number: 1,
        placement: Placement::Unspecified,
    });
    note.articulations.push(Articulation {
        name: "staccato".to_string(),
        placement: Placement::Unspecified,
    });

    let attach = attachments_to_ly(&note);
    assert!(attach.contains('~'));
    assert!(attach.contains('('));
    assert!(attach.contains("-."));
}

#[test]
fn test_emit_chord() {
    use crate::ir::note::Chord;
    let notes = vec![
        make_note(PitchStep::C, 4, Duration::quarter()),
        make_note(PitchStep::E, 4, Duration::quarter()),
        make_note(PitchStep::G, 4, Duration::quarter()),
    ];
    let chord = Chord::new(Duration::quarter(), notes);
    let (ly, _) = chord_to_ly(&chord, PitchLanguage::Nederlands, PitchMode::Absolute, None);
    assert!(ly.starts_with('<'));
    assert!(ly.contains("c'"));
    assert!(ly.contains("e'"));
    assert!(ly.contains("g'"));
    assert!(ly.contains(">4"));
}

#[test]
fn test_roundtrip_mxml_to_ly() {
    // Parse a MusicXML fixture, then emit as LilyPond
    use crate::adapters::mxml_to_ir::MxmlToIrAdapter;
    use crate::adapters::ToIrAdapter;

    let xml_path = std::path::Path::new("tests/fixtures/xml/01a-Pitches-Pitches.xml");
    if !xml_path.exists() {
        return; // skip if fixtures not available
    }

    let to_ir = MxmlToIrAdapter;
    let score = to_ir.convert_file(xml_path).unwrap();

    let from_ir = IrToLyAdapter::new();
    let ly = from_ir.convert(&score).unwrap();

    assert!(ly.contains("\\version"));
    assert!(ly.contains("\\score {"));
    // Should have at least some notes
    assert!(ly.len() > 100);
}

#[test]
fn test_roundtrip_ly_to_ir_to_ly_with_variables() {
    use crate::adapters::ly_to_ir::LyToIrAdapter;
    use crate::adapters::ToIrAdapter;

    let input = r#"\version "2.24.0"

melody = {
  \key g \major
  \time 3/4
  c'4 d' e' |
  f' g' a' |
}

\score {
  \new Staff \melody
  \layout {}
}
"#;
    let to_ir = LyToIrAdapter::new();
    let score = to_ir.convert_str(input).unwrap();

    // Score should have notes from the variable
    let parts = score.parts();
    assert!(!parts.is_empty());
    let notes: Vec<_> = parts[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter_map(|e| match e {
            crate::ir::note::VoiceElement::Note(n) => Some(n.as_ref()),
            _ => None,
        })
        .collect();
    assert!(notes.len() >= 6, "Expected >= 6 notes, got {}", notes.len());

    // Emit back to LilyPond
    let from_ir = IrToLyAdapter::new();
    let ly = from_ir.convert(&score).unwrap();

    assert!(ly.contains("\\version"));
    assert!(ly.contains("\\score {"));
    // Should contain actual notes, not empty
    assert!(ly.contains("c'") || ly.contains("d'") || ly.contains("e'"));
}

#[test]
fn test_roundtrip_ly_preserves_relative_mode() {
    use crate::adapters::ly_to_ir::LyToIrAdapter;
    use crate::adapters::ToIrAdapter;

    let input = r#"\relative c' { c4 d e f }"#;
    let to_ir = LyToIrAdapter::new();
    let score = to_ir.convert_str(input).unwrap();

    let from_ir = IrToLyAdapter::new();
    let ly = from_ir.convert(&score).unwrap();

    // Should emit \relative since the score used relative mode
    assert!(
        ly.contains("\\relative"),
        "Output should contain \\relative when input used relative mode. Got:\n{}",
        ly
    );
}

#[test]
fn test_roundtrip_all_xml_fixtures_to_ly() {
    // MusicXML -> IR -> LilyPond for every fixture file; verify
    // non-empty output with required structural elements.
    use crate::adapters::mxml_to_ir::MxmlToIrAdapter;
    use crate::adapters::ToIrAdapter;

    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("xml");
    let to_ir = MxmlToIrAdapter::new();
    let from_ir = IrToLyAdapter::new();

    let mut failures: Vec<(String, String)> = Vec::new();
    let mut success_count = 0;

    for entry in std::fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "xml") {
            let xml = std::fs::read_to_string(&path).unwrap();
            let score = match to_ir.convert_str(&xml) {
                Ok(s) => s,
                Err(e) => {
                    failures.push((
                        path.file_name().unwrap().to_string_lossy().to_string(),
                        format!("parse: {e}"),
                    ));
                    continue;
                }
            };

            match from_ir.convert(&score) {
                Ok(ly) => {
                    if !ly.contains("\\version") || !ly.contains("\\score {") {
                        failures.push((
                            path.file_name().unwrap().to_string_lossy().to_string(),
                            "missing \\version or \\score block".to_string(),
                        ));
                    } else {
                        success_count += 1;
                    }
                }
                Err(e) => {
                    failures.push((
                        path.file_name().unwrap().to_string_lossy().to_string(),
                        format!("emit: {e}"),
                    ));
                }
            }
        }
    }

    if !failures.is_empty() {
        let report: Vec<String> = failures
            .iter()
            .map(|(f, e)| format!("  {f}: {e}"))
            .collect();
        panic!(
            "{} of {} fixture files failed roundtrip:\n{}",
            failures.len(),
            success_count + failures.len(),
            report.join("\n")
        );
    }

    assert!(
        success_count >= 100,
        "expected at least 100 fixtures, got {success_count}"
    );
}

#[test]
fn test_roundtrip_musicxml_fixture_to_ly_and_back() {
    // MusicXML -> IR -> LilyPond -> IR (re-parse) -> IR -> MusicXML
    // Verify the full round-trip produces non-empty output with
    // matching part counts.
    use crate::adapters::ir_to_mxml::IrToMxmlAdapter;
    use crate::adapters::ly_to_ir::LyToIrAdapter;
    use crate::adapters::mxml_to_ir::MxmlToIrAdapter;
    use crate::adapters::{FromIrAdapter, ToIrAdapter};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("musicxml")
        .join("ross_32_Rossini_Duetto_for_Cello_and_Bass_mvt.1.musicxml");
    if !fixture.exists() {
        return;
    }

    // MusicXML -> IR
    let mxml_to_ir = MxmlToIrAdapter::new();
    let score1 = mxml_to_ir.convert_file(&fixture).unwrap();
    let part_count = score1.parts().len();

    // IR -> LilyPond
    let ir_to_ly = IrToLyAdapter::new();
    let ly = ir_to_ly.convert(&score1).unwrap();
    assert!(ly.contains("\\version"));
    assert!(ly.len() > 500, "LilyPond output unexpectedly short");

    // LilyPond -> IR (re-parse)
    let ly_to_ir = LyToIrAdapter::new();
    let score2 = ly_to_ir.convert_str(&ly).unwrap();
    assert!(!score2.parts().is_empty(), "re-parsed IR should have parts");

    // IR -> MusicXML
    let ir_to_mxml = IrToMxmlAdapter::new();
    let mxml = ir_to_mxml.convert(&score2).unwrap();
    assert!(mxml.contains("<score-partwise"));
    assert!(mxml.contains("<part "));

    // Part count should match
    assert_eq!(
        score2.parts().len(),
        part_count,
        "re-parsed score should have same number of parts"
    );
}

#[test]
fn test_emit_tuplet() {
    use crate::ir::articulation::TupletDisplay;
    // Build a score with 3 notes in a 3/2 tuplet
    let notes: Vec<VoiceElement> = (0..3)
        .map(|i| {
            let mut n = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
            n.duration.tuplet_actual = 3;
            n.duration.tuplet_normal = 2;
            if i == 0 {
                n.tuplet = Some(TupletDisplay {
                    tuplet_type: StartStop::Start,
                    bracket: true,
                    show_number: "actual".to_string(),
                });
            } else if i == 2 {
                n.tuplet = Some(TupletDisplay {
                    tuplet_type: StartStop::Stop,
                    bracket: true,
                    show_number: String::new(),
                });
            }
            VoiceElement::Note(Box::new(n))
        })
        .collect();

    let voice = Voice {
        number: 1,
        elements: notes,
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\tuplet 3/2"),
        "should emit \\tuplet 3/2: {}",
        ly
    );
}

#[test]
fn test_emit_acciaccatura() {
    let mut n = Note::new(
        Pitch::new(PitchStep::E, 5),
        Duration::new(Ratio::new(1, 16)),
    );
    n.is_grace = true;
    n.grace_slash = true;
    let main = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n)),
            VoiceElement::Note(Box::new(main)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\acciaccatura"),
        "should emit \\acciaccatura: {}",
        ly
    );
}

#[test]
fn test_emit_grace_not_acciaccatura() {
    let mut n = Note::new(
        Pitch::new(PitchStep::E, 5),
        Duration::new(Ratio::new(1, 16)),
    );
    n.is_grace = true;
    n.grace_slash = false;
    let main = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n)),
            VoiceElement::Note(Box::new(main)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\appoggiatura"),
        "should emit \\appoggiatura: {}",
        ly
    );
    assert!(
        !ly.contains("\\acciaccatura"),
        "should NOT emit \\acciaccatura: {}",
        ly
    );
}

#[test]
fn test_emit_glissando() {
    let mut n = make_note(PitchStep::C, 4, Duration::quarter());
    n.glissando = Some(StartStop::Start);
    let n2 = make_note(PitchStep::E, 4, Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n)),
            VoiceElement::Note(Box::new(n2)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\glissando"),
        "should emit \\glissando: {}",
        ly
    );
}

#[test]
fn test_emit_glissando_dashed_style() {
    let mut n = make_note(PitchStep::C, 4, Duration::quarter());
    n.glissando = Some(StartStop::Start);
    n.glissando_line_type = Some("dashed".to_string());
    let n2 = make_note(PitchStep::E, 4, Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n)),
            VoiceElement::Note(Box::new(n2)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("Glissando.style = #'dashed-line"),
        "should emit dashed-line override: {}",
        ly
    );
}

#[test]
fn test_emit_slide() {
    let mut n = make_note(PitchStep::C, 4, Duration::quarter());
    n.slide = Some(StartStop::Start);
    let n2 = make_note(PitchStep::E, 4, Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n)),
            VoiceElement::Note(Box::new(n2)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\glissando"),
        "slide should emit \\glissando: {}",
        ly
    );
}

#[test]
fn test_emit_arpeggio() {
    use crate::ir::note::{ArpeggioType, Chord};
    let notes = vec![
        make_note(PitchStep::C, 4, Duration::quarter()),
        make_note(PitchStep::E, 4, Duration::quarter()),
        make_note(PitchStep::G, 4, Duration::quarter()),
    ];
    let mut chord = Chord::new(Duration::quarter(), notes);
    chord.arpeggio = Some(ArpeggioType::Up);
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Chord(chord)],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\arpeggioArrowUp"),
        "should emit \\arpeggioArrowUp: {}",
        ly
    );
    assert!(ly.contains("\\arpeggio"), "should emit \\arpeggio: {}", ly);
}

#[test]
fn test_emit_non_arpeggio() {
    use crate::ir::note::{ArpeggioType, Chord};
    let notes = vec![
        make_note(PitchStep::C, 4, Duration::quarter()),
        make_note(PitchStep::E, 4, Duration::quarter()),
    ];
    let mut chord = Chord::new(Duration::quarter(), notes);
    chord.arpeggio = Some(ArpeggioType::NonArpeggio);
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Chord(chord)],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\arpeggioBracket"),
        "should emit \\arpeggioBracket: {}",
        ly
    );
}

#[test]
fn test_emit_after_grace() {
    let mut n = make_note(PitchStep::D, 5, Duration::sixteenth());
    n.is_grace = true;
    n.after_grace = true;
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(n))],
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\afterGrace"),
        "should emit \\afterGrace: {}",
        ly
    );
}

#[test]
fn test_emit_coda_segno() {
    use crate::ir::direction::Direction;
    let mut dir = Direction::default();
    dir.coda = true;
    let mut dir2 = Direction::default();
    dir2.segno = true;
    let mut measure = Measure::new(1);
    measure.directions.push(dir);
    measure.directions.push(dir2);
    measure.voices.push(Voice {
        number: 1,
        elements: vec![],
    });
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("scripts.coda"),
        "should emit coda markup: {}",
        ly
    );
    assert!(
        ly.contains("scripts.segno"),
        "should emit segno markup: {}",
        ly
    );
}

#[test]
fn test_emit_da_capo_dal_segno() {
    use crate::ir::direction::Direction;
    let mut dir = Direction::default();
    dir.da_capo = Some("D.C.".to_string());
    let mut dir2 = Direction::default();
    dir2.dal_segno = Some("D.S. al Coda".to_string());
    let mut measure = Measure::new(1);
    measure.directions.push(dir);
    measure.directions.push(dir2);
    measure.voices.push(Voice {
        number: 1,
        elements: vec![],
    });
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(ly.contains("\\mark \"D.C.\""), "should emit D.C.: {}", ly);
    assert!(
        ly.contains("\\mark \"D.S. al Coda\""),
        "should emit D.S. al Coda: {}",
        ly
    );
}

#[test]
fn test_emit_partial_anacrusis() {
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(make_note(
            PitchStep::G,
            4,
            Duration::quarter(),
        )))],
    };
    let mut measure = Measure::new(0);
    measure.voices.push(voice);
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.metadata.partial_duration = Some(Duration::quarter());
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\partial 4"),
        "should emit \\partial 4: {}",
        ly
    );
}

#[test]
fn test_emit_paper_block() {
    use crate::ir::score::PageLayout;
    let mut score = make_simple_score();
    score.page_layout = Some(PageLayout {
        page_height: Some(29.7),
        page_width: Some(21.0),
        left_margin: Some(1.5),
        right_margin: None,
        top_margin: None,
        bottom_margin: None,
        system_distance: None,
        top_system_distance: None,
        staff_size: Some(20.0),
    });
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("#(set-global-staff-size 20.0)"),
        "should emit staff size: {}",
        ly
    );
    assert!(ly.contains("\\paper {"), "should emit paper block: {}", ly);
    assert!(
        ly.contains("page-height = 29.70\\cm"),
        "should emit page height: {}",
        ly
    );
    assert!(
        ly.contains("page-width = 21.00\\cm"),
        "should emit page width: {}",
        ly
    );
    assert!(
        ly.contains("left-margin = 1.50\\cm"),
        "should emit left margin: {}",
        ly
    );
}

#[test]
fn test_emit_harmony_chordnames() {
    use crate::ir::harmony::{ChordPitch, Harmony};
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        ..Default::default()
    });
    measure.harmonies.push(Harmony {
        root: ChordPitch {
            step: "C".to_string(),
            alter: 0.0,
        },
        kind: "major".to_string(),
        bass: None,
        degrees: vec![],
        offset: 0,
    });
    measure.voices.push(Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(make_note(
            PitchStep::C,
            4,
            Duration::whole(),
        )))],
    });
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(ly.contains("\\chordmode"), "should emit chordmode: {}", ly);
    assert!(
        ly.contains("ChordNames"),
        "should emit ChordNames context: {}",
        ly
    );
}

#[test]
fn test_emit_harmony_minor_with_bass() {
    use crate::ir::harmony::{ChordPitch, Harmony};
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        ..Default::default()
    });
    measure.harmonies.push(Harmony {
        root: ChordPitch {
            step: "D".to_string(),
            alter: 0.0,
        },
        kind: "minor".to_string(),
        bass: Some(ChordPitch {
            step: "F".to_string(),
            alter: 0.0,
        }),
        degrees: vec![],
        offset: 0,
    });
    measure.voices.push(Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(make_note(
            PitchStep::D,
            4,
            Duration::whole(),
        )))],
    });
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(ly.contains("d:m/f"), "should emit d:m/f for Dm/F: {}", ly);
}

#[test]
fn test_emit_figured_bass() {
    use crate::ir::harmony::{Figure, FiguredBass};
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        ..Default::default()
    });
    measure.figured_bass.push(FiguredBass {
        figures: vec![
            Figure {
                number: Some(6),
                prefix: None,
                suffix: None,
            },
            Figure {
                number: Some(4),
                prefix: None,
                suffix: None,
            },
        ],
        duration: Duration::whole(),
        parentheses: false,
        offset: 0,
    });
    measure.voices.push(Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(make_note(
            PitchStep::C,
            3,
            Duration::whole(),
        )))],
    });
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\figuremode"),
        "should emit figuremode: {}",
        ly
    );
    assert!(
        ly.contains("FiguredBass"),
        "should emit FiguredBass context: {}",
        ly
    );
    assert!(ly.contains("<6 4>"), "should emit <6 4> figures: {}", ly);
}

#[test]
fn test_helper_harmony_kind_to_ly() {
    assert_eq!(harmony_kind_to_ly("major"), "");
    assert_eq!(harmony_kind_to_ly("minor"), ":m");
    assert_eq!(harmony_kind_to_ly("dominant"), ":7");
    assert_eq!(harmony_kind_to_ly("major-seventh"), ":maj7");
    assert_eq!(harmony_kind_to_ly("diminished"), ":dim");
    assert_eq!(harmony_kind_to_ly("augmented"), ":aug");
    assert_eq!(harmony_kind_to_ly("suspended-fourth"), ":sus4");
}

#[test]
fn test_helper_figure_to_ly() {
    use crate::ir::harmony::Figure;
    assert_eq!(
        figure_to_ly(&Figure {
            number: Some(6),
            prefix: None,
            suffix: None
        }),
        "6"
    );
    assert_eq!(
        figure_to_ly(&Figure {
            number: Some(6),
            prefix: None,
            suffix: Some("sharp".to_string())
        }),
        "6+"
    );
    assert_eq!(
        figure_to_ly(&Figure {
            number: None,
            prefix: None,
            suffix: None
        }),
        "_"
    );
}

/// Tuplet starting on a rest should emit `\tuplet` wrapper (regression).
#[test]
fn test_tuplet_starting_on_rest() {
    use crate::ir::articulation::TupletDisplay;
    // Build a 6/4 sextuplet: rest + 5 notes
    let mut rest = Rest::new(Duration::new(Ratio::new(1, 16)));
    rest.duration.tuplet_actual = 6;
    rest.duration.tuplet_normal = 4;
    rest.tuplet = Some(TupletDisplay {
        tuplet_type: StartStop::Start,
        bracket: true,
        show_number: "actual".to_string(),
    });

    let mut elements: Vec<VoiceElement> = vec![VoiceElement::Rest(rest)];
    for i in 0..5u8 {
        let step = match i {
            0 => PitchStep::A,
            1 => PitchStep::B,
            2 => PitchStep::C,
            3 => PitchStep::D,
            _ => PitchStep::E,
        };
        let mut n = Note::new(
            Pitch::new(step, 4 + (i / 3) as i32),
            Duration::new(Ratio::new(1, 16)),
        );
        n.duration.tuplet_actual = 6;
        n.duration.tuplet_normal = 4;
        if i == 4 {
            n.tuplet = Some(TupletDisplay {
                tuplet_type: StartStop::Stop,
                bracket: true,
                show_number: String::new(),
            });
        }
        elements.push(VoiceElement::Note(Box::new(n)));
    }

    let voice = Voice {
        number: 1,
        elements,
    };
    let mut measure = Measure::new(1);
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();
    assert!(
        ly.contains("\\tuplet 6/4"),
        "tuplet starting on rest should emit \\tuplet 6/4: {}",
        ly
    );
}

/// Wedge directions should attach to the correct note based on offset,
/// not all to the first note (regression).
#[test]
fn test_wedge_position_aware_attachment() {
    use crate::ir::articulation::{DynamicMark, Wedge};
    use crate::ir::direction::Direction;

    // Build 4 quarter-note chords in 4/4 at divisions=4
    let mut elements: Vec<VoiceElement> = Vec::new();
    for _ in 0..4 {
        let n = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        elements.push(VoiceElement::Note(Box::new(n)));
    }

    let voice = Voice {
        number: 1,
        elements,
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        divisions: 4,
        time: Some(TimeSignature::default()),
        key: Some(KeySignature::default()),
        clefs: std::collections::HashMap::new(),
        staves: None,
        staff_lines: None,
        transpose: None,
    });
    measure.voices.push(voice);

    // Dynamic \p at offset 0 (before note 1)
    measure.directions.push(Direction {
        offset: 0,
        dynamic: Some(DynamicMark {
            sign: "p".to_string(),
            placement: Placement::Below,
        }),
        ..Direction::default()
    });
    // Crescendo start at offset 4 (before note 2)
    measure.directions.push(Direction {
        offset: 4,
        wedge: Some(Wedge {
            wedge_type: "crescendo".to_string(),
            placement: Placement::Below,
        }),
        ..Direction::default()
    });
    // Wedge stop at offset 12 (before note 4)
    measure.directions.push(Direction {
        offset: 12,
        wedge: Some(Wedge {
            wedge_type: "stop".to_string(),
            placement: Placement::Below,
        }),
        ..Direction::default()
    });

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    // \p should be on first note only, not together with \< or \!
    assert!(
        ly.contains("c'4\\p"),
        "dynamic should be on first note: {}",
        ly
    );
    assert!(
        ly.contains("c'4\\<"),
        "crescendo should be on second note: {}",
        ly
    );
    assert!(
        ly.contains("c'4\\!"),
        "wedge stop should be on fourth note: {}",
        ly
    );
    // Must NOT have all directions on first note
    assert!(
        !ly.contains("\\p\\<"),
        "dynamics and wedge should not all be on same note: {}",
        ly
    );
}

#[test]
fn test_lyrics_emission() {
    // Build a score with lyrics on notes
    let mut n1 = make_note(PitchStep::C, 4, Duration::quarter());
    n1.lyrics.push(LyricSyllable {
        text: "Hel".to_string(),
        syllabic: SyllabicType::Begin,
        number: 1,
        extend: false,
        elision: false,
    });
    let mut n2 = make_note(PitchStep::D, 4, Duration::quarter());
    n2.lyrics.push(LyricSyllable {
        text: "lo".to_string(),
        syllabic: SyllabicType::End,
        number: 1,
        extend: false,
        elision: false,
    });
    let mut n3 = make_note(PitchStep::E, 4, Duration::quarter());
    n3.lyrics.push(LyricSyllable {
        text: "world".to_string(),
        syllabic: SyllabicType::Single,
        number: 1,
        extend: false,
        elision: false,
    });
    let n4 = make_note(PitchStep::F, 4, Duration::quarter());

    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n1)),
            VoiceElement::Note(Box::new(n2)),
            VoiceElement::Note(Box::new(n3)),
            VoiceElement::Note(Box::new(n4)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.name = "Soprano".to_string();
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    // Should have a lyrics variable with lyricmode
    assert!(
        ly.contains("\\lyricmode"),
        "should contain \\lyricmode: {ly}"
    );
    // Should have syllable with hyphens
    assert!(ly.contains("Hel --"), "should contain 'Hel --': {ly}");
    assert!(ly.contains("lo"), "should contain 'lo': {ly}");
    assert!(ly.contains("world"), "should contain 'world': {ly}");
    // Should have \lyricsto reference
    assert!(ly.contains("\\lyricsto"), "should contain \\lyricsto: {ly}");
    // Should have named Voice
    assert!(
        ly.contains("\\new Voice ="),
        "should contain named Voice: {ly}"
    );
}

#[test]
fn test_melisma_emission() {
    // Build a score with melisma notes
    let mut n1 = make_note(PitchStep::C, 4, Duration::quarter());
    n1.lyrics.push(LyricSyllable {
        text: "word".to_string(),
        syllabic: SyllabicType::Single,
        number: 1,
        extend: false,
        elision: false,
    });
    let mut n2 = make_note(PitchStep::D, 4, Duration::quarter());
    n2.in_melisma = true; // melisma
    let mut n3 = make_note(PitchStep::E, 4, Duration::quarter());
    n3.in_melisma = true; // still melisma
    let mut n4 = make_note(PitchStep::F, 4, Duration::quarter());
    n4.lyrics.push(LyricSyllable {
        text: "next".to_string(),
        syllabic: SyllabicType::Single,
        number: 1,
        extend: false,
        elision: false,
    });

    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n1)),
            VoiceElement::Note(Box::new(n2)),
            VoiceElement::Note(Box::new(n3)),
            VoiceElement::Note(Box::new(n4)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    // Should emit \melisma and \melismaEnd
    assert!(ly.contains("\\melisma"), "should contain \\melisma: {ly}");
    assert!(
        ly.contains("\\melismaEnd"),
        "should contain \\melismaEnd: {ly}"
    );
    // Lyrics should only have "word" and "next" (no skips for melisma notes)
    assert!(ly.contains("word"), "should contain 'word': {ly}");
    assert!(ly.contains("next"), "should contain 'next': {ly}");
}

#[test]
fn test_auto_beam_off_emission() {
    // Build a score with no_auto_beam notes
    let mut n1 = make_note(PitchStep::C, 4, Duration::eighth());
    n1.no_auto_beam = true;
    let mut n2 = make_note(PitchStep::D, 4, Duration::eighth());
    n2.no_auto_beam = true;
    let n3 = make_note(PitchStep::E, 4, Duration::eighth()); // autoBeamOn
    let n4 = make_note(PitchStep::F, 4, Duration::eighth());

    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n1)),
            VoiceElement::Note(Box::new(n2)),
            VoiceElement::Note(Box::new(n3)),
            VoiceElement::Note(Box::new(n4)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    // Should emit \autoBeamOff before first no_auto_beam note
    assert!(
        ly.contains("\\autoBeamOff"),
        "should contain \\autoBeamOff: {ly}"
    );
    // Should emit \autoBeamOn when reverting
    assert!(
        ly.contains("\\autoBeamOn"),
        "should contain \\autoBeamOn: {ly}"
    );
}

#[test]
fn test_accidental_display_emission() {
    let mut n1 = make_note(PitchStep::C, 4, Duration::quarter());
    n1.pitch.alter = crate::ir::pitch::Alter::from_integer(1); // C#
    n1.pitch.accidental = crate::ir::pitch::AccidentalDisplay::Forced;
    let mut n2 = make_note(PitchStep::D, 4, Duration::quarter());
    n2.pitch.accidental = crate::ir::pitch::AccidentalDisplay::Cautionary;
    let n3 = make_note(PitchStep::E, 4, Duration::quarter());

    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n1)),
            VoiceElement::Note(Box::new(n2)),
            VoiceElement::Note(Box::new(n3)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    // Default output language is Nederlands, so C# = "cis"
    assert!(
        ly.contains("cis'!"),
        "should emit forced accidental '!' after cis': {ly}"
    );
    assert!(
        ly.contains("d'?"),
        "should emit cautionary accidental '?' after d': {ly}"
    );
}

#[test]
fn test_layout_break_emission() {
    use crate::ir::direction::{Direction, LayoutBreakType};

    let n1 = make_note(PitchStep::C, 4, Duration::whole());
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(n1))],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);
    measure.directions.push(Direction {
        layout_break: Some(LayoutBreakType::System),
        ..Default::default()
    });

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    assert!(ly.contains("\\break"), "should emit \\break: {ly}");
}

#[test]
fn test_pedal_emission() {
    use crate::ir::direction::{Direction, PedalEvent};

    let n1 = make_note(PitchStep::C, 4, Duration::quarter());
    let n2 = make_note(PitchStep::D, 4, Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n1)),
            VoiceElement::Note(Box::new(n2)),
        ],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);
    measure.directions.push(Direction {
        pedal: Some(PedalEvent {
            pedal_type: "start".to_string(),
            line: false,
        }),
        ..Default::default()
    });
    measure.directions.push(Direction {
        offset: 1,
        pedal: Some(PedalEvent {
            pedal_type: "stop".to_string(),
            line: false,
        }),
        ..Default::default()
    });

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    assert!(ly.contains("\\sustainOn"), "should emit \\sustainOn: {ly}");
    assert!(
        ly.contains("\\sustainOff"),
        "should emit \\sustainOff: {ly}"
    );
}

#[test]
fn test_ottava_emission() {
    use crate::ir::direction::{Direction, OctaveShift};

    let n1 = make_note(PitchStep::C, 5, Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(n1))],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);
    measure.directions.push(Direction {
        octave_shift: Some(OctaveShift {
            shift_type: "up".to_string(),
            size: 8,
        }),
        ..Default::default()
    });

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    assert!(ly.contains("\\ottava #1"), "should emit \\ottava #1: {ly}");
}

#[test]
fn test_tremolo_emission() {
    let mut n1 = make_note(PitchStep::C, 4, Duration::quarter());
    n1.tremolo_marks = 3;
    n1.ornaments.push(crate::ir::articulation::Ornament {
        name: "tremolo".to_string(),
        placement: Default::default(),
    });

    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(n1))],
    };
    let mut measure = Measure::new(1);
    measure.attributes = Some(MeasureAttributes {
        time: Some(TimeSignature::default()),
        clefs: {
            let mut m = HashMap::new();
            m.insert(1, Clef::default());
            m
        },
        ..Default::default()
    });
    measure.voices.push(voice);

    let mut part = Part::new("P1");
    part.measures.push(measure);

    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToLyAdapter::new();
    let ly = adapter.convert(&score).unwrap();

    assert!(
        ly.contains(":32"),
        "should emit :32 for 3 tremolo marks on quarter note: {ly}"
    );
}
