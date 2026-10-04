//! Dynamics, hairpins and text have one home in a returned score: the
//! bar's directions, each at its place, a note's with its voice. Readers
//! may hang them on the elements they read them with ([`hoist`] then moves
//! them out), and code that works note by note puts the voiced ones back
//! on their notes for a while ([`sink`]); `sink` undoes `hoist`.

use std::borrow::Cow;

use super::direction::Direction;
use super::duration::Frac;
use super::measure::Measure;
use super::note::VoiceElement;
use super::score::Score;

/// Move every element's dynamics, hairpins and text into its bar's
/// directions: at the element's onset, in its voice and staff.
pub(crate) fn hoist(score: &mut Score) {
    for part in score.parts_mut() {
        for m in &mut part.measures {
            hoist_measure(m);
        }
    }
}

fn hoist_measure(m: &mut Measure) {
    let mut found = Vec::new();
    for v in &mut m.voices {
        let mut at = Frac::from_integer(0);
        for e in &mut v.elements {
            let base = Direction {
                offset: at,
                staff: e.staff(),
                voice: Some(v.number),
                ..Direction::default()
            };
            let (dynamics, wedges, texts) = match e {
                VoiceElement::Rest(r) => (
                    std::mem::take(&mut r.dynamics),
                    std::mem::take(&mut r.wedges),
                    Vec::new(),
                ),
                _ => {
                    let (mut d, mut w, mut t) = (Vec::new(), Vec::new(), Vec::new());
                    for n in e.notes_mut() {
                        d.append(&mut n.dynamics);
                        w.append(&mut n.wedges);
                        t.append(&mut n.text_directions);
                    }
                    (d, w, t)
                }
            };
            found.extend(dynamics.into_iter().map(|d| Direction {
                placement: d.placement,
                dynamic: Some(d),
                ..base.clone()
            }));
            found.extend(wedges.into_iter().map(|w| Direction {
                placement: w.placement,
                wedge: Some(w),
                ..base.clone()
            }));
            found.extend(texts.into_iter().map(|t| Direction {
                placement: t.placement,
                text: Some(t),
                ..base.clone()
            }));
            at += e.metric_duration();
        }
    }
    m.directions.extend(found);
}

/// Put each voiced dynamic, hairpin and text back on the element of its
/// voice at its place (the main note rather than a grace note before it);
/// one with nowhere to go stays a direction.
pub(crate) fn sink(score: &mut Score) {
    for part in score.parts_mut() {
        for m in &mut part.measures {
            sink_measure(m);
        }
    }
}

fn sink_measure(m: &mut Measure) {
    let directions = std::mem::take(&mut m.directions);
    for d in directions {
        if let Some(d) = sink_one(m, d) {
            m.directions.push(d);
        }
    }
}

/// `None` when `d` went onto its element.
fn sink_one(m: &mut Measure, d: Direction) -> Option<Direction> {
    let Some(voice) = d.voice.filter(|_| is_one_mark(&d)) else {
        return Some(d);
    };
    let Some(v) = m.voices.iter_mut().find(|v| v.number == voice) else {
        return Some(d);
    };
    let mut at = Frac::from_integer(0);
    let mut target: Option<usize> = None;
    for (i, e) in v.elements.iter().enumerate() {
        if at == d.offset && (d.staff == 0 || e.staff() == d.staff) {
            let grace =
                e.metric_duration() == Frac::from_integer(0) && !matches!(e, VoiceElement::Rest(_));
            if target.is_none() || !grace {
                target = Some(i);
            }
            if !grace {
                break;
            }
        }
        if at > d.offset {
            break;
        }
        at += e.metric_duration();
    }
    let Some(i) = target else {
        return Some(d);
    };
    match &mut v.elements[i] {
        VoiceElement::Rest(r) => {
            if let Some(dy) = d.dynamic {
                r.dynamics.push(dy);
            } else if let Some(w) = d.wedge {
                r.wedges.push(w);
            } else {
                // A rest carries no text.
                return Some(d);
            }
        }
        e => {
            let n = &mut e.notes_mut()[0];
            if let Some(dy) = d.dynamic {
                n.dynamics.push(dy);
            } else if let Some(w) = d.wedge {
                n.wedges.push(w);
            } else if let Some(t) = d.text {
                n.text_directions.push(t);
            }
        }
    }
    None
}

/// Whether `d` is one dynamic, hairpin or text, and nothing else.
fn is_one_mark(d: &Direction) -> bool {
    let mark_only = Direction {
        offset: d.offset,
        placement: d.placement,
        staff: d.staff,
        voice: d.voice,
        dynamic: d.dynamic.clone(),
        wedge: d.wedge.clone(),
        text: d.text.clone(),
        ..Direction::default()
    };
    let marks = [d.dynamic.is_some(), d.wedge.is_some(), d.text.is_some()];
    mark_only == *d && marks.iter().filter(|&&m| m).count() == 1
}

/// `score` with its voiced marks back on their notes (a copy only when it
/// has some).
pub(crate) fn sunk(score: &Score) -> Cow<'_, Score> {
    let voiced = score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.directions)
        .any(|d| d.voice.is_some());
    if !voiced {
        return Cow::Borrowed(score);
    }
    let mut score = score.clone();
    sink(&mut score);
    Cow::Owned(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::articulation::{DynamicMark, DynamicType, Placement, Wedge, WedgeType};
    use crate::ir::direction::TextDirection;
    use crate::ir::duration::Duration;
    use crate::ir::note::{Chord, Note, Rest};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::voice::Voice;

    fn note(step: PitchStep) -> Note {
        Note::new(Pitch::new(step, 4), Duration::quarter())
    }

    #[test]
    fn a_read_score_keeps_its_marks_in_directions() {
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::ToIrAdapter;
        let score = LyToIrAdapter::new()
            .convert_str(r#"<< { c''4\f d''\< e''^"dolce" f''\! } \\ { c'2\p r2 } >>"#)
            .expect("reads");
        let m = &score.parts()[0].measures[0];
        let bare = m.voices.iter().flat_map(|v| &v.elements).all(|e| match e {
            VoiceElement::Rest(r) => r.dynamics.is_empty() && r.wedges.is_empty(),
            e => e.notes().iter().all(|n| {
                n.dynamics.is_empty() && n.wedges.is_empty() && n.text_directions.is_empty()
            }),
        });
        assert!(bare, "marks left on elements");
        let marks: Vec<(Frac, Option<u8>)> =
            m.directions.iter().map(|d| (d.offset, d.voice)).collect();
        let at = |q| Frac::new(q, 4);
        assert_eq!(
            marks,
            [
                (at(0), Some(1)),
                (at(1), Some(1)),
                (at(2), Some(1)),
                (at(3), Some(1)),
                (at(0), Some(2)),
            ]
        );
    }

    #[test]
    fn hoisting_then_sinking_puts_every_mark_back() {
        let mut grace = note(PitchStep::B);
        grace.is_grace = true;
        let mut main = note(PitchStep::C);
        main.dynamics.push(DynamicMark {
            sign: DynamicType::F,
            placement: Placement::Below,
        });
        main.wedges.push(Wedge {
            wedge_type: WedgeType::Crescendo,
            placement: Placement::Unspecified,
        });
        let mut chord = Chord::new(
            Duration::quarter(),
            vec![note(PitchStep::D), note(PitchStep::F)],
        );
        chord.notes[0].text_directions.push(TextDirection {
            text: "dolce".to_string(),
            placement: Placement::Above,
            font_style: None,
            font_weight: None,
        });
        let mut rest = Rest::new(Duration::quarter());
        rest.wedges.push(Wedge {
            wedge_type: WedgeType::Stop,
            placement: Placement::Unspecified,
        });
        let mut second = Voice::new(2);
        let mut low = note(PitchStep::E);
        low.dynamics.push(DynamicMark {
            sign: DynamicType::P,
            placement: Placement::Unspecified,
        });
        second.elements.push(VoiceElement::Note(Box::new(low)));
        let mut m = Measure::new(1);
        m.voices.push(Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(grace)),
                VoiceElement::Note(Box::new(main)),
                VoiceElement::Chord(chord),
                VoiceElement::Rest(rest),
            ],
        });
        m.voices.push(second);
        let mut part = crate::ir::Part::new("P1");
        part.measures.push(m);
        let mut score = Score::new();
        score
            .children
            .push(crate::ir::score::ScoreChild::Part(part));

        let original = score.clone();
        hoist(&mut score);
        let m = &score.parts()[0].measures[0];
        let marks: Vec<_> = m
            .directions
            .iter()
            .map(|d| (d.offset, d.voice, d.dynamic.is_some(), d.text.is_some()))
            .collect();
        let at = |q| Frac::new(q, 4);
        assert_eq!(
            marks,
            [
                (at(0), Some(1), true, false),
                (at(0), Some(1), false, false),
                (at(1), Some(1), false, true),
                (at(2), Some(1), false, false),
                (at(0), Some(2), true, false),
            ]
        );
        assert!(m.voices.iter().all(|v| v
            .elements
            .iter()
            .all(|e| e.notes().iter().all(|n| n.dynamics.is_empty()))));
        sink(&mut score);
        assert_eq!(score, original);
    }
}
