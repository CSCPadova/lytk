//! Lift a Score layout (Layer 2) to a Music tree (Layer 1).
//!
//! This pass converts the measure-based `Score → Part → Measure → Voice`
//! structure back into a `Music` tree, enabling round-trip conversion:
//! MusicXML → Score → Music tree → LilyPond.
//!
//! # Algorithm
//! 1. Wrap each Part in a `Context::Staff`.
//! 2. Multi-staff parts (staves > 1) are wrapped in `Context::PianoStaff`.
//! 3. Measures are flattened into `Sequential` with `TimeSignature`/`KeySignature` events at changes.
//! 4. Multi-voice measures become `Simultaneous` blocks.
//! 5. Forward/Backup elements are converted to skips.

use super::annotation::Annotation;
use super::articulation::*;
use super::direction::Direction;
use super::measure::{KeySignature, TimeSignature};
use super::music::{ContextType, Music, MusicDocument};
use super::note::{Note, VoiceElement};
use super::score::*;

/// Convert a `Score` to a `MusicDocument` with a Music tree.
pub fn lift_to_music(score: &Score) -> MusicDocument {
    let music = lift_score(score);
    MusicDocument {
        metadata: score.metadata.clone(),
        music,
    }
}

/// Convert a `Score` to a bare `Music` tree.
pub fn lift_score(score: &Score) -> Music {
    let mut parts = Vec::new();

    for child in &score.children {
        match child {
            ScoreChild::Part(p) => {
                parts.push(lift_part(p));
            }
            ScoreChild::PartGroup(pg) => {
                parts.push(lift_part_group(pg));
            }
        }
    }

    match parts.len() {
        0 => Music::empty(),
        1 => parts.into_iter().next().unwrap(),
        _ => Music::Simultaneous(parts),
    }
}

/// Lift a PartGroup to a Music tree.
fn lift_part_group(pg: &PartGroup) -> Music {
    let ctx_type = match pg.group_type.as_str() {
        "PianoStaff" => ContextType::PianoStaff,
        "GrandStaff" => ContextType::GrandStaff,
        "ChoirStaff" => ContextType::ChoirStaff,
        _ => ContextType::StaffGroup,
    };

    let mut children = Vec::new();
    for child in &pg.children {
        match child {
            ScoreChild::Part(p) => children.push(lift_part(p)),
            ScoreChild::PartGroup(nested) => children.push(lift_part_group(nested)),
        }
    }

    let content = if children.len() == 1 {
        children.into_iter().next().unwrap()
    } else {
        Music::Simultaneous(children)
    };

    Music::Context {
        context_type: ctx_type,
        name: if pg.name.is_empty() {
            None
        } else {
            Some(pg.name.clone())
        },
        content: Box::new(content),
    }
}

/// Lift a single Part to a Music tree.
fn lift_part(part: &super::part::Part) -> Music {
    if part.staves > 1 {
        // Multi-staff: split voices by staff number
        lift_multi_staff_part(part)
    } else {
        lift_single_staff_part(part)
    }
}

/// Lift a single-staff Part.
fn lift_single_staff_part(part: &super::part::Part) -> Music {
    let content = lift_measures(&part.measures);
    let name = if part.name.is_empty() {
        None
    } else {
        Some(part.name.clone())
    };

    Music::Context {
        context_type: ContextType::Staff,
        name,
        content: Box::new(content),
    }
}

/// Lift a multi-staff Part into a PianoStaff context.
fn lift_multi_staff_part(part: &super::part::Part) -> Music {
    let mut staff_contents: Vec<Vec<Music>> = (0..part.staves).map(|_| Vec::new()).collect();

    let mut prev_time: Option<TimeSignature> = None;
    let mut prev_key: Option<KeySignature> = None;

    for measure in &part.measures {
        // Emit attribute changes (only on staff 1, others get them via sync)
        if let Some(ref attrs) = measure.attributes {
            if let Some(ref ts) = attrs.time {
                if prev_time.as_ref() != Some(ts) {
                    for staff_events in staff_contents.iter_mut() {
                        staff_events.push(Music::TimeSignature(ts.clone()));
                    }
                    prev_time = Some(ts.clone());
                }
            }
            if let Some(ref ks) = attrs.key {
                if prev_key.as_ref() != Some(ks) {
                    for staff_events in staff_contents.iter_mut() {
                        staff_events.push(Music::KeySignature(*ks));
                    }
                    prev_key = Some(*ks);
                }
            }
            for (staff_num, clef) in &attrs.clefs {
                let idx = (*staff_num as usize).saturating_sub(1);
                if idx < staff_contents.len() {
                    staff_contents[idx].push(Music::Clef(*clef));
                }
            }
        }

        // Emit directions on the first staff
        for dir in &measure.directions {
            staff_contents[0].push(lift_direction(dir));
        }

        // Group voices by staff
        for voice in &measure.voices {
            let staff_num = voice_staff_number(voice);
            let idx = (staff_num as usize)
                .saturating_sub(1)
                .min(staff_contents.len() - 1);
            let voice_music = lift_voice_elements(&voice.elements);
            staff_contents[idx].extend(voice_music);
        }

        // Add barlines
        if let Some(ref barline) = measure.right_barline {
            staff_contents[0].push(Music::Barline(barline.clone()));
        }
    }

    let staves: Vec<Music> = staff_contents
        .into_iter()
        .map(|events| Music::Context {
            context_type: ContextType::Staff,
            name: None,
            content: Box::new(Music::Sequential(events)),
        })
        .collect();

    Music::Context {
        context_type: ContextType::PianoStaff,
        name: if part.name.is_empty() {
            None
        } else {
            Some(part.name.clone())
        },
        content: Box::new(Music::Simultaneous(staves)),
    }
}

/// Get the staff number for a voice (from its first element).
fn voice_staff_number(voice: &super::voice::Voice) -> u8 {
    for elem in &voice.elements {
        match elem {
            VoiceElement::Note(n) => return n.staff,
            VoiceElement::Rest(r) => return r.staff,
            VoiceElement::Chord(c) => return c.staff,
            VoiceElement::Forward(f) => return f.staff,
            VoiceElement::Backup(_) => {}
        }
    }
    1 // default
}

/// Lift a sequence of measures into a Music tree.
fn lift_measures(measures: &[super::measure::Measure]) -> Music {
    let mut events: Vec<Music> = Vec::new();
    let mut prev_time: Option<TimeSignature> = None;
    let mut prev_key: Option<KeySignature> = None;

    for measure in measures {
        // Emit attribute changes
        if let Some(ref attrs) = measure.attributes {
            if let Some(ref ts) = attrs.time {
                if prev_time.as_ref() != Some(ts) {
                    events.push(Music::TimeSignature(ts.clone()));
                    prev_time = Some(ts.clone());
                }
            }
            if let Some(ref ks) = attrs.key {
                if prev_key.as_ref() != Some(ks) {
                    events.push(Music::KeySignature(*ks));
                    prev_key = Some(*ks);
                }
            }
            for clef in attrs.clefs.values() {
                events.push(Music::Clef(*clef));
            }
        }

        // Emit directions
        for dir in &measure.directions {
            events.push(lift_direction(dir));
        }

        // Emit voice content
        if measure.voices.len() == 1 {
            let voice_events = lift_voice_elements(&measure.voices[0].elements);
            events.extend(voice_events);
        } else if measure.voices.len() > 1 {
            // Multi-voice: wrap in Simultaneous
            let voices: Vec<Music> = measure
                .voices
                .iter()
                .map(|v| Music::Sequential(lift_voice_elements(&v.elements)))
                .collect();
            events.push(Music::Simultaneous(voices));
        }

        // Emit barlines
        if let Some(ref barline) = measure.right_barline {
            events.push(Music::Barline(barline.clone()));
        }
    }

    Music::Sequential(events)
}

/// Convert a Direction to a Music node.
fn lift_direction(dir: &Direction) -> Music {
    if let Some(ref tempo) = dir.tempo {
        return Music::Tempo(tempo.clone());
    }
    Music::Direction(Box::new(dir.clone()))
}

/// Convert voice elements to a list of Music nodes.
fn lift_voice_elements(elements: &[VoiceElement]) -> Vec<Music> {
    let mut result = Vec::new();

    for elem in elements {
        match elem {
            VoiceElement::Note(n) => {
                if n.is_grace {
                    let grace_note = Music::Note {
                        pitch: n.pitch,
                        duration: n.duration.clone(),
                        annotations: note_to_annotations(n),
                    };
                    result.push(Music::Grace {
                        content: Box::new(grace_note),
                        slash: n.grace_slash,
                    });
                } else {
                    result.push(Music::Note {
                        pitch: n.pitch,
                        duration: n.duration.clone(),
                        annotations: note_to_annotations(n),
                    });
                }
            }
            VoiceElement::Rest(r) => {
                if r.is_spacer {
                    result.push(Music::Skip {
                        duration: r.duration.clone(),
                    });
                } else {
                    result.push(Music::Rest {
                        duration: r.duration.clone(),
                        is_measure_rest: r.is_measure_rest,
                    });
                }
            }
            VoiceElement::Chord(c) => {
                let pitches: Vec<(super::pitch::Pitch, Vec<Annotation>)> = c
                    .notes
                    .iter()
                    .map(|n| (n.pitch, note_to_annotations(n)))
                    .collect();
                let mut annotations = Vec::new();
                if let Some(arp) = &c.arpeggio {
                    annotations.push(Annotation::Arpeggio(*arp));
                }
                result.push(Music::Chord {
                    pitches,
                    duration: c.duration.clone(),
                    annotations,
                });
            }
            VoiceElement::Forward(f) => {
                // Convert Forward to Skip
                result.push(Music::Skip {
                    duration: f.duration.clone(),
                });
            }
            VoiceElement::Backup(_) => {
                // Backup is a MusicXML concept — skip it in the Music tree.
                // The voice structure handles time positioning.
            }
        }
    }

    result
}

/// Convert a Layer 2 Note's annotations back to the Annotation enum.
fn note_to_annotations(note: &Note) -> Vec<Annotation> {
    let mut anns = Vec::new();

    for a in &note.articulations {
        anns.push(Annotation::Articulation(a.clone()));
    }
    for o in &note.ornaments {
        anns.push(Annotation::Ornament(o.clone()));
    }
    for t in &note.technicals {
        anns.push(Annotation::Technical(t.clone()));
    }
    for d in &note.dynamics {
        anns.push(Annotation::Dynamic(d.clone()));
    }
    for w in &note.wedges {
        anns.push(Annotation::Wedge(w.clone()));
    }
    for s in &note.slurs {
        match s.slur_type {
            StartStop::Start => anns.push(Annotation::SlurStart {
                number: s.number,
                placement: s.placement,
            }),
            StartStop::Stop => anns.push(Annotation::SlurStop { number: s.number }),
            StartStop::Continue => {}
        }
    }
    for t in &note.ties {
        match t.tie_type {
            StartStop::Start => anns.push(Annotation::TieStart),
            StartStop::Stop => anns.push(Annotation::TieStop),
            StartStop::Continue => {}
        }
    }
    if let Some(ref f) = note.fermata {
        anns.push(Annotation::Fermata(f.clone()));
    }
    if note.tremolo_marks > 0 {
        anns.push(Annotation::Tremolo {
            marks: note.tremolo_marks,
        });
    }
    if let Some(g) = &note.glissando {
        anns.push(Annotation::Glissando(*g));
    }
    for td in &note.text_directions {
        anns.push(Annotation::Text(td.clone()));
    }
    for l in &note.lyrics {
        anns.push(Annotation::Lyric(l.clone()));
    }

    anns
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::{Duration, Frac};
    use crate::ir::measure::{KeyMode, Measure, MeasureAttributes};
    use crate::ir::note::Rest;
    use crate::ir::part::Part;
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::voice::Voice;

    fn make_note(step: PitchStep, octave: i32) -> Note {
        Note::new(Pitch::new(step, octave), Duration::quarter())
    }

    fn make_score_with_part(measures: Vec<Measure>) -> Score {
        let mut score = Score::new();
        let mut part = Part::new("P1");
        part.measures = measures;
        score.children.push(ScoreChild::Part(part));
        score
    }

    #[test]
    fn test_lift_empty_score() {
        let score = Score::new();
        let doc = lift_to_music(&score);
        assert!(doc.music.is_empty());
    }

    #[test]
    fn test_lift_single_note() {
        let mut m = Measure::new(1);
        let mut v = Voice::new(1);
        v.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::C, 4))));
        m.voices.push(v);

        let score = make_score_with_part(vec![m]);
        let doc = lift_to_music(&score);

        // Should be Context(Staff, Sequential([Note]))
        match &doc.music {
            Music::Context {
                context_type,
                content,
                ..
            } => {
                assert_eq!(*context_type, ContextType::Staff);
                match content.as_ref() {
                    Music::Sequential(items) => {
                        assert!(items.iter().any(|m| matches!(m, Music::Note { .. })));
                    }
                    _ => panic!("Expected Sequential"),
                }
            }
            _ => panic!("Expected Context"),
        }
    }

    #[test]
    fn test_lift_with_time_sig() {
        let mut m = Measure::new(1);
        m.attributes = Some(MeasureAttributes {
            time: Some(TimeSignature {
                beats: "3".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            ..MeasureAttributes::default()
        });
        let mut v = Voice::new(1);
        v.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::C, 4))));
        m.voices.push(v);

        let score = make_score_with_part(vec![m]);
        let doc = lift_to_music(&score);

        // Should contain a TimeSignature event
        match &doc.music {
            Music::Context { content, .. } => match content.as_ref() {
                Music::Sequential(items) => {
                    assert!(items.iter().any(|m| matches!(m, Music::TimeSignature(_))));
                }
                _ => panic!("Expected Sequential"),
            },
            _ => panic!("Expected Context"),
        }
    }

    #[test]
    fn test_lift_multi_voice() {
        let mut m = Measure::new(1);
        let mut v1 = Voice::new(1);
        v1.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::C, 4))));
        let mut v2 = Voice::new(2);
        v2.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::E, 3))));
        m.voices.push(v1);
        m.voices.push(v2);

        let score = make_score_with_part(vec![m]);
        let doc = lift_to_music(&score);

        // Should contain a Simultaneous block for the voices
        match &doc.music {
            Music::Context { content, .. } => match content.as_ref() {
                Music::Sequential(items) => {
                    assert!(items.iter().any(|m| matches!(m, Music::Simultaneous(_))));
                }
                _ => panic!("Expected Sequential"),
            },
            _ => panic!("Expected Context"),
        }
    }

    #[test]
    fn test_lift_spacer_rest() {
        let mut m = Measure::new(1);
        let mut v = Voice::new(1);
        let mut r = Rest::new(Duration::whole());
        r.is_spacer = true;
        v.elements.push(VoiceElement::Rest(r));
        m.voices.push(v);

        let score = make_score_with_part(vec![m]);
        let doc = lift_to_music(&score);

        match &doc.music {
            Music::Context { content, .. } => match content.as_ref() {
                Music::Sequential(items) => {
                    assert!(items.iter().any(|m| matches!(m, Music::Skip { .. })));
                }
                _ => panic!("Expected Sequential"),
            },
            _ => panic!("Expected Context"),
        }
    }

    #[test]
    fn test_lift_round_trip() {
        // Build a simple score, lift to Music, lower back to Score
        let mut m1 = Measure::new(1);
        m1.attributes = Some(MeasureAttributes {
            time: Some(TimeSignature {
                beats: "4".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            key: Some(KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            }),
            ..MeasureAttributes::default()
        });
        let mut v = Voice::new(1);
        v.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::C, 4))));
        v.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::D, 4))));
        v.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::E, 4))));
        v.elements
            .push(VoiceElement::Note(Box::new(make_note(PitchStep::F, 4))));
        m1.voices.push(v);

        let score = make_score_with_part(vec![m1]);

        // Lift to music
        let doc = lift_to_music(&score);

        // Lower back to score
        let score2 = crate::ir::lower::lower_to_score(&doc);

        // Should have same number of parts and measures
        assert_eq!(score2.parts().len(), 1);
        assert_eq!(score2.parts()[0].measures.len(), 1);
        assert_eq!(score2.parts()[0].measures[0].voices[0].elements.len(), 4);
    }
}
