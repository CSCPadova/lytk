#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::ir::duration::{Duration, Frac};
    use crate::ir::lower::{lower_music_to_score, lower_to_score};
    use crate::ir::measure::*;
    use crate::ir::music::{ContextType, Music, MusicDocument};
    use crate::ir::note::VoiceElement;
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::*;

    use super::super::build::compute_measure_boundaries;

    fn c4_quarter() -> Music {
        Music::Note {
            pitch: Pitch::new(PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        }
    }

    fn d4_quarter() -> Music {
        Music::Note {
            pitch: Pitch::new(PitchStep::D, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        }
    }

    #[test]
    fn test_lower_empty() {
        let doc = MusicDocument::new(Music::empty());
        let score = lower_to_score(&doc);
        assert!(score.parts().is_empty());
    }

    #[test]
    fn test_lower_single_note() {
        let music = Music::Sequential(vec![c4_quarter()]).in_context(ContextType::Staff, None);
        let score = lower_music_to_score(&music);
        assert_eq!(score.parts().len(), 1);
        assert!(!score.parts()[0].measures.is_empty());
    }

    #[test]
    fn test_lower_sequential_notes() {
        let music = Music::Sequential(vec![c4_quarter(), d4_quarter(), c4_quarter(), d4_quarter()])
            .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        // 4 quarter notes = 1 measure in 4/4
        assert_eq!(parts[0].measures.len(), 1);
        assert_eq!(parts[0].measures[0].voices[0].elements.len(), 4);
    }

    #[test]
    fn test_lower_two_measures() {
        // 8 quarter notes = 2 measures in 4/4
        let notes: Vec<Music> = (0..8).map(|_| c4_quarter()).collect();
        let music = Music::Sequential(notes).in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert_eq!(parts[0].measures.len(), 2);
    }

    #[test]
    fn test_lower_time_sig_change() {
        let music = Music::Sequential(vec![
            Music::TimeSignature(TimeSignature {
                beats: "3".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            c4_quarter(),
            d4_quarter(),
            c4_quarter(),
            // measure boundary (3/4 done)
            Music::TimeSignature(TimeSignature {
                beats: "3".to_string(),
                beat_type: 8,
                symbol: None,
            }),
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::eighth(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::F, 4),
                duration: Duration::eighth(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::G, 4),
                duration: Duration::eighth(),
                annotations: vec![],
            },
        ])
        .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        // Measure 1: 3/4 (3 quarter notes), Measure 2: 3/8 (3 eighth notes)
        assert_eq!(parts[0].measures.len(), 2);

        // Check time signatures
        let m1_ts = parts[0].measures[0]
            .attributes
            .as_ref()
            .and_then(|a| a.time.as_ref());
        assert_eq!(m1_ts.unwrap().beats, "3");
        assert_eq!(m1_ts.unwrap().beat_type, 4);

        let m2_ts = parts[0].measures[1]
            .attributes
            .as_ref()
            .and_then(|a| a.time.as_ref());
        assert_eq!(m2_ts.unwrap().beats, "3");
        assert_eq!(m2_ts.unwrap().beat_type, 8);
    }

    #[test]
    fn test_lower_key_signature() {
        let music = Music::Sequential(vec![
            Music::KeySignature(KeySignature {
                fifths: -1,
                mode: KeyMode::Minor,
            }),
            c4_quarter(),
            d4_quarter(),
            c4_quarter(),
            d4_quarter(),
        ])
        .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        let m1_key = parts[0].measures[0].attributes.as_ref().and_then(|a| a.key);
        assert_eq!(m1_key.unwrap().fifths, -1);
        assert_eq!(m1_key.unwrap().mode, KeyMode::Minor);
    }

    #[test]
    fn test_lower_simultaneous_voices() {
        let v1 = Music::Sequential(vec![c4_quarter(), d4_quarter(), c4_quarter(), d4_quarter()]);
        let v2 = Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 3),
                duration: Duration::half(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::F, 3),
                duration: Duration::half(),
                annotations: vec![],
            },
        ]);

        let music = Music::Simultaneous(vec![v1, v2]).in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert_eq!(parts.len(), 1);
        // Should have 2 voices in the measure
        assert!(parts[0].measures[0].voices.len() >= 2);
    }

    #[test]
    fn test_lower_piano_staff() {
        let rh = Music::Sequential(vec![c4_quarter(), d4_quarter(), c4_quarter(), d4_quarter()])
            .in_context(ContextType::Staff, Some("rh".to_string()));

        let lh = Music::Sequential(vec![Music::Note {
            pitch: Pitch::new(PitchStep::C, 3),
            duration: Duration::whole(),
            annotations: vec![],
        }])
        .in_context(ContextType::Staff, Some("lh".to_string()));

        let music = Music::Simultaneous(vec![rh, lh]).in_context(ContextType::PianoStaff, None);

        let score = lower_music_to_score(&music);
        // PianoStaff should result in a PartGroup with a single multi-staff part
        assert_eq!(score.children.len(), 1);
        match &score.children[0] {
            ScoreChild::PartGroup(pg) => {
                assert_eq!(pg.group_type, "PianoStaff");
                assert_eq!(pg.children.len(), 1);
                match &pg.children[0] {
                    ScoreChild::Part(p) => {
                        assert_eq!(p.staves, 2);
                    }
                    _ => panic!("Expected Part inside PianoStaff group"),
                }
            }
            _ => panic!("Expected PartGroup"),
        }
    }

    #[test]
    fn test_lower_with_skip() {
        let music = Music::Sequential(vec![
            Music::Skip {
                duration: Duration::whole(),
            },
            c4_quarter(),
        ])
        .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert!(!parts[0].measures.is_empty());
        // First element should be a spacer rest
        let first_elem = &parts[0].measures[0].voices[0].elements[0];
        match first_elem {
            VoiceElement::Rest(r) => assert!(r.is_spacer),
            _ => panic!("Expected spacer rest"),
        }
    }

    #[test]
    fn test_lower_annotations() {
        use crate::ir::annotation::Annotation;
        use crate::ir::articulation::{Articulation, DynamicMark, Placement};

        let music = Music::Sequential(vec![Music::Note {
            pitch: Pitch::new(PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![
                Annotation::Articulation(Articulation {
                    name: "staccato".to_string(),
                    placement: Placement::Above,
                }),
                Annotation::Dynamic(DynamicMark {
                    sign: "f".to_string(),
                    placement: Placement::Below,
                }),
                Annotation::TieStart,
            ],
        }])
        .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        let note = match &parts[0].measures[0].voices[0].elements[0] {
            VoiceElement::Note(n) => n,
            _ => panic!("Expected note"),
        };
        assert_eq!(note.articulations.len(), 1);
        assert_eq!(note.articulations[0].name, "staccato");
        assert_eq!(note.dynamics.len(), 1);
        assert_eq!(note.dynamics[0].sign, "f");
        assert_eq!(note.ties.len(), 1);
    }

    #[test]
    fn test_lower_sync_time_sig_across_parts() {
        let p1 = Music::Sequential(vec![
            Music::TimeSignature(TimeSignature {
                beats: "3".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            c4_quarter(),
            c4_quarter(),
            c4_quarter(),
        ])
        .in_context(ContextType::Staff, None);

        let p2 = Music::Sequential(vec![Music::Skip {
            duration: Duration::new(Frac::new(3, 4)),
        }])
        .in_context(ContextType::Staff, None);

        let music = Music::Simultaneous(vec![p1, p2]).in_context(ContextType::Score, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert_eq!(parts.len(), 2);

        // P2 should also have the 3/4 time signature from synchronization
        let p2_ts = parts[1].measures[0]
            .attributes
            .as_ref()
            .and_then(|a| a.time.as_ref());
        assert!(p2_ts.is_some());
        assert_eq!(p2_ts.unwrap().beats, "3");
    }

    #[test]
    fn test_compute_measure_boundaries_default() {
        let boundaries = compute_measure_boundaries(&[], Frac::new(2, 1));
        // 2 whole notes in 4/4 = 2 measures
        // Expected boundaries: [(0, Some(4/4)), (1, None), (2, None)]
        assert!(
            boundaries.len() >= 3,
            "got {} boundaries: {:?}",
            boundaries.len(),
            boundaries
        );
    }

    #[test]
    fn test_lower_variable() {
        let var_content = Music::Sequential(vec![c4_quarter(), d4_quarter()]);
        let music = Music::Sequential(vec![Music::Variable {
            name: "theme".to_string(),
            content: Box::new(var_content),
        }])
        .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let parts = score.parts();
        assert_eq!(parts[0].measures[0].voices[0].elements.len(), 2);
    }

    #[test]
    fn test_lower_grace_note_keeps_grace_flag() {
        // Grace content was walked with time restored but the produced notes
        // were NOT flagged is_grace — they came out as regular notes
        // overlapping the main note.
        let grace = Music::Grace {
            content: Box::new(Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::eighth(),
                annotations: vec![],
            }),
            slash: true,
        };
        let music = Music::Sequential(vec![grace, c4_quarter(), d4_quarter()])
            .in_context(ContextType::Staff, None);

        let score = lower_music_to_score(&music);
        let elements = &score.parts()[0].measures[0].voices[0].elements;
        assert_eq!(elements.len(), 3);
        match &elements[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace, "grace note must keep its is_grace flag");
                assert!(n.grace_slash, "acciaccatura slash must survive");
            }
            other => panic!("expected the grace note first, got {other:?}"),
        }
        match &elements[1] {
            VoiceElement::Note(n) => assert!(!n.is_grace),
            other => panic!("expected a regular note, got {other:?}"),
        }
    }
}
