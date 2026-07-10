//! Retrograde transform — reverse the order of voice elements within each voice.
//!
//! This reverses the temporal ordering of notes, rests, and chords in every
//! voice of every measure, producing a time-reversed version of the music.
//!
//! # Idempotency
//! Retrograde is self-inverse: `R(R(x)) == x`.

use crate::ir::annotation::Annotation;
use crate::ir::articulation::StartStop;
use crate::ir::music::{Music, MusicDocument};
use crate::ir::note::VoiceElement;
use crate::ir::score::Score;

use super::{MusicTransform, Transform};

/// Reverse the order of voice elements within every voice.
///
/// Voice elements are reversed, producing a time-reversed version.
pub struct Retrograde;

impl Retrograde {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Retrograde {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for Retrograde {
    fn apply(&self, score: &Score) -> Score {
        let mut result = score.clone();

        for part in result.parts_mut() {
            // Only musical content travels backwards. Positional properties —
            // attributes (divisions/key/time/clef), barlines, numbering —
            // stay on their measure shells, so the reversed score still
            // declares its key and meter up front instead of ending with
            // them. (Content swapped between unequal-length measures under a
            // mid-piece meter change is a known ceiling of this rule.)
            let mut contents: Vec<_> = part
                .measures
                .iter_mut()
                .map(|m| {
                    (
                        std::mem::take(&mut m.voices),
                        std::mem::take(&mut m.directions),
                        std::mem::take(&mut m.harmonies),
                        std::mem::take(&mut m.figured_bass),
                    )
                })
                .collect();
            contents.reverse();
            for (measure, (voices, directions, harmonies, figured_bass)) in
                part.measures.iter_mut().zip(contents)
            {
                measure.voices = voices;
                measure.directions = directions;
                measure.harmonies = harmonies;
                measure.figured_bass = figured_bass;
            }

            for measure in &mut part.measures {
                for voice in &mut measure.voices {
                    voice.elements.reverse();
                    reanchor_graces(&mut voice.elements);
                    for elem in &mut voice.elements {
                        swap_pairings(elem);
                    }
                }
            }
        }

        result
    }
}

/// After reversal a grace run trails its principal; put it back in front
/// (in original order). Graces at the very start (a trailing grace forward,
/// e.g. `\afterGrace`) are left alone.
fn reanchor_graces(elements: &mut Vec<VoiceElement>) {
    let is_grace = |e: &VoiceElement| matches!(e, VoiceElement::Note(n) if n.is_grace);
    let old = std::mem::take(elements);
    let mut out: Vec<VoiceElement> = Vec::with_capacity(old.len());
    let mut iter = old.into_iter().peekable();
    while let Some(e) = iter.next() {
        if is_grace(&e) {
            out.push(e); // leading grace: no principal before it
            continue;
        }
        let mut run: Vec<VoiceElement> = Vec::new();
        while iter.peek().is_some_and(&is_grace) {
            run.push(iter.next().unwrap());
        }
        if run.is_empty() {
            out.push(e);
        } else {
            run.reverse();
            out.extend(run);
            out.push(e);
        }
    }
    *elements = out;
}

/// Swap Start↔Stop on paired events (ties, slurs, tuplet brackets) so every
/// pair still opens before it closes in the reversed timeline.
fn swap_pairings(elem: &mut VoiceElement) {
    let flip = |t: &mut StartStop| {
        *t = match *t {
            StartStop::Start => StartStop::Stop,
            StartStop::Stop => StartStop::Start,
            StartStop::Continue => StartStop::Continue,
        }
    };
    match elem {
        VoiceElement::Note(n) => {
            for tie in &mut n.ties {
                flip(&mut tie.tie_type);
            }
            for slur in &mut n.slurs {
                flip(&mut slur.slur_type);
            }
            if let Some(t) = &mut n.tuplet {
                flip(&mut t.tuplet_type);
            }
        }
        VoiceElement::Chord(c) => {
            for n in &mut c.notes {
                for tie in &mut n.ties {
                    flip(&mut tie.tie_type);
                }
                for slur in &mut n.slurs {
                    flip(&mut slur.slur_type);
                }
                if let Some(t) = &mut n.tuplet {
                    flip(&mut t.tuplet_type);
                }
            }
        }
        VoiceElement::Rest(r) => {
            if let Some(t) = &mut r.tuplet {
                flip(&mut t.tuplet_type);
            }
        }
    }
}

impl MusicTransform for Retrograde {
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
        let mut result = doc.clone();
        retrograde_music_node(&mut result.music);
        result
    }
}

/// True for events that position the music (key, meter, clef, tempo) rather
/// than being music — a leading run of these stays at the front.
fn is_attribute_event(m: &Music) -> bool {
    matches!(
        m,
        Music::KeySignature(_) | Music::TimeSignature(_) | Music::Clef(_) | Music::Tempo(_)
    )
}

/// Swap paired annotations (ties, slurs, beams) on a reversed note/chord.
fn swap_annotations(annotations: &mut [Annotation]) {
    for a in annotations {
        *a = match std::mem::replace(a, Annotation::TieStart) {
            Annotation::TieStart => Annotation::TieStop,
            Annotation::TieStop => Annotation::TieStart,
            Annotation::SlurStart { number, .. } => Annotation::SlurStop { number },
            Annotation::SlurStop { number } => Annotation::SlurStart {
                number,
                placement: Default::default(),
            },
            Annotation::BeamStart => Annotation::BeamStop,
            Annotation::BeamStop => Annotation::BeamStart,
            other => other,
        };
    }
}

/// After reversal a `Music::Grace` node trails its principal; move each grace
/// run back in front of the element it ornaments (in original order).
fn reanchor_grace_nodes(children: &mut Vec<Music>) {
    let is_grace = |m: &Music| matches!(m, Music::Grace { .. });
    let old = std::mem::take(children);
    let mut out: Vec<Music> = Vec::with_capacity(old.len());
    let mut iter = old.into_iter().peekable();
    while let Some(e) = iter.next() {
        if is_grace(&e) {
            out.push(e);
            continue;
        }
        let mut run: Vec<Music> = Vec::new();
        while iter.peek().is_some_and(&is_grace) {
            run.push(iter.next().unwrap());
        }
        if run.is_empty() {
            out.push(e);
        } else {
            run.reverse();
            out.extend(run);
            out.push(e);
        }
    }
    *children = out;
}

/// Recursively reverse Sequential children in a Music tree.
fn retrograde_music_node(music: &mut Music) {
    match music {
        Music::Sequential(children) => {
            // A leading attribute run (\key \time \clef \tempo) stays put —
            // reversing it would migrate the declarations to the end.
            let split = children
                .iter()
                .position(|c| !is_attribute_event(c))
                .unwrap_or(children.len());
            children[split..].reverse();
            let mut tail: Vec<Music> = children.split_off(split);
            reanchor_grace_nodes(&mut tail);
            children.extend(tail);
            for child in children {
                retrograde_music_node(child);
            }
        }
        Music::Note { annotations, .. } => {
            swap_annotations(annotations);
        }
        Music::Chord {
            annotations,
            pitches,
            ..
        } => {
            swap_annotations(annotations);
            for (_, per_note) in pitches.iter_mut() {
                swap_annotations(per_note);
            }
        }
        Music::Simultaneous(children) => {
            // Don't reverse simultaneous — each voice gets retrograded independently
            for child in children {
                retrograde_music_node(child);
            }
        }
        Music::Context { content, .. }
        | Music::Grace { content, .. }
        | Music::Tuplet { content, .. }
        | Music::Variable { content, .. } => {
            retrograde_music_node(content);
        }
        Music::Repeat {
            body, alternatives, ..
        } => {
            retrograde_music_node(body);
            for alt in alternatives {
                retrograde_music_node(alt);
            }
        }
        _ => {}
    }
}

/// Functional API: reverse all voice elements and measure order.
pub fn retrograde(score: &Score) -> Score {
    Retrograde::new().apply(score)
}

/// Functional API: reverse Sequential children in a Music tree.
pub fn retrograde_music(doc: &MusicDocument) -> MusicDocument {
    Retrograde::new().apply_music(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::measure::Measure;
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;

    fn make_three_note_score() -> Score {
        let n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let n2 = Note::new(Pitch::new(PitchStep::D, 4), Duration::quarter());
        let n3 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let voice = Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(n1)),
                VoiceElement::Note(Box::new(n2)),
                VoiceElement::Note(Box::new(n3)),
            ],
        };
        let measure = Measure {
            number: 1,
            number_label: None,
            implicit: false,
            senza_misura: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            measure_repeat: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    fn extract_pitches(score: &Score) -> Vec<PitchStep> {
        let parts = score.parts();
        parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.step),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn retrograde_basic() {
        let score = make_three_note_score();
        let result = retrograde(&score);
        let pitches = extract_pitches(&result);
        assert_eq!(pitches, vec![PitchStep::E, PitchStep::D, PitchStep::C]);
    }

    #[test]
    fn retrograde_self_inverse() {
        let score = make_three_note_score();
        let doubled = retrograde(&retrograde(&score));
        let orig_pitches = extract_pitches(&score);
        let round_pitches = extract_pitches(&doubled);
        assert_eq!(orig_pitches, round_pitches, "retrograde is self-inverse");
    }

    #[test]
    fn retrograde_no_mutation() {
        let score = make_three_note_score();
        let original = score.clone();
        let _ = retrograde(&score);
        assert_eq!(score, original, "input must not be mutated");
    }

    #[test]
    fn retrograde_empty_score() {
        let score = Score::new();
        let result = retrograde(&score);
        assert_eq!(result, score);
    }

    #[test]
    fn retrograde_music_basic() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let result = super::retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                let steps: Vec<_> = children
                    .iter()
                    .filter_map(|c| match c {
                        Music::Note { pitch, .. } => Some(pitch.step),
                        _ => None,
                    })
                    .collect();
                assert_eq!(steps, vec![PitchStep::E, PitchStep::D, PitchStep::C]);
            }
            _ => panic!("expected Sequential"),
        }
    }

    #[test]
    fn retrograde_music_self_inverse() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let doubled = super::retrograde_music(&super::retrograde_music(&doc));
        assert_eq!(doc, doubled);
    }
}

/// Regression tests (review R4): retrograde must keep the score playable —
/// attributes stay at the (new) start, tie/slur/tuplet pairing still opens
/// before it closes, grace notes still precede their principal.
#[cfg(test)]
mod structure_tests {
    use super::*;
    use crate::ir::articulation::{StartStop, TieEvent, TupletDisplay};
    use crate::ir::duration::Duration;
    use crate::ir::measure::{KeyMode, KeySignature, Measure, MeasureAttributes, TimeSignature};
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;

    fn note(step: PitchStep) -> Note {
        Note::new(Pitch::new(step, 4), Duration::quarter())
    }

    fn score_of(measures: Vec<Measure>) -> Score {
        let mut part = Part::new("P1");
        part.measures = measures;
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    fn measure_of(number: u32, notes: Vec<Note>) -> Measure {
        let mut m = Measure::new(number);
        m.voices.push(Voice {
            number: 1,
            elements: notes
                .into_iter()
                .map(|n| VoiceElement::Note(Box::new(n)))
                .collect(),
        });
        m
    }

    #[test]
    fn attributes_stay_on_first_measure() {
        let mut m1 = measure_of(1, vec![note(PitchStep::C), note(PitchStep::D)]);
        m1.attributes = Some(MeasureAttributes {
            key: Some(KeySignature {
                fifths: 2,
                mode: KeyMode::Major,
            }),
            time: Some(TimeSignature::default()),
            ..MeasureAttributes::default()
        });
        let m2 = measure_of(2, vec![note(PitchStep::E), note(PitchStep::F)]);
        let result = retrograde(&score_of(vec![m1, m2]));
        let measures = &result.parts()[0].measures;
        assert!(
            measures[0].attributes.is_some(),
            "reversed score must still declare key/time up front"
        );
        assert!(measures[1].attributes.is_none());
        // Content is reversed: F E | D C.
        let steps: Vec<_> = measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.step),
                _ => None,
            })
            .collect();
        assert_eq!(
            steps,
            vec![PitchStep::F, PitchStep::E, PitchStep::D, PitchStep::C]
        );
    }

    #[test]
    fn tie_pairing_is_swapped() {
        let mut n1 = note(PitchStep::C);
        n1.ties.push(TieEvent {
            tie_type: StartStop::Start,
        });
        let mut n2 = note(PitchStep::C);
        n2.ties.push(TieEvent {
            tie_type: StartStop::Stop,
        });
        let result = retrograde(&score_of(vec![measure_of(1, vec![n1, n2])]));
        let elems = &result.parts()[0].measures[0].voices[0].elements;
        let tie_of = |e: &VoiceElement| match e {
            VoiceElement::Note(n) => n.ties[0].tie_type,
            _ => panic!("expected note"),
        };
        assert_eq!(tie_of(&elems[0]), StartStop::Start, "chain must open first");
        assert_eq!(tie_of(&elems[1]), StartStop::Stop);
    }

    #[test]
    fn tuplet_markers_are_swapped() {
        let mut n1 = note(PitchStep::C);
        n1.tuplet = Some(TupletDisplay {
            tuplet_type: StartStop::Start,
            bracket: true,
            show_number: "actual".to_string(),
        });
        let mut n3 = note(PitchStep::E);
        n3.tuplet = Some(TupletDisplay {
            tuplet_type: StartStop::Stop,
            bracket: true,
            show_number: "actual".to_string(),
        });
        let result = retrograde(&score_of(vec![measure_of(
            1,
            vec![n1, note(PitchStep::D), n3],
        )]));
        let elems = &result.parts()[0].measures[0].voices[0].elements;
        let tuplet_of = |e: &VoiceElement| match e {
            VoiceElement::Note(n) => n.tuplet.as_ref().map(|t| t.tuplet_type),
            _ => None,
        };
        assert_eq!(tuplet_of(&elems[0]), Some(StartStop::Start));
        assert_eq!(tuplet_of(&elems[2]), Some(StartStop::Stop));
    }

    #[test]
    fn grace_notes_stay_before_their_principal() {
        let mut g = note(PitchStep::B);
        g.is_grace = true;
        let result = retrograde(&score_of(vec![measure_of(
            1,
            vec![note(PitchStep::C), g, note(PitchStep::D)],
        )]));
        let elems = &result.parts()[0].measures[0].voices[0].elements;
        let flags: Vec<(PitchStep, bool)> = elems
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some((n.pitch.step, n.is_grace)),
                _ => None,
            })
            .collect();
        assert_eq!(
            flags,
            vec![
                (PitchStep::B, true),  // grace still precedes its principal
                (PitchStep::D, false), // reversed principal order: D then C
                (PitchStep::C, false),
            ]
        );
    }

    #[test]
    fn structural_retrograde_is_self_inverse() {
        let mut m1 = measure_of(1, vec![note(PitchStep::C), note(PitchStep::D)]);
        m1.attributes = Some(MeasureAttributes::default());
        let mut g = note(PitchStep::B);
        g.is_grace = true;
        let mut tied = note(PitchStep::E);
        tied.ties.push(TieEvent {
            tie_type: StartStop::Start,
        });
        let m2 = measure_of(2, vec![g, tied, note(PitchStep::F)]);
        let score = score_of(vec![m1, m2]);
        assert_eq!(retrograde(&retrograde(&score)), score);
    }

    #[test]
    fn music_layer_keeps_leading_attributes() {
        use crate::ir::music::{Music, MusicDocument};
        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::KeySignature(KeySignature {
                fifths: 1,
                mode: KeyMode::Major,
            }),
            Music::TimeSignature(TimeSignature::default()),
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let result = retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                assert!(
                    matches!(children[0], Music::KeySignature(_)),
                    "key must not migrate to the end"
                );
                assert!(matches!(children[1], Music::TimeSignature(_)));
                match (&children[2], &children[3]) {
                    (Music::Note { pitch: p1, .. }, Music::Note { pitch: p2, .. }) => {
                        assert_eq!((p1.step, p2.step), (PitchStep::D, PitchStep::C));
                    }
                    other => panic!("expected two notes, got {other:?}"),
                }
            }
            other => panic!("expected Sequential, got {other:?}"),
        }
        assert_eq!(retrograde_music(&retrograde_music(&doc)), doc);
    }

    #[test]
    fn music_layer_grace_and_tie_annotations() {
        use crate::ir::annotation::Annotation;
        use crate::ir::music::{Music, MusicDocument};
        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Grace {
                content: Box::new(Music::Note {
                    pitch: Pitch::new(PitchStep::B, 4),
                    duration: Duration::quarter(),
                    annotations: vec![],
                }),
                slash: true,
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![Annotation::TieStart],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![Annotation::TieStop],
            },
        ]));
        let result = retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                assert!(
                    matches!(children[1], Music::Grace { .. }),
                    "grace must precede its principal, got {children:?}"
                );
                match (&children[0], &children[2]) {
                    (
                        Music::Note {
                            annotations: a1, ..
                        },
                        Music::Note {
                            annotations: a2, ..
                        },
                    ) => {
                        assert!(matches!(a1[0], Annotation::TieStart), "chain opens first");
                        assert!(matches!(a2[0], Annotation::TieStop));
                    }
                    other => panic!("expected notes, got {other:?}"),
                }
            }
            other => panic!("expected Sequential, got {other:?}"),
        }
        assert_eq!(retrograde_music(&retrograde_music(&doc)), doc);
    }
}
