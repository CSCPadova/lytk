//! Build Score/Part/Measure/Voice from collected events.
//!
//! Each staff's timed events become a positioned [`Timeline`] (voices as
//! lanes, everything else as events); a PianoStaff's staves fold into one
//! multi-staff timeline; then one score-wide [`Grid`] cuts every part into
//! measures, tying notes across bar lines — the same bar-splitter the
//! LilyPond reader uses (`crate::ir::timeline`).

use std::collections::BTreeMap;

use super::super::annotation::Annotation;
use super::super::articulation::*;
use super::super::direction::{Barline, BarlineType, RepeatDirection};
use super::super::duration::Frac;
use super::super::measure::*;
use super::super::music::ContextType;
use super::super::note::{Chord, Note, Rest, VoiceElement};
use super::super::part::Part;
use super::super::score::*;
use super::super::timeline::{split_tied, Event, Grid, Timeline, OFFSET_DIVISIONS};
use super::state::{LowerState, TimedEvent};

/// Build the final Score from collected staff events.
pub(super) fn build_score(state: &mut LowerState) -> Score {
    let mut score = Score::new();
    score.metadata = state.metadata.clone();

    if state.staves.is_empty() {
        return score;
    }

    let mut timelines: Vec<Timeline> = state
        .staves
        .iter()
        .map(|s| staff_timeline(&s.events))
        .collect();
    // The opening pickup (`\partial` at the start, an ABC first bar shorter than
    // `M:`) sizes the first bar.
    if let Some(p) = state
        .metadata
        .partial_duration
        .as_ref()
        .map(|d| d.actual_duration())
        .filter(|p| *p > Frac::from_integer(0))
    {
        timelines[0].add(Frac::from_integer(0), Event::Partial(p));
    }

    // Each slot is a part to be: a staff, or the staves of a piano folded into
    // its first one (`None` for the staves folded away), with its staff count.
    let mut slots: Vec<Option<(Timeline, u8)>> =
        timelines.into_iter().map(|tl| Some((tl, 1))).collect();
    let mut piano_at: BTreeMap<usize, ContextType> = BTreeMap::new();
    let mut piano_groups: Vec<&(ContextType, Option<String>, Vec<usize>)> = state
        .groups
        .iter()
        .filter(|(ctx, _, idx)| {
            matches!(ctx, ContextType::PianoStaff | ContextType::GrandStaff) && idx.len() >= 2
        })
        .collect();
    // Innermost first.
    piano_groups.sort_by_key(|(_, _, idx)| idx.len());
    for (ctx, _, idx) in piano_groups {
        let mut merged = Timeline::default();
        let mut offset = 0u8;
        let mut staves = 0u8;
        for &i in idx {
            let Some((tl, n)) = slots[i].take() else {
                continue;
            };
            let top = tl.lanes.keys().copied().max().unwrap_or(0);
            // A slot already folded (a piano inside a grand staff) keeps its
            // own staves, moved down past the ones before it.
            merged.absorb(tl.shift_staves(staves, offset));
            offset = offset.saturating_add(top);
            staves += n;
        }
        slots[idx[0]] = Some((merged, staves.max(1)));
        piano_at.insert(idx[0], ctx.clone());
    }

    let grid = Grid::build_tied(slots.iter().flatten().map(|(tl, _)| tl));
    let mut parts: Vec<Option<ScoreChild>> = slots
        .into_iter()
        .enumerate()
        .map(|(i, slot)| {
            let (tl, staves) = slot?;
            let mut part = Part::new(&format!("P{}", i + 1));
            part.name = if state.staves[i].name.is_empty() {
                format!("Part {}", i + 1)
            } else {
                state.staves[i].name.clone()
            };
            part.staves = staves;
            part.measures = split_tied(tl, &grid, OFFSET_DIVISIONS, OFFSET_DIVISIONS);
            if staves > 1 {
                if let Some(m) = part.measures.first_mut() {
                    m.attributes.get_or_insert_with(Default::default).staves = Some(staves);
                }
            }
            Some(match piano_at.get(&i) {
                Some(ctx) => {
                    let mut pg = PartGroup::new(match ctx {
                        ContextType::GrandStaff => "GrandStaff",
                        _ => "PianoStaff",
                    });
                    pg.bracket = "brace".to_string();
                    pg.children.push(ScoreChild::Part(part));
                    ScoreChild::PartGroup(pg)
                }
                None => ScoreChild::Part(part),
            })
        })
        .collect();

    // StaffGroup / ChoirStaff brackets, outermost only, in staff order.
    let brackets: Vec<(usize, usize, &ContextType)> = state
        .groups
        .iter()
        .filter(|(ctx, _, idx)| {
            matches!(ctx, ContextType::StaffGroup | ContextType::ChoirStaff) && !idx.is_empty()
        })
        .map(|(ctx, _, idx)| (idx[0], idx[idx.len() - 1], ctx))
        .collect();
    let outermost: Vec<&(usize, usize, &ContextType)> = brackets
        .iter()
        .filter(|(a, b, _)| {
            !brackets
                .iter()
                .any(|(c, d, _)| (c, d) != (a, b) && c <= a && b <= d)
        })
        .collect();
    let mut i = 0;
    while i < parts.len() {
        if let Some(&&(first, last, ctx)) = outermost.iter().find(|g| g.0 == i) {
            let mut pg = PartGroup::new(ctx.ly_name());
            pg.children = parts[first..=last]
                .iter_mut()
                .filter_map(Option::take)
                .collect();
            score.children.push(ScoreChild::PartGroup(pg));
            i = last + 1;
            continue;
        }
        if let Some(child) = parts[i].take() {
            score.children.push(child);
        }
        i += 1;
    }

    // Every part shows the key in force (the grid already gives every part
    // the meter).
    synchronize_attributes(&mut score);

    score
}

/// A staff's timed events as a positioned timeline: each voice's notes, rests
/// and spacers placed as lanes, everything else as events.
fn staff_timeline(events: &[(Frac, TimedEvent)]) -> Timeline {
    let mut tl = Timeline::default();
    // Onset order, keeping walk order at equal onsets (grace notes stay
    // before their main note).
    let mut voices: BTreeMap<u8, Vec<(Frac, VoiceElement)>> = BTreeMap::new();
    let mut ordered: Vec<&(Frac, TimedEvent)> = events.iter().collect();
    ordered.sort_by_key(|(t, _)| *t);
    for (t, ev) in ordered {
        let t = *t;
        match ev {
            TimedEvent::Note { voice, .. }
            | TimedEvent::Chord { voice, .. }
            | TimedEvent::Rest { voice, .. }
            | TimedEvent::Skip { voice, .. } => {
                if let Some(e) = timed_event_to_voice_element(ev, *voice, 1) {
                    voices.entry(*voice).or_default().push((t, e));
                }
            }
            TimedEvent::TimeSignature(ts) => tl.add(t, Event::Time(ts.clone())),
            TimedEvent::Partial(d) => tl.add(t, Event::Partial(*d)),
            TimedEvent::KeySignature(k) => tl.add(t, Event::Key(*k)),
            TimedEvent::Clef(c) => tl.add(t, Event::Clef(1, *c)),
            // A lifted direction sits at its bar's start with its offset
            // inside the bar: place it where it happens.
            TimedEvent::Direction(d) => {
                let mut d = d.clone();
                let at = t + std::mem::take(&mut d.offset_frac);
                // The staff is this timeline's (a lifted direction still names
                // the staff of the part it came from).
                d.staff = 0;
                tl.add(at, Event::Direction(d));
            }
            TimedEvent::Barline(b) => {
                for ev in barline_events(b) {
                    // Nothing closes at the very start: a bar line there
                    // opens the first bar (`[|` at the head of a tune).
                    let ev = match ev {
                        Event::RightBarline(mut b) if t == Frac::from_integer(0) => {
                            b.location = "left".to_string();
                            Event::LeftBarline(b)
                        }
                        other => other,
                    };
                    tl.add(t, ev);
                }
            }
            TimedEvent::FiguredBass(fb) => tl.add(t, Event::FiguredBass(fb.clone())),
            TimedEvent::Harmony(h) => tl.add(t, Event::Harmony(h.clone())),
        }
    }
    for (voice, elems) in voices {
        tl.place_voice(voice, &elems);
    }
    tl
}

/// A bar line opens the bar starting at its position when it starts a repeat
/// or an ending (or says so); any other closes the bar ending there. One that
/// does both — `::`, or a `|:` that also ends an ending — becomes a closing
/// bar line and an opening one.
fn barline_events(b: &Barline) -> Vec<Event> {
    use BarlineType::{Regular, RepeatBackward, RepeatBoth, RepeatForward};
    let forward = matches!(b.style, RepeatForward | RepeatBoth)
        || b.repeat_direction == Some(RepeatDirection::Forward);
    let backward = matches!(b.style, RepeatBackward | RepeatBoth)
        || b.repeat_direction == Some(RepeatDirection::Backward);
    let stops = matches!(b.ending_type.as_deref(), Some("stop" | "discontinue"));
    let left = |mut b: Barline| {
        b.location = "left".to_string();
        Event::LeftBarline(b)
    };
    let right = |mut b: Barline| {
        b.location = "right".to_string();
        Event::RightBarline(b)
    };
    if forward && (backward || stops) {
        let mut close = b.clone();
        close.style = if backward { RepeatBackward } else { Regular };
        close.repeat_direction = backward.then_some(RepeatDirection::Backward);
        close.repeat_times = None;
        let open = Barline {
            style: RepeatForward,
            repeat_direction: Some(RepeatDirection::Forward),
            repeat_times: b.repeat_times,
            ..Barline::default()
        };
        return vec![right(close), left(open)];
    }
    let opens = b.location == "left" || b.ending_type.as_deref() == Some("start") || forward;
    vec![if opens {
        left(b.clone())
    } else {
        right(b.clone())
    }]
}

/// Convert a TimedEvent to a VoiceElement.
fn timed_event_to_voice_element(
    event: &TimedEvent,
    voice_num: u8,
    staff_num: u8,
) -> Option<VoiceElement> {
    match event {
        TimedEvent::Note {
            pitch,
            duration,
            annotations,
            grace,
            ..
        } => {
            let mut n = Note::new(*pitch, duration.clone());
            n.voice = voice_num;
            n.staff = staff_num;
            if let Some(slash) = grace {
                n.is_grace = true;
                n.grace_slash = *slash;
            }
            apply_annotations_to_note(&mut n, annotations);
            Some(VoiceElement::Note(Box::new(n)))
        }

        TimedEvent::Chord {
            pitches,
            duration,
            annotations,
            grace,
            ..
        } => {
            let notes: Vec<Note> = pitches
                .iter()
                .map(|(p, anns)| {
                    let mut n = Note::new(*p, duration.clone());
                    n.voice = voice_num;
                    n.staff = staff_num;
                    if let Some(slash) = grace {
                        n.is_grace = true;
                        n.grace_slash = *slash;
                    }
                    apply_annotations_to_note(&mut n, anns);
                    n
                })
                .collect();
            let mut chord = Chord::new(duration.clone(), notes);
            chord.voice = voice_num;
            chord.staff = staff_num;
            // Apply chord-level annotations
            for ann in annotations {
                match ann {
                    Annotation::Arpeggio(arp_type) => {
                        chord.arpeggio = Some(*arp_type);
                    }
                    _ => {
                        // Apply to first note as fallback
                        if let Some(first) = chord.notes.first_mut() {
                            apply_single_annotation(first, ann);
                        }
                    }
                }
            }
            Some(VoiceElement::Chord(chord))
        }

        TimedEvent::Rest {
            duration,
            is_measure_rest,
            ..
        } => {
            let mut r = Rest::new(duration.clone());
            r.voice = voice_num;
            r.staff = staff_num;
            r.is_measure_rest = *is_measure_rest;
            Some(VoiceElement::Rest(r))
        }

        TimedEvent::Skip { duration, .. } => {
            let mut r = Rest::new(duration.clone());
            r.voice = voice_num;
            r.staff = staff_num;
            r.is_spacer = true;
            Some(VoiceElement::Rest(r))
        }

        _ => None, // Non-voice events handled separately
    }
}

/// Apply annotations from the Music tree to a Layer 2 Note.
fn apply_annotations_to_note(note: &mut Note, annotations: &[Annotation]) {
    for ann in annotations {
        apply_single_annotation(note, ann);
    }
}

fn apply_single_annotation(note: &mut Note, ann: &Annotation) {
    match ann {
        Annotation::Articulation(a) => note.articulations.push(a.clone()),
        Annotation::Ornament(o) => note.ornaments.push(o.clone()),
        Annotation::Technical(t) => note.technicals.push(t.clone()),
        Annotation::Dynamic(d) => note.dynamics.push(d.clone()),
        Annotation::Wedge(w) => note.wedges.push(w.clone()),
        Annotation::SlurStart { number, placement } => note.slurs.push(SlurEvent {
            slur_type: StartStop::Start,
            number: *number,
            placement: *placement,
        }),
        Annotation::SlurStop { number } => note.slurs.push(SlurEvent {
            slur_type: StartStop::Stop,
            number: *number,
            placement: Placement::Unspecified,
        }),
        Annotation::TieStart => note.ties.push(TieEvent {
            tie_type: StartStop::Start,
        }),
        Annotation::TieStop => note.ties.push(TieEvent {
            tie_type: StartStop::Stop,
        }),
        Annotation::BeamStart => note.beams.push(BeamEvent {
            beam_type: "begin".to_string(),
            number: 1,
        }),
        Annotation::BeamStop => note.beams.push(BeamEvent {
            beam_type: "end".to_string(),
            number: 1,
        }),
        Annotation::Fermata(f) => note.fermata = Some(f.clone()),
        Annotation::Arpeggio(_) => {} // handled at chord level
        Annotation::Glissando(ss) => note.glissando = Some(*ss),
        Annotation::Tremolo { marks } => note.tremolo_marks = *marks,
        Annotation::PedalStart | Annotation::PedalStop | Annotation::PedalChange => {
            // Pedal handled at direction level
        }
        Annotation::Text(td) => note.text_directions.push(td.clone()),
        Annotation::Fingering(f) => note.technicals.push(Technical {
            name: "fingering".to_string(),
            value: f.clone(),
        }),
        Annotation::Lyric(l) => note.lyrics.push(l.clone()),
        Annotation::OctaveShift(_) => {} // handled at direction level
        Annotation::Velocity(v) => note.velocity = Some(*v),
    }
}

/// Synchronize time/key signatures across all parts by measure index.
fn synchronize_attributes(score: &mut Score) {
    let num_parts = score.parts().len();
    if num_parts < 2 {
        return;
    }

    let max_measures = score
        .parts()
        .iter()
        .map(|p| p.measures.len())
        .max()
        .unwrap_or(0);

    let mut unified_time: Vec<Option<TimeSignature>> = vec![None; max_measures];
    let mut unified_key: Vec<Option<KeySignature>> = vec![None; max_measures];

    // Collect from all parts
    for part in score.parts().iter() {
        for (mi, m) in part.measures.iter().enumerate() {
            if let Some(ref attrs) = m.attributes {
                if attrs.time.is_some() && unified_time[mi].is_none() {
                    unified_time[mi] = attrs.time.clone();
                }
                if attrs.key.is_some() && unified_key[mi].is_none() {
                    unified_key[mi] = attrs.key;
                }
            }
        }
    }

    // Apply to all parts
    for part in score.parts_mut().iter_mut() {
        for (mi, m) in part.measures.iter_mut().enumerate() {
            if mi >= max_measures {
                break;
            }
            if let Some(ref ts) = unified_time[mi] {
                let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                if ma.time.is_none() {
                    ma.time = Some(ts.clone());
                }
            }
            if let Some(ref ks) = unified_key[mi] {
                let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                if ma.key.is_none() {
                    ma.key = Some(*ks);
                }
            }
        }
    }
}
