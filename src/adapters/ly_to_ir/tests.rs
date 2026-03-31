#[cfg(test)]
mod tests {
    use num::rational::Ratio;

    use crate::adapters::ly_to_ir::LyToIrAdapter;
    use crate::adapters::ToIrAdapter;
    use crate::ir::articulation::{StartStop, SyllabicType};
    use crate::ir::direction::BarlineType;
    use crate::ir::duration::Frac;
    use crate::ir::language::{PitchLanguage, PitchMode};
    use crate::ir::measure::ClefSign;
    use crate::ir::note::{ArpeggioType, Chord, Note, VoiceElement};
    use crate::ir::pitch::PitchStep;

    use super::super::merge::voice_element_duration;

    #[test]
    fn test_parse_simple_melody() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4 d' e' f' }"#).unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty());
        let part = &parts[0];
        assert!(!part.measures.is_empty());

        // Should have 4 notes
        let notes: Vec<&Note> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[0].pitch.octave, 4);
        assert_eq!(notes[1].pitch.step, PitchStep::D);
        assert_eq!(notes[2].pitch.step, PitchStep::E);
        assert_eq!(notes[3].pitch.step, PitchStep::F);
    }

    #[test]
    fn test_parse_with_key_time_clef() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
  \new Staff {
    \key g \major
    \time 3/4
    \clef treble
    c'4 d' e' |
  }
}"#,
            )
            .unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty());
        let part = &parts[0];
        let measure = &part.measures[0];

        // Check attributes
        let attrs = measure.attributes.as_ref().unwrap();
        assert_eq!(attrs.key.as_ref().unwrap().fifths, 1); // G major = 1 sharp
        assert_eq!(attrs.time.as_ref().unwrap().beats, "3");
        assert_eq!(attrs.time.as_ref().unwrap().beat_type, 4);
        assert!(attrs.clefs.contains_key(&1));
        assert_eq!(attrs.clefs[&1].sign, ClefSign::G);
    }

    #[test]
    fn test_parse_relative_pitch() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
  \new Staff \relative c' {
    c4 d e f |
    g a b c |
  }
}"#,
            )
            .unwrap();

        let parts = score.parts();
        let part = &parts[0];
        let notes: Vec<&Note> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        // In relative mode starting from c': c d e f g a b c
        // c=4, d=4, e=4, f=4, g=4, a=4, b=4, c=5
        assert!(notes.len() >= 8, "Expected 8 notes, got {}", notes.len());
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[0].pitch.octave, 4);
        assert_eq!(notes[4].pitch.step, PitchStep::G);
        assert_eq!(notes[4].pitch.octave, 4);
        assert_eq!(notes[7].pitch.step, PitchStep::C);
        assert_eq!(notes[7].pitch.octave, 5);
    }

    #[test]
    fn test_parse_rests_and_spacers() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ r4 R1 s2 }"#).unwrap();

        let parts = score.parts();
        let part = &parts[0];
        let elems: Vec<&VoiceElement> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 3);
        match &elems[0] {
            VoiceElement::Rest(r) => {
                assert!(!r.is_measure_rest);
                assert!(!r.is_spacer);
            }
            _ => panic!("expected Rest"),
        }
        match &elems[1] {
            VoiceElement::Rest(r) => assert!(r.is_measure_rest),
            _ => panic!("expected measure Rest"),
        }
        match &elems[2] {
            VoiceElement::Rest(r) => assert!(r.is_spacer),
            _ => panic!("expected spacer Rest"),
        }
    }

    #[test]
    fn test_parse_chord() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ <c' e' g'>4 }"#).unwrap();

        let parts = score.parts();
        let part = &parts[0];
        let elems: Vec<&VoiceElement> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 1);
        match &elems[0] {
            VoiceElement::Chord(c) => {
                assert_eq!(c.notes.len(), 3);
                assert_eq!(c.notes[0].pitch.step, PitchStep::C);
                assert_eq!(c.notes[1].pitch.step, PitchStep::E);
                assert_eq!(c.notes[2].pitch.step, PitchStep::G);
            }
            _ => panic!("expected Chord"),
        }
    }

    #[test]
    fn test_parse_header() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\header {
  title = "My Piece"
  composer = "J. S. Bach"
}
{ c'4 }"#,
            )
            .unwrap();

        assert_eq!(score.metadata.title.as_deref(), Some("My Piece"));
        assert_eq!(score.metadata.composer.as_deref(), Some("J. S. Bach"));
    }

    #[test]
    fn test_parse_dynamics_and_ties() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4\f~ c' d'\< e'\! }"#).unwrap();

        let parts = score.parts();
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 4);
        // c' has \f dynamic
        assert!(!notes[0].dynamics.is_empty());
        assert_eq!(notes[0].dynamics[0].sign, "f");
        // c' has tie
        assert!(!notes[0].ties.is_empty());
        // e' has \! (wedge stop)
        assert!(!notes[3].wedges.is_empty());
        assert_eq!(notes[3].wedges[0].wedge_type, "stop");
    }

    #[test]
    fn test_parse_language_english() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\language "english"
{ cs'4 ef' fs' bf' }"#,
            )
            .unwrap();

        let notes: Vec<&Note> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 4);
        // cs = C#
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[0].pitch.alter, Ratio::new(1, 1));
        // ef = Eb
        assert_eq!(notes[1].pitch.step, PitchStep::E);
        assert_eq!(notes[1].pitch.alter, Ratio::new(-1, 1));
    }

    #[test]
    fn test_parse_grace_note() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ \grace { e'16 } c'4 }"#).unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert!(elems.len() >= 2);
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace);
                assert_eq!(n.pitch.step, PitchStep::E);
            }
            _ => panic!("expected grace Note"),
        }
        match &elems[1] {
            VoiceElement::Note(n) => {
                assert!(!n.is_grace);
                assert_eq!(n.pitch.step, PitchStep::C);
            }
            _ => panic!("expected non-grace Note"),
        }
    }

    #[test]
    fn test_parse_fixture_file() {
        let path = std::path::Path::new("tests/fixtures/ly/rest-dynamic.ly");
        if !path.exists() {
            return;
        }
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_file(path).unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty());
        let part = &parts[0];
        assert!(!part.measures.is_empty());
    }

    #[test]
    fn test_parse_bar_checks_create_measures() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' | g' a' b' c'' | }"#)
            .unwrap();

        let part = &score.parts()[0];
        // Should have at least 2 measures from bar checks
        assert!(
            part.measures.len() >= 2,
            "Expected >= 2 measures, got {}",
            part.measures.len()
        );
    }

    #[test]
    fn test_parse_variable_definition_and_reference() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
melody = { c'4 d' e' f' }

\score {
  \new Staff \melody
}
"#,
            )
            .unwrap();

        let parts = score.parts();
        assert!(!parts.is_empty(), "Should have at least one part");
        let part = &parts[0];

        let notes: Vec<&Note> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(
            notes.len(),
            4,
            "Variable \\melody should resolve to 4 notes"
        );
        assert_eq!(notes[0].pitch.step, PitchStep::C);
        assert_eq!(notes[3].pitch.step, PitchStep::F);
    }

    #[test]
    fn test_parse_multiple_variable_references() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
partA = { c'4 d' e' f' }
partB = { g'4 a' b' c'' }

\score {
  <<
    \new Staff \partA
    \new Staff \partB
  >>
}
"#,
            )
            .unwrap();

        let parts = score.parts();
        assert_eq!(parts.len(), 2, "Should have two parts from two \\new Staff");

        let notes_a: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes_a.len(), 4);
        assert_eq!(notes_a[0].pitch.step, PitchStep::C);

        let notes_b: Vec<&Note> = parts[1]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes_b.len(), 4);
        assert_eq!(notes_b[0].pitch.step, PitchStep::G);
    }

    #[test]
    fn test_pitch_mode_preserved_in_metadata() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"\relative c' { c4 d e f }"#).unwrap();

        assert_eq!(
            score.metadata.pitch_mode,
            PitchMode::Relative,
            "PitchMode should be Relative when \\relative is used"
        );
    }

    #[test]
    fn test_pitch_language_preserved_in_metadata() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"\language "english" { cs'4 df' ef' fs' }"#)
            .unwrap();

        assert_eq!(
            score.metadata.pitch_language,
            Some(PitchLanguage::English),
            "PitchLanguage should be English"
        );
    }

    #[test]
    fn test_parse_alphabetic_var_names() {
        let input = r#"pA = { c'4 d' e' f' }
pB = { g4 a b c' }
\score {
  <<
    \new Staff \pA
    \new Staff \pB
  >>
  \layout {}
}
"#;
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(input).unwrap();
        assert_eq!(
            score.parts().len(),
            2,
            "Expected 2 parts from 2 variable references in score block"
        );
    }

    #[test]
    fn test_parse_acciaccatura() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \acciaccatura { e'16 } c'4 }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert!(elems.len() >= 2);
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace);
                assert!(n.grace_slash, "acciaccatura should set grace_slash=true");
            }
            _ => panic!("expected grace Note"),
        }
    }

    #[test]
    fn test_parse_grace_not_slash() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ \grace { d'16 } c'4 }"#).unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace);
                assert!(!n.grace_slash, "\\grace should set grace_slash=false");
            }
            _ => panic!("expected grace Note"),
        }
    }

    #[test]
    fn test_parse_tuplet() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \tuplet 3/2 { c'4 d' e' } }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 3, "tuplet should produce 3 elements");
        for elem in &elems {
            match elem {
                VoiceElement::Note(n) => {
                    assert_eq!(n.duration.tuplet_actual, 3);
                    assert_eq!(n.duration.tuplet_normal, 2);
                }
                _ => panic!("expected Note in tuplet"),
            }
        }
        // First element should have TupletDisplay::Start
        match &elems[0] {
            VoiceElement::Note(n) => {
                let td = n
                    .tuplet
                    .as_ref()
                    .expect("first note should have tuplet display");
                assert_eq!(td.tuplet_type, StartStop::Start);
            }
            _ => {}
        }
        // Last element should have TupletDisplay::Stop
        match &elems[2] {
            VoiceElement::Note(n) => {
                let td = n
                    .tuplet
                    .as_ref()
                    .expect("last note should have tuplet display");
                assert_eq!(td.tuplet_type, StartStop::Stop);
            }
            _ => {}
        }
    }

    #[test]
    fn test_parse_times_old_syntax() {
        let adapter = LyToIrAdapter::new();
        // \times has reversed fraction: normal/actual
        let score = adapter
            .convert_str(r#"{ \times 2/3 { c'4 d' e' } }"#)
            .unwrap();

        let elems: Vec<&VoiceElement> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();

        assert_eq!(elems.len(), 3);
        match &elems[0] {
            VoiceElement::Note(n) => {
                // \times 2/3 means normal=2, actual=3 -> reversed to actual=3, normal=2
                assert_eq!(n.duration.tuplet_actual, 3);
                assert_eq!(n.duration.tuplet_normal, 2);
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn test_sextuplet_6_4() {
        // \tuplet 6/4 { r16 a'16 b'16 cis''16 d''16 e''16 } = 6 16ths in time of 4 16ths = 1 quarter
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 c'4 r4 r4 \tuplet 6/4 { r16 a'16 b'16 cis''16 d''16 e''16 } }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        // Should be exactly 1 measure (3 quarters + sextuplet = 1 quarter = 4/4)
        assert_eq!(
            part.measures.len(),
            1,
            "sextuplet should fit in one measure, got {} measures",
            part.measures.len()
        );

        let elems: Vec<&VoiceElement> = part.measures[0].voices[0].elements.iter().collect();
        // c'4 r4 r4 + 6 tuplet notes = 9 elements
        assert_eq!(elems.len(), 9, "expected 9 elements, got {}", elems.len());

        // Check that the tuplet rest and notes have tuplet_actual=6, tuplet_normal=4
        for elem in &elems[3..9] {
            match elem {
                VoiceElement::Note(n) => {
                    assert_eq!(
                        n.duration.tuplet_actual, 6,
                        "note should have tuplet_actual=6"
                    );
                    assert_eq!(
                        n.duration.tuplet_normal, 4,
                        "note should have tuplet_normal=4"
                    );
                }
                VoiceElement::Rest(r) => {
                    assert_eq!(
                        r.duration.tuplet_actual, 6,
                        "rest should have tuplet_actual=6"
                    );
                    assert_eq!(
                        r.duration.tuplet_normal, 4,
                        "rest should have tuplet_normal=4"
                    );
                }
                _ => panic!("unexpected element in tuplet"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Section 10: Extended feature parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_partial() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \partial 4 c'4 | d'2 e'2 }"#)
            .unwrap();

        assert!(score.metadata.partial_duration.is_some());
        let partial = score.metadata.partial_duration.as_ref().unwrap();
        assert_eq!(partial.actual_duration(), Ratio::new(1i64, 4));
    }

    #[test]
    fn test_parse_glissando() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4\glissando d'4 }"#).unwrap();

        let notes: Vec<&Note> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].glissando, Some(StartStop::Start));
        assert!(notes[0].slide.is_none());
    }

    #[test]
    fn test_parse_arpeggio_up() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \arpeggioArrowUp <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::Up));
    }

    #[test]
    fn test_parse_arpeggio_down() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \arpeggioArrowDown <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::Down));
    }

    #[test]
    fn test_parse_arpeggio_bracket() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \arpeggioBracket <c' e' g'>4\arpeggio }"#)
            .unwrap();

        let chords: Vec<&Chord> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::NonArpeggio));
    }

    #[test]
    fn test_parse_after_grace() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 \afterGrace { d'16 } }"#)
            .unwrap();

        let notes: Vec<&Note> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 2);
        // First note is normal
        assert!(!notes[0].is_grace);
        // Second note is after-grace
        assert!(notes[1].is_grace);
        assert!(notes[1].after_grace);
        assert_eq!(notes[1].pitch.step, PitchStep::D);
    }

    #[test]
    fn test_parse_mark_da_capo() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' \mark "D.C." }"#)
            .unwrap();

        let dirs: Vec<_> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        let dc_dir = dirs
            .iter()
            .find(|d| d.da_capo.is_some())
            .expect("expected D.C. direction");
        assert_eq!(dc_dir.da_capo.as_deref(), Some("D.C."));
    }

    #[test]
    fn test_parse_mark_dal_segno() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' \mark "D.S. al Coda" }"#)
            .unwrap();

        let dirs: Vec<_> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        let ds_dir = dirs
            .iter()
            .find(|d| d.dal_segno.is_some())
            .expect("expected D.S. direction");
        assert_eq!(ds_dir.dal_segno.as_deref(), Some("D.S. al Coda"));
    }

    #[test]
    fn test_parse_mark_coda() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 \mark \markup { \musicglyph "scripts.coda" } d'4 }"#)
            .unwrap();

        let dirs: Vec<_> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        assert!(dirs.iter().any(|d| d.coda), "expected coda direction");
    }

    #[test]
    fn test_parse_mark_segno() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 \mark \markup { \musicglyph "scripts.segno" } d'4 }"#)
            .unwrap();

        let dirs: Vec<_> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .collect();

        assert!(!dirs.is_empty());
        assert!(dirs.iter().any(|d| d.segno), "expected segno direction");
    }

    #[test]
    fn test_parse_paper_block() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\paper {
  paper-height = 29.70\cm
  paper-width = 21.00\cm
  left-margin = 2.00\cm
}
{ c'4 d' e' f' }"#,
            )
            .unwrap();

        let layout = score.page_layout.as_ref().expect("expected page_layout");
        assert!((layout.page_height.unwrap() - 29.70).abs() < 0.01);
        assert!((layout.page_width.unwrap() - 21.00).abs() < 0.01);
        assert!((layout.left_margin.unwrap() - 2.00).abs() < 0.01);
    }

    #[test]
    fn test_parse_arpeggio_default_up() {
        // Without explicit direction, \arpeggio should default to Up
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ <c' e' g'>4\arpeggio }"#).unwrap();

        let chords: Vec<&Chord> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Chord(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].arpeggio, Some(ArpeggioType::Up));
    }

    #[test]
    fn test_single_note_acciaccatura() {
        // \acciaccatura d''8 should produce a grace note without braces
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \acciaccatura d''8 c''4 }"#)
            .unwrap();

        let part = &score.parts()[0];
        let elems = &part.measures[0].voices[0].elements;

        // First element should be the grace note
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace, "first note should be a grace note");
                assert!(n.grace_slash, "acciaccatura should have slash");
            }
            other => panic!("expected Note, got {:?}", other),
        }
        // Second element should be the main note
        match &elems[1] {
            VoiceElement::Note(n) => {
                assert!(!n.is_grace, "second note should not be grace");
            }
            other => panic!("expected Note, got {:?}", other),
        }
    }

    #[test]
    fn test_single_note_grace() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ \grace e'16 c'4 }"#).unwrap();

        let part = &score.parts()[0];
        let elems = &part.measures[0].voices[0].elements;

        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace, "first note should be grace");
                assert!(!n.grace_slash, "\\grace should not have slash");
            }
            other => panic!("expected Note, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Regression tests for example.ly features
    // -----------------------------------------------------------------------

    #[test]
    fn test_staff_variable_with_new_staff() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
staffA = \new Staff {
  \set Staff.instrumentName = "Violin"
  \set Staff.midiInstrument = "violin"
  \key c \major
  \clef treble
  \relative c' { c4 d e f | }
}
staffB = \new Staff {
  \set Staff.instrumentName = "Cello"
  \set Staff.midiInstrument = "cello"
  \key c \major
  \clef bass
  \relative c { c4 d e f | }
}
\score { << \staffA \staffB >> }
"#,
            )
            .unwrap();

        let parts = score.parts();
        assert_eq!(parts.len(), 2, "should have 2 parts from 2 staff variables");
        assert_eq!(parts[0].name, "Violin");
        assert_eq!(parts[0].midi_instrument, "violin");
        assert_eq!(parts[1].name, "Cello");
        assert_eq!(parts[1].midi_instrument, "cello");
        // Check notes exist
        let notes_a: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes_a.len(), 4);
    }

    #[test]
    fn test_set_instrument_name() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\new Staff {
  \set Staff.instrumentName = "Trumpet"
  \set Staff.midiInstrument = "trumpet"
  c'4 d' e' f'
}"#,
            )
            .unwrap();
        let part = &score.parts()[0];
        assert_eq!(part.name, "Trumpet");
        assert_eq!(part.midi_instrument, "trumpet");
    }

    #[test]
    fn test_context_voice_named() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\new Staff {
  \context Voice = "melody" { c'4 d' e' f' }
}"#,
            )
            .unwrap();
        let parts = score.parts();
        assert_eq!(
            parts.len(),
            1,
            "should be 1 part, Voice doesn't create a new part"
        );
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 4);
    }

    #[test]
    fn test_lyrics_variable_and_lyricsto() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"
verse = \lyricmode { hel -- lo world }
staffSop = \new Staff {
  \context Voice = "sop" { c'4 d' e' }
}
\score {
  <<
    \staffSop
    \context Lyrics = "lsop" \lyricmode { \lyricsto "sop" \verse }
  >>
}
"#,
            )
            .unwrap();
        let parts = score.parts();
        assert_eq!(
            parts.len(),
            1,
            "Lyrics context should not create an extra part"
        );
        // Check that lyrics were attached to notes
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert!(notes.len() >= 3, "should have at least 3 notes");
        // First note should have lyric "hel"
        assert!(
            !notes[0].lyrics.is_empty(),
            "first note should have a lyric"
        );
        assert_eq!(notes[0].lyrics[0].text, "hel");
        assert_eq!(notes[0].lyrics[0].syllabic, SyllabicType::Begin);
    }

    #[test]
    fn test_six_part_score_from_variables() {
        let adapter = LyToIrAdapter::new().with_language(PitchLanguage::Deutsch);
        let source = std::fs::read_to_string("tests/fixtures/ly/example.ly").unwrap();
        let score = adapter.convert_str(&source).unwrap();

        let parts = score.parts();
        assert_eq!(parts.len(), 6, "example.ly should produce 6 parts");
        assert_eq!(parts[0].name, "Corno da Caccia");
        assert_eq!(parts[1].name, "Violino I");
        assert_eq!(parts[2].name, "Violino II");
        assert_eq!(parts[3].name, "Viola");
        assert_eq!(parts[4].name, "Soprano");
        assert_eq!(parts[5].name, "Basso");

        // Check MIDI instruments
        assert_eq!(parts[0].midi_instrument, "french horn");
        assert_eq!(parts[1].midi_instrument, "violin");
        assert_eq!(parts[3].midi_instrument, "viola");
        assert_eq!(parts[5].midi_instrument, "harpsichord");

        // Check that each part has measures
        for (i, part) in parts.iter().enumerate() {
            assert!(
                !part.measures.is_empty(),
                "Part {} ({}) should have measures",
                i,
                part.name
            );
        }

        // Check soprano part has lyrics
        let sop_notes: Vec<&Note> = parts[4]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        let notes_with_lyrics = sop_notes.iter().filter(|n| !n.lyrics.is_empty()).count();
        assert!(
            notes_with_lyrics > 0,
            "Soprano part should have notes with lyrics attached"
        );
    }

    #[test]
    fn test_cadenza_and_melisma_dont_crash() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"{
  \cadenzaOn c'2 \bar "|" \cadenzaOff
  c'4\melisma d' e'\melismaEnd f'
  \autoBeamOff c'8 d' e' f'
  \dynamicUp c'4\f d'\p
}"#,
            )
            .unwrap();
        let parts = score.parts();
        assert!(!parts.is_empty());
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        // Should have parsed all notes without crashing
        assert!(
            notes.len() >= 10,
            "should parse notes despite cadenza/melisma commands"
        );
    }

    #[test]
    fn test_ly_to_mxml_roundtrip_example() {
        // Test the full ly -> IR -> MusicXML pipeline doesn't lose parts
        let adapter = LyToIrAdapter::new().with_language(PitchLanguage::Deutsch);
        let source = std::fs::read_to_string("tests/fixtures/ly/example.ly").unwrap();
        let score = adapter.convert_str(&source).unwrap();

        let mxml_adapter = crate::adapters::ir_to_mxml::IrToMxmlAdapter::new();
        let xml = crate::adapters::FromIrAdapter::convert(&mxml_adapter, &score).unwrap();

        // Verify all 6 parts appear in XML
        let part_count = xml.matches("<score-part ").count();
        assert_eq!(
            part_count, 6,
            "MusicXML should have 6 <score-part> elements"
        );

        // Verify part names
        assert!(xml.contains("<part-name>Corno da Caccia</part-name>"));
        assert!(xml.contains("<part-name>Violino I</part-name>"));
        assert!(xml.contains("<part-name>Soprano</part-name>"));
        assert!(xml.contains("<part-name>Basso</part-name>"));

        // Verify MIDI instruments
        assert!(xml.contains("<midi-name>french horn</midi-name>"));
        assert!(xml.contains("<midi-name>violin</midi-name>"));

        // Verify lyrics in soprano part
        assert!(
            xml.contains("<lyric"),
            "Should contain lyrics in MusicXML output"
        );
        assert!(
            xml.contains("<text>Men</text>"),
            "Should contain first lyric syllable"
        );
    }

    #[test]
    fn test_figuremode_basic() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
bc = { c'1 | d'1 }
figs = \figuremode { <6 4>1 | <_+>1 }
\score { \new Staff <<\bc\figs>> }
"#;
        let score = adapter.convert_str(source).unwrap();
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        // Should have figured bass in the measures
        let total_figs: usize = parts[0].measures.iter().map(|m| m.figured_bass.len()).sum();
        assert!(total_figs > 0, "should have figured bass entries, got 0");

        // First measure should have figure [6, 4]
        let m1_figs = &parts[0].measures[0].figured_bass;
        assert_eq!(m1_figs.len(), 1);
        assert_eq!(m1_figs[0].figures.len(), 2);
        assert_eq!(m1_figs[0].figures[0].number, Some(6));
        assert_eq!(m1_figs[0].figures[1].number, Some(4));

        // Second measure should have figure [_+] (sharp on placeholder)
        if parts[0].measures.len() > 1 {
            let m2_figs = &parts[0].measures[1].figured_bass;
            assert_eq!(m2_figs.len(), 1);
            assert_eq!(m2_figs[0].figures.len(), 1);
            assert_eq!(m2_figs[0].figures[0].number, None);
            assert_eq!(m2_figs[0].figures[0].suffix.as_deref(), Some("sharp"));
        }
    }

    #[test]
    fn test_figuremode_accidentals() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
bc = { c'1 }
figs = \figuremode { <6+ 4->1 }
\score { \new Staff <<\bc\figs>> }
"#;
        let score = adapter.convert_str(source).unwrap();
        let parts = score.parts();
        let m1_figs = &parts[0].measures[0].figured_bass;
        assert_eq!(m1_figs.len(), 1);
        assert_eq!(m1_figs[0].figures.len(), 2);
        assert_eq!(m1_figs[0].figures[0].number, Some(6));
        assert_eq!(m1_figs[0].figures[0].suffix.as_deref(), Some("sharp"));
        assert_eq!(m1_figs[0].figures[1].number, Some(4));
        assert_eq!(m1_figs[0].figures[1].suffix.as_deref(), Some("flat"));
    }

    #[test]
    fn test_figuremode_distribution_across_measures() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
bc = { c'1 | d'1 | e'1 }
figs = \figuremode { <6>1 s1 <4 3>1 }
forma = { \time 4/4 \key c\major s1*3 }
\score { \new Staff { \clef bass <<\bc\forma\figs>> } }
"#;
        let score = adapter.convert_str(source).unwrap();
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        assert!(
            parts[0].measures.len() >= 3,
            "should have at least 3 measures"
        );
        // Measure 1: <6>
        assert_eq!(parts[0].measures[0].figured_bass.len(), 1);
        assert_eq!(
            parts[0].measures[0].figured_bass[0].figures[0].number,
            Some(6)
        );
        // Measure 2: skip (no figures)
        assert_eq!(parts[0].measures[1].figured_bass.len(), 0);
        // Measure 3: <4 3>
        assert_eq!(parts[0].measures[2].figured_bass.len(), 1);
        assert_eq!(parts[0].measures[2].figured_bass[0].figures.len(), 2);
    }

    #[test]
    fn test_cautionary_accidental_measure_split() {
        // Cautionary accidental `!` after pitch should not disrupt bar splitting
        let adapter = LyToIrAdapter::new();
        let src = "\\language \"italiano\"\n{ \\time 4/4 mi''8[mi la8. mi16] fad!8 sol16 la fad8. sol16 sol4 r r2 }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        assert_eq!(part.measures.len(), 2, "Expected 2 measures");
        // Each measure should sum to exactly 1 whole note
        for m in &part.measures {
            let total: Frac = m
                .voices
                .iter()
                .flat_map(|v| &v.elements)
                .map(voice_element_duration)
                .fold(Frac::from_integer(0), |a, b| a + b);
            assert_eq!(
                total,
                Frac::from_integer(1),
                "measure {} should be 1 whole note",
                m.number
            );
        }
    }

    #[test]
    fn test_multi_movement_scores() {
        let adapter = LyToIrAdapter::new();
        let source = r#"
\version "2.24.0"
melA = { c'4 d' e' f' }
melB = { g'4 a' b' c'' }
\score { \new Staff \melA }
\score { \new Staff \melB }
"#;
        let scores = adapter.convert_str_multi(source).unwrap();
        assert_eq!(scores.len(), 2, "should produce 2 scores (movements)");
        // Each score should have 1 part
        assert_eq!(scores[0].parts().len(), 1);
        assert_eq!(scores[1].parts().len(), 1);
        // Each part should have notes
        let notes0: Vec<&Note> = scores[0].parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        let notes1: Vec<&Note> = scores[1].parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(notes0.len(), 4);
        assert_eq!(notes1.len(), 4);
    }

    #[test]
    fn test_multi_movement_example2() {
        let adapter = LyToIrAdapter::new().with_language(PitchLanguage::Nederlands);
        let source = std::fs::read_to_string("tests/fixtures/ly/example2.ly").unwrap();
        let scores = adapter.convert_str_multi(&source).unwrap();
        assert_eq!(scores.len(), 2, "example2.ly has two \\score blocks");
        // Movement 1: 4 parts, G major (1 sharp)
        assert_eq!(scores[0].parts().len(), 4);
        // Movement 2: 4 parts, F major (1 flat)
        assert_eq!(scores[1].parts().len(), 4);
    }

    #[test]
    fn test_explicit_beam_brackets() {
        let adapter = LyToIrAdapter::new();
        let src = "{ c'8[ d' e' f'] }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 4);
        // First note: beam begin at level 1
        assert!(
            notes[0]
                .beams
                .iter()
                .any(|b| b.beam_type == "begin" && b.number == 1),
            "first note should have beam begin: {:?}",
            notes[0].beams
        );
        // Middle notes: beam continue at level 1
        assert!(
            notes[1]
                .beams
                .iter()
                .any(|b| b.beam_type == "continue" && b.number == 1),
            "second note should have beam continue: {:?}",
            notes[1].beams
        );
        assert!(
            notes[2]
                .beams
                .iter()
                .any(|b| b.beam_type == "continue" && b.number == 1),
            "third note should have beam continue: {:?}",
            notes[2].beams
        );
        // Last note: beam end at level 1
        assert!(
            notes[3]
                .beams
                .iter()
                .any(|b| b.beam_type == "end" && b.number == 1),
            "last note should have beam end: {:?}",
            notes[3].beams
        );
    }

    #[test]
    fn test_auto_beam_eighths_in_4_4() {
        let adapter = LyToIrAdapter::new();
        let src = "{ \\time 4/4 c'8 d' e' f' g' a' b' c'' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 8, "should have 8 eighth notes");
        assert!(notes[0]
            .beams
            .iter()
            .any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[1]
            .beams
            .iter()
            .any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[2]
            .beams
            .iter()
            .any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[3]
            .beams
            .iter()
            .any(|b| b.beam_type == "end" && b.number == 1));
        assert!(notes[4]
            .beams
            .iter()
            .any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[5]
            .beams
            .iter()
            .any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[6]
            .beams
            .iter()
            .any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[7]
            .beams
            .iter()
            .any(|b| b.beam_type == "end" && b.number == 1));
    }

    #[test]
    fn test_auto_beam_compound_6_8() {
        let adapter = LyToIrAdapter::new();
        let src = "{ \\time 6/8 c'8 d' e' f' g' a' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 6, "should have 6 eighth notes");
        assert!(notes[0].beams.iter().any(|b| b.beam_type == "begin"));
        assert!(notes[1].beams.iter().any(|b| b.beam_type == "continue"));
        assert!(notes[2].beams.iter().any(|b| b.beam_type == "end"));
        assert!(notes[3].beams.iter().any(|b| b.beam_type == "begin"));
        assert!(notes[4].beams.iter().any(|b| b.beam_type == "continue"));
        assert!(notes[5].beams.iter().any(|b| b.beam_type == "end"));
    }

    #[test]
    #[allow(clippy::vec_init_then_push)]
    fn test_stem_direction_commands() {
        let adapter = LyToIrAdapter::new();
        let src = "{ \\stemUp c'8 d' \\stemDown e' f' \\stemNeutral g' a' b' c'' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 8);
        assert_eq!(notes[0].stem_direction, "up");
        assert_eq!(notes[1].stem_direction, "up");
        assert_eq!(notes[2].stem_direction, "down");
        assert_eq!(notes[3].stem_direction, "down");
        assert_eq!(notes[4].stem_direction, "up", "G4 auto-stem should be up");
    }

    #[test]
    fn test_auto_stem_direction() {
        let adapter = LyToIrAdapter::new();
        let src = "{ c'4 b' c'' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes[0].stem_direction, "up", "C4 below middle -> up");
        assert_eq!(notes[1].stem_direction, "down", "B4 on middle line -> down");
        assert_eq!(notes[2].stem_direction, "down", "C5 above middle -> down");
    }

    #[test]
    fn test_acciaccatura_no_measure_duration() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 \acciaccatura d''8 c''2 e''8 d'' c'' b' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        assert_eq!(
            part.measures.len(),
            1,
            "acciaccatura should not cause extra measure split"
        );
    }

    #[test]
    fn test_auto_beam_16ths_grouped_by_4() {
        let adapter = LyToIrAdapter::new();
        let src = "{ \\time 4/4 c'16 d' e' f' g' a' b' c'' d'' e'' f'' g'' a'' b'' c''' d''' }";
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 16);
        assert!(notes[0]
            .beams
            .iter()
            .any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[3]
            .beams
            .iter()
            .any(|b| b.beam_type == "end" && b.number == 1));
        assert!(notes[4]
            .beams
            .iter()
            .any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[7]
            .beams
            .iter()
            .any(|b| b.beam_type == "end" && b.number == 1));
        assert!(notes[8]
            .beams
            .iter()
            .any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[11]
            .beams
            .iter()
            .any(|b| b.beam_type == "end" && b.number == 1));
    }

    #[test]
    fn test_tuplet_beam_isolation() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 c'4 \tuplet 3/2 { d'8 e' f' } g'4 }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 5);
        assert!(notes[1]
            .beams
            .iter()
            .any(|b| b.beam_type == "begin" && b.number == 1));
        assert!(notes[2]
            .beams
            .iter()
            .any(|b| b.beam_type == "continue" && b.number == 1));
        assert!(notes[3]
            .beams
            .iter()
            .any(|b| b.beam_type == "end" && b.number == 1));
        assert!(notes[0].beams.is_empty());
        assert!(notes[4].beams.is_empty());
    }

    #[test]
    fn test_alto_clef_auto_stem() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \clef "alto" \time 4/4 b4 c' d' e' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[0].stem_direction, "up", "B3 below alto middle -> up");
        assert_eq!(notes[1].stem_direction, "down", "C4 on alto middle -> down");
        assert_eq!(
            notes[2].stem_direction, "down",
            "D4 above alto middle -> down"
        );
    }

    #[test]
    fn test_lyric_melisma_skip() {
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "melody" { c'4 d' e' f' }
    \new Lyrics \lyricsto "melody" \lyricmode { hello _ world _ }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(notes.len() >= 4);
        assert_eq!(notes[0].lyrics.len(), 1, "note 0 should have a lyric");
        assert_eq!(notes[0].lyrics[0].text, "hello");
        assert!(
            notes[1].lyrics.is_empty(),
            "note 1 should have no lyric (melisma skip)"
        );
        assert_eq!(notes[2].lyrics.len(), 1, "note 2 should have a lyric");
        assert_eq!(notes[2].lyrics[0].text, "world");
        assert!(
            notes[3].lyrics.is_empty(),
            "note 3 should have no lyric (melisma skip)"
        );
    }

    #[test]
    fn test_lyric_single_hyphen_separator() {
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "v" { c'4 d' e' f' }
    \new Lyrics \lyricsto "v" \lyricmode { fi - li - ae rest }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(notes.len() >= 4);
        assert_eq!(notes[0].lyrics[0].text, "fi");
        assert_eq!(notes[0].lyrics[0].syllabic, SyllabicType::Begin);
        assert_eq!(notes[1].lyrics[0].text, "li");
        assert_eq!(notes[1].lyrics[0].syllabic, SyllabicType::Middle);
        assert_eq!(notes[2].lyrics[0].text, "ae");
        assert_eq!(notes[2].lyrics[0].syllabic, SyllabicType::End);
        assert_eq!(notes[3].lyrics[0].text, "rest");
        assert_eq!(notes[3].lyrics[0].syllabic, SyllabicType::Single);
    }

    #[test]
    fn test_auto_beam_off() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 \autoBeamOff c'8 d' e' f' g' a' b' c'' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 8);
        for (i, note) in notes.iter().enumerate() {
            assert!(
                note.beams.is_empty(),
                "note {} should have no beams with \\autoBeamOff",
                i
            );
        }
    }

    #[test]
    fn test_auto_beam_off_with_explicit_brackets() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 \autoBeamOff c'8[ d'] e' f' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        let notes: Vec<&Note> = m.voices[0]
            .elements
            .iter()
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(!notes[0].beams.is_empty(), "note 0 should have beam from [");
        assert!(!notes[1].beams.is_empty(), "note 1 should have beam from ]");
        assert!(notes[2].beams.is_empty(), "note 2 should have no beams");
        assert!(notes[3].beams.is_empty(), "note 3 should have no beams");
    }

    #[test]
    fn test_time_sig_change_measure_duration() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \time 4/4 c'4 d' e' f' | \time 3/4 g'4 a' b' | c''2. }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let part = &parts[0];
        assert!(part.measures.len() >= 3, "should have at least 3 measures");
        let m1_dur: Frac = part.measures[0].voices[0]
            .elements
            .iter()
            .map(|e| voice_element_duration(e))
            .sum();
        assert_eq!(m1_dur, Frac::new(1, 1), "m1 should be 1 whole");
        let m2_dur: Frac = part.measures[1].voices[0]
            .elements
            .iter()
            .map(|e| voice_element_duration(e))
            .sum();
        assert_eq!(m2_dur, Frac::new(3, 4), "m2 should be 3/4");
    }

    #[test]
    fn test_melisma_command() {
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "v" { \autoBeamOff c'4\melisma d' e'\melismaEnd f' }
    \new Lyrics \lyricsto "v" \lyricmode { word next }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(notes.len() >= 4, "expected 4 notes, got {}", notes.len());
        assert_eq!(notes[0].lyrics.len(), 1, "note 0 should have 'word'");
        assert_eq!(notes[0].lyrics[0].text, "word");
        assert!(
            notes[1].lyrics.is_empty(),
            "note 1 (d') should have no lyric (in melisma)"
        );
        assert!(
            notes[2].lyrics.is_empty(),
            "note 2 (e') should have no lyric (in melisma)"
        );
        assert_eq!(notes[3].lyrics.len(), 1, "note 3 should have 'next'");
        assert_eq!(notes[3].lyrics[0].text, "next");
    }

    #[test]
    fn test_slur_melisma() {
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "v" { \autoBeamOff c'4( d' e') f' }
    \new Lyrics \lyricsto "v" \lyricmode { word next }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<&Note> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(notes.len() >= 4, "expected 4 notes, got {}", notes.len());
        assert_eq!(notes[0].lyrics.len(), 1, "note 0 should have 'word'");
        assert_eq!(notes[0].lyrics[0].text, "word");
        assert!(
            notes[1].lyrics.is_empty(),
            "note 1 (d') should have no lyric (slur melisma)"
        );
        assert!(
            notes[2].lyrics.is_empty(),
            "note 2 (e') should have no lyric (slur melisma)"
        );
        assert_eq!(notes[3].lyrics.len(), 1, "note 3 should have 'next'");
        assert_eq!(notes[3].lyrics[0].text, "next");
    }

    #[test]
    fn test_chord_lyric() {
        let adapter = LyToIrAdapter::new();
        let src = r#"
\score {
  <<
    \new Voice = "v" { \autoBeamOff <c' e'>4 f' }
    \new Lyrics \lyricsto "v" \lyricmode { word two }
  >>
}
"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let elems: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();
        let chord = elems.iter().find_map(|e| {
            if let VoiceElement::Chord(c) = e {
                Some(c)
            } else {
                None
            }
        });
        assert!(chord.is_some(), "should have a chord");
        let chord = chord.unwrap();
        assert_eq!(
            chord.notes[0].lyrics.len(),
            1,
            "chord's first note should have 'word'"
        );
        assert_eq!(chord.notes[0].lyrics[0].text, "word");
        let f_note = elems.iter().find_map(|e| {
            if let VoiceElement::Note(n) = e {
                Some(n.as_ref())
            } else {
                None
            }
        });
        assert!(f_note.is_some(), "should have a note after chord");
        let f_note = f_note.unwrap();
        assert_eq!(f_note.lyrics.len(), 1, "f' should have 'two'");
        assert_eq!(f_note.lyrics[0].text, "two");
    }

    #[test]
    fn test_sustain_pedal_parsing() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ c'4\sustainOn d' e'\sustainOff f' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let dirs: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .filter_map(|d| d.pedal.as_ref())
            .collect();
        assert_eq!(dirs.len(), 2, "should have 2 pedal events");
        assert_eq!(dirs[0].pedal_type, "start");
        assert_eq!(dirs[1].pedal_type, "stop");
    }

    #[test]
    fn test_ottava_parsing() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \ottava #1 c''4 d'' \ottava #0 e' f' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let shifts: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .filter_map(|d| d.octave_shift.as_ref())
            .collect();
        assert_eq!(shifts.len(), 2, "should have 2 octave shifts");
        assert_eq!(shifts[0].shift_type, "up");
        assert_eq!(shifts[0].size, 8);
        assert_eq!(shifts[1].shift_type, "stop");
        assert_eq!(shifts[1].size, 0);
    }

    #[test]
    fn test_layout_break_parsing() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ c'4 d' e' f' \break g' a' b' c'' \pageBreak d'' e'' f'' g'' }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let breaks: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.directions)
            .filter_map(|d| d.layout_break.as_ref())
            .collect();
        assert_eq!(breaks.len(), 2, "should have 2 layout breaks");
        assert_eq!(*breaks[0], crate::ir::direction::LayoutBreakType::System);
        assert_eq!(*breaks[1], crate::ir::direction::LayoutBreakType::Page);
    }

    #[test]
    fn test_accidental_display_forced() {
        let adapter = LyToIrAdapter::new();
        let src = r#"\language "english"
{ cs'!4 d'?4 e'4 }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(notes.len() >= 3, "should have at least 3 notes");
        assert_eq!(
            notes[0].pitch.accidental,
            crate::ir::pitch::AccidentalDisplay::Forced
        );
        assert_eq!(
            notes[1].pitch.accidental,
            crate::ir::pitch::AccidentalDisplay::Cautionary
        );
        assert_eq!(
            notes[2].pitch.accidental,
            crate::ir::pitch::AccidentalDisplay::None
        );
    }

    #[test]
    fn test_transpose_simple() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \transpose c d { c'4 d' e' } }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 3, "should have 3 notes");
        assert_eq!(notes[0].pitch.step, PitchStep::D);
        assert_eq!(notes[0].pitch.octave, 4);
        assert_eq!(notes[1].pitch.step, PitchStep::E);
        assert_eq!(notes[1].pitch.octave, 4);
        assert_eq!(notes[2].pitch.step, PitchStep::F);
        assert_eq!(*notes[2].pitch.alter.numer(), 1, "F# should have alter=1");
    }

    #[test]
    fn test_transpose_with_relative() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ \transpose c d \relative c' { c4 d e } }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 3, "should have 3 notes");
        assert_eq!(notes[0].pitch.step, PitchStep::D);
        assert_eq!(notes[1].pitch.step, PitchStep::E);
        assert_eq!(notes[2].pitch.step, PitchStep::F);
    }

    #[test]
    fn test_tremolo_parsing() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{ c'4:32 d'8:16 e'2:32 }"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let notes: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(notes.len() >= 3, "should have at least 3 notes");
        assert_eq!(
            notes[0].tremolo_marks, 3,
            "c'4:32 should have 3 tremolo marks"
        );
        assert!(notes[0].ornaments.iter().any(|o| o.name == "tremolo"));
        assert_eq!(
            notes[1].tremolo_marks, 1,
            "d'8:16 should have 1 tremolo mark"
        );
        assert_eq!(
            notes[2].tremolo_marks, 4,
            "e'2:32 should have 4 tremolo marks"
        );
    }

    #[test]
    fn test_repeat_volta_with_alternative() {
        let adapter = LyToIrAdapter::new();
        let src = r#"\language "english"
{
    \time 3/4
    \repeat volta 2 {
        c'4 d' e' |
        f' g' a' |
    }
    \alternative {
        { b'4 a' g' | }
        { c''4 b' a' | }
    }
}"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let measures = &parts[0].measures;
        assert!(
            measures.len() >= 4,
            "should have at least 4 measures, got {}",
            measures.len()
        );
        assert!(measures[0].left_barline.is_some());
        let lb = measures[0].left_barline.as_ref().unwrap();
        assert_eq!(lb.style, BarlineType::RepeatForward);
        let alt1_start = measures.iter().position(|m| {
            m.left_barline
                .as_ref()
                .map_or(false, |bl| bl.ending_number == Some(1))
        });
        assert!(alt1_start.is_some(), "should find ending 1 start");
        let alt2_start = measures.iter().position(|m| {
            m.left_barline
                .as_ref()
                .map_or(false, |bl| bl.ending_number == Some(2))
        });
        assert!(alt2_start.is_some(), "should find ending 2 start");
    }

    #[test]
    fn test_repeat_volta_no_alternative() {
        let adapter = LyToIrAdapter::new();
        let src = r#"{
    \time 3/4
    \repeat volta 2 {
        c'4 d' e' |
        f' g' a' |
    }
}"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let measures = &parts[0].measures;
        assert!(measures[0].left_barline.is_some());
        assert_eq!(
            measures[0].left_barline.as_ref().unwrap().style,
            BarlineType::RepeatForward
        );
        let has_backward = measures.iter().any(|m| {
            m.right_barline
                .as_ref()
                .map_or(false, |bl| bl.style == BarlineType::RepeatBackward)
        });
        assert!(has_backward, "should have backward repeat barline");
    }

    #[test]
    fn test_repeat_barlines_survive_variable_chain() {
        let adapter = LyToIrAdapter::new();
        let src = r#"partA = \relative d' {
    \repeat volta 2 {
        d4 e f |
        g a b |
    }
    \alternative {
        { c4 b a | }
        { d4 c b | }
    }
}
scoreRH = {
    \new Staff = "rh" {
        <<
            \partA
        >>
    }
}
scoreAll = {
    \new PianoStaff {
        <<
            \scoreRH
        >>
    }
}
\score {
    \scoreAll
    \layout { }
}"#;
        let score = adapter.convert_str(src).unwrap();
        let parts = score.parts();
        let fwd = parts
            .iter()
            .flat_map(|p| &p.measures)
            .filter(|m| {
                m.left_barline
                    .as_ref()
                    .map_or(false, |bl| bl.style == BarlineType::RepeatForward)
            })
            .count();
        assert!(fwd > 0, "should have forward repeat barlines");
        let endings = parts
            .iter()
            .flat_map(|p| &p.measures)
            .filter(|m| {
                m.left_barline
                    .as_ref()
                    .map_or(false, |bl| bl.ending_number.is_some())
            })
            .count();
        assert!(endings > 0, "should have ending markers");
    }

    #[test]
    fn test_fixture_repeats_ly() {
        let src = std::fs::read_to_string("tests/fixtures/ly/repeats.ly").unwrap();
        let adapter = LyToIrAdapter::new();
        let scores = adapter.convert_str_multi(&src).unwrap();
        assert_eq!(
            scores.len(),
            1,
            "should produce exactly 1 score (MIDI-only block skipped), got {}",
            scores.len()
        );
        assert_eq!(
            scores[0].parts().len(),
            1,
            "should have 1 part (PianoStaff merged), got {}",
            scores[0].parts().len()
        );
        assert!(
            scores[0].parts()[0].staves >= 2,
            "piano part should have multiple staves, got {}",
            scores[0].parts()[0].staves
        );
        let total_measures: usize = scores[0].parts().iter().map(|p| p.measures.len()).sum();
        assert!(
            total_measures > 50,
            "should produce many measures, got {total_measures}"
        );
    }

    #[test]
    fn test_fixture_pedal_ly() {
        let src = std::fs::read_to_string("tests/fixtures/ly/pedal.ly").unwrap();
        let adapter = LyToIrAdapter::new();
        let scores = adapter.convert_str_multi(&src).unwrap();
        assert_eq!(
            scores.len(),
            1,
            "should produce exactly 1 score, got {}",
            scores.len()
        );
        let score = &scores[0];
        assert_eq!(
            score.parts().len(),
            1,
            "should have 1 part (PianoStaff with 2 staves), got {}",
            score.parts().len()
        );
        assert_eq!(
            score.parts()[0].staves,
            2,
            "piano part should have 2 staves"
        );
        let pedal_count: usize = score
            .parts()
            .iter()
            .flat_map(|p| &p.measures)
            .flat_map(|m| &m.directions)
            .filter(|d| d.pedal.is_some())
            .count();
        assert!(pedal_count > 0, "should have pedal events, got 0");
    }

    #[test]
    fn test_fixture_chopin_n_ly() {
        let src = std::fs::read_to_string("tests/fixtures/ly/chopin_n.ly").unwrap();
        let adapter = LyToIrAdapter::new();
        let scores = adapter.convert_str_multi(&src).unwrap();
        assert_eq!(
            scores.len(),
            1,
            "should produce exactly 1 score, got {}",
            scores.len()
        );
        assert_eq!(
            scores[0].parts().len(),
            1,
            "should have 1 part (PianoStaff with 2 staves), got {}",
            scores[0].parts().len()
        );
        assert_eq!(
            scores[0].parts()[0].staves,
            2,
            "piano part should have 2 staves"
        );
        let total_measures: usize = scores[0].parts().iter().map(|p| p.measures.len()).sum();
        assert!(
            total_measures > 50,
            "should produce many measures, got {total_measures}"
        );
    }

    // -----------------------------------------------------------------------
    // Music tree (Layer 1) round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ly_to_music_simple() {
        use crate::adapters::ToMusicAdapter;
        use crate::ir::music::Music;

        let adapter = LyToIrAdapter::new();
        let doc = adapter.convert_str_to_music(r#"{ c'4 d' e' f' }"#).unwrap();

        // The Music tree should contain note events
        fn count_notes(m: &Music) -> usize {
            match m {
                Music::Note { .. } => 1,
                Music::Sequential(v) | Music::Simultaneous(v) => v.iter().map(count_notes).sum(),
                Music::Context { content, .. }
                | Music::Grace { content, .. }
                | Music::Tuplet { content, .. }
                | Music::Variable { content, .. }
                | Music::Repeat { body: content, .. } => count_notes(content),
                _ => 0,
            }
        }

        assert!(count_notes(&doc.music) >= 4, "should have at least 4 notes");
    }

    #[test]
    fn test_ly_to_music_round_trip() {
        use crate::adapters::ir_to_ly::IrToLyAdapter;
        use crate::adapters::{FromMusicAdapter, ToMusicAdapter};

        let adapter = LyToIrAdapter::new();
        let doc = adapter.convert_str_to_music(r#"{ c'4 d' e' f' }"#).unwrap();

        // Convert Music tree back to LilyPond
        let emitter = IrToLyAdapter::new();
        let ly_output = emitter.convert_music(&doc).unwrap();

        // Should contain the note names
        assert!(
            ly_output.contains("c'"),
            "output should contain c': {}",
            ly_output
        );
        assert!(
            ly_output.contains("d'"),
            "output should contain d': {}",
            ly_output
        );
    }

    #[test]
    fn test_ly_to_music_with_time_sig() {
        use crate::adapters::ToMusicAdapter;
        use crate::ir::music::Music;

        let adapter = LyToIrAdapter::new();
        let doc = adapter
            .convert_str_to_music(r#"{ \time 3/4 c'4 d' e' }"#)
            .unwrap();

        // Should contain a TimeSignature event
        fn has_time_sig(m: &Music) -> bool {
            match m {
                Music::TimeSignature(_) => true,
                Music::Sequential(v) | Music::Simultaneous(v) => v.iter().any(has_time_sig),
                Music::Context { content, .. } => has_time_sig(content),
                _ => false,
            }
        }

        assert!(has_time_sig(&doc.music), "should contain a TimeSignature");
    }

    // -----------------------------------------------------------------------
    // E3T3: Extended unit tests for ly_to_ir parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_multi_measure_rest_expansion() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \time 4/4 c'4 d' e' f' | R1*3 | g'4 a' b' c'' }"#)
            .unwrap();
        let parts = score.parts();
        let part = &parts[0];
        // Should have 5 measures: 1 notes + 3 multi-measure rests + 1 notes
        assert!(
            part.measures.len() >= 5,
            "R1*3 should expand to 3 rest measures; got {}",
            part.measures.len()
        );
    }

    #[test]
    fn test_bar_type_final() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' \bar "|." }"#)
            .unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        assert!(m.right_barline.is_some(), "should have a final barline");
        assert_eq!(m.right_barline.as_ref().unwrap().style, BarlineType::Final);
    }

    #[test]
    fn test_bar_type_double() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ c'4 d' e' f' \bar "||" g' a' b' c'' }"#)
            .unwrap();
        let parts = score.parts();
        let m = &parts[0].measures[0];
        assert!(m.right_barline.is_some(), "should have a double barline");
        assert_eq!(m.right_barline.as_ref().unwrap().style, BarlineType::Double);
    }

    #[test]
    fn test_tempo_parsing() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \tempo 4 = 120 c'4 d' e' f' }"#)
            .unwrap();
        let parts = score.parts();
        let dirs = &parts[0].measures[0].directions;
        let has_tempo = dirs.iter().any(|d| d.tempo.is_some());
        assert!(has_tempo, "should have tempo direction: {dirs:?}");
        if let Some(t) = dirs.iter().find_map(|d| d.tempo.as_ref()) {
            assert!(
                (t.per_minute.unwrap_or(0.0) - 120.0).abs() < 0.1,
                "tempo should be 120 BPM"
            );
        }
    }

    #[test]
    fn test_tempo_with_text() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \tempo "Allegro" 4 = 144 c'4 d' e' f' }"#)
            .unwrap();
        let parts = score.parts();
        let dirs = &parts[0].measures[0].directions;
        let has_tempo = dirs.iter().any(|d| d.tempo.is_some());
        assert!(has_tempo, "should have tempo with text: {dirs:?}");
    }

    #[test]
    fn test_multi_voice_backslash_separator() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"\new Staff { << { c'4 d' e' f' } \\ { a4 b c' d' } >> }"#)
            .unwrap();
        let parts = score.parts();
        assert!(!parts.is_empty(), "should have at least 1 part");
        let m = &parts[0].measures[0];
        assert!(
            m.voices.len() >= 2,
            "should have 2+ voices from \\\\; got {}",
            m.voices.len()
        );
    }

    #[test]
    fn test_shorthand_articulation_staccato() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4-. d'-> e'-^ f'-! }"#).unwrap();
        let parts = score.parts();
        let elems = &parts[0].measures[0].voices[0].elements;

        fn note_arts(e: &VoiceElement) -> Vec<String> {
            if let VoiceElement::Note(n) = e {
                n.articulations.iter().map(|a| a.name.clone()).collect()
            } else {
                vec![]
            }
        }

        let a0 = note_arts(&elems[0]);
        assert!(
            a0.iter().any(|a| a.contains("staccato")),
            "-. should be staccato: {a0:?}"
        );
        let a1 = note_arts(&elems[1]);
        assert!(
            a1.iter().any(|a| a.contains("accent")),
            "-> should be accent: {a1:?}"
        );
    }

    #[test]
    fn test_slur_events_on_notes() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4( d' e') f' }"#).unwrap();
        let parts = score.parts();
        let elems = &parts[0].measures[0].voices[0].elements;

        if let VoiceElement::Note(n) = &elems[0] {
            assert!(
                n.slurs.iter().any(|s| s.slur_type == StartStop::Start),
                "first note should have slur start: {:?}",
                n.slurs
            );
        } else {
            panic!("expected Note");
        }

        // Find closing slur
        if let VoiceElement::Note(n) = &elems[2] {
            assert!(
                n.slurs.iter().any(|s| s.slur_type == StartStop::Stop),
                "third note should have slur stop: {:?}",
                n.slurs
            );
        } else {
            panic!("expected Note");
        }
    }

    #[test]
    fn test_appoggiatura_grace() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \appoggiatura c'8 d'4 e' f' g' }"#)
            .unwrap();
        let parts = score.parts();
        let elems = &parts[0].measures[0].voices[0].elements;
        // First element should be a grace note
        if let VoiceElement::Note(n) = &elems[0] {
            assert!(n.is_grace, "appoggiatura should produce grace note");
            assert!(!n.grace_slash, "appoggiatura should NOT have slash");
        } else {
            panic!("expected grace Note, got {:?}", elems[0]);
        }
    }

    #[test]
    fn test_dynamics_context_merge() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
  <<
    \new Staff { c'4 d' e' f' }
    \new Dynamics { s4\f s\p s\ff s\pp }
  >>
}"#,
            )
            .unwrap();
        let parts = score.parts();
        // Dynamics context should be merged — either 1 enriched part or
        // 2 parts where the dynamics part is folded
        assert!(
            parts.len() <= 2,
            "dynamics should fold or merge: got {} parts",
            parts.len()
        );
    }

    #[test]
    fn test_time_sig_synchronize_across_parts() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
  <<
    \new Staff { \time 3/4 c'4 d' e' }
    \new Staff { g4 a b }
  >>
}"#,
            )
            .unwrap();
        let parts = score.parts();
        assert!(parts.len() >= 2);
        // Both parts should have time sig in measure 1
        for (i, part) in parts.iter().enumerate() {
            let attr = part.measures[0].attributes.as_ref();
            assert!(
                attr.is_some(),
                "part {i} should have attributes in measure 1"
            );
            let time = attr.unwrap().time.as_ref();
            assert!(time.is_some(), "part {i} should have time signature");
        }
    }

    #[test]
    fn test_chained_variable_resolution() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"inner = { c'4 d' }
middle = { \inner e' f' }
\score { \new Staff \middle }"#,
            )
            .unwrap();
        let parts = score.parts();
        assert!(!parts.is_empty());
        let elems = &parts[0].measures[0].voices[0].elements;
        // Should have at least 4 notes from the chain
        let note_count: usize = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter(|e| matches!(e, VoiceElement::Note(_)))
            .count();
        assert!(
            note_count >= 4,
            "chained variables should resolve to ≥4 notes; got {note_count}"
        );
    }

    #[test]
    fn test_voice_one_two_direction() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\new Staff { << { \voiceOne c'4 d' e' f' } \\ { \voiceTwo a4 b c' d' } >> }"#,
            )
            .unwrap();
        let parts = score.parts();
        assert!(!parts.is_empty());
        // Multi-voice should produce ≥2 voices
        let total_voices: usize = parts[0]
            .measures
            .iter()
            .map(|m| m.voices.len())
            .max()
            .unwrap_or(0);
        assert!(
            total_voices >= 2,
            "should have ≥2 voices: got {total_voices}"
        );
    }

    #[test]
    fn test_once_override_no_crash() {
        let adapter = LyToIrAdapter::new();
        // \once \override is very common; should not crash
        let result =
            adapter.convert_str(r#"{ \once \override NoteHead.color = #red c'4 d' e' f' }"#);
        assert!(
            result.is_ok(),
            "\\once \\override should not crash: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_skip_as_spacer() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"{ \time 4/4 \skip 1 c'4 d' e' f' }"#)
            .unwrap();
        let parts = score.parts();
        // Should have at least 2 measures (1 skip + 1 notes)
        assert!(
            parts[0].measures.len() >= 2,
            "\\skip should create a spacer measure: got {} measures",
            parts[0].measures.len()
        );
    }

    #[test]
    fn test_fermata_on_note() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4 d' e' f'\fermata }"#).unwrap();
        let parts = score.parts();
        let elems = &parts[0].measures[0].voices[0].elements;
        // Last note should have fermata
        if let VoiceElement::Note(n) = elems.last().unwrap() {
            assert!(n.fermata.is_some(), "should have fermata: {n:?}");
        } else {
            panic!("expected Note");
        }
    }

    #[test]
    fn test_multi_staff_piano_pattern_basic() {
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\new PianoStaff <<
  \new Staff { c'4 d' e' f' }
  \new Staff { c4 d e f }
>>"#,
            )
            .unwrap();
        let parts = score.parts();
        // PianoStaff should create a single part with 2 staves
        // or 2 parts grouped
        assert!(!parts.is_empty());
    }

    #[test]
    fn test_tied_note_pair() {
        let adapter = LyToIrAdapter::new();
        let score = adapter.convert_str(r#"{ c'4~ c' d' e' }"#).unwrap();
        let parts = score.parts();
        let elems = &parts[0].measures[0].voices[0].elements;
        if let VoiceElement::Note(n) = &elems[0] {
            assert!(
                n.ties.iter().any(|t| t.tie_type == StartStop::Start),
                "first note should have tie start"
            );
        }
    }

    #[test]
    fn test_relative_octave_preservation() {
        // Relative mode should correctly determine octaves
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(r#"\relative c' { c d e f g a b c }"#)
            .unwrap();
        let parts = score.parts();
        let elems = &parts[0].measures[0].voices[0].elements;
        // First note should be C4 (c')
        if let VoiceElement::Note(first) = &elems[0] {
            assert_eq!(first.pitch.octave, 4, "first note c' should be octave 4");
        }
        // Second note should be D4 (step up from C4)
        if let VoiceElement::Note(n) = &elems[1] {
            assert_eq!(n.pitch.step, PitchStep::D);
            assert_eq!(n.pitch.octave, 4, "d should be octave 4");
        }
    }

    #[test]
    fn test_set_midi_instrument_scheme_string() {
        // \set Staff.midiInstrument = #"flute" (scheme string syntax)
        let adapter = LyToIrAdapter::new();
        let score = adapter
            .convert_str(
                r#"\score {
    \new Staff <<
        \set Staff.midiInstrument = #"flute"
        \relative c'' { c4 d e f }
    >>
}"#,
            )
            .unwrap();
        let part = &score.parts()[0];
        assert_eq!(part.midi_instrument, "flute");
    }
}
