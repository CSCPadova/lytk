//! Functions that build Score/Part/Measure/Voice from collected events.

use std::collections::HashMap;

use super::super::annotation::Annotation;
use super::super::articulation::*;
use super::super::duration::Frac;
use super::super::measure::*;
use super::super::music::ContextType;
use super::super::note::{Chord, Note, Rest, VoiceElement};
use super::super::part::Part;
use super::super::score::*;
use super::super::voice::Voice;
use super::state::{LowerState, TimedEvent};

/// Build the final Score from collected staff events.
pub(super) fn build_score(state: &mut LowerState) -> Score {
    let mut score = Score::new();
    score.metadata = state.metadata.clone();

    if state.staves.is_empty() {
        return score;
    }

    // Build parts from staves
    let mut parts: Vec<(usize, Part)> = Vec::new();

    for (i, staff) in state.staves.iter().enumerate() {
        let mut part = Part::new(&format!("P{}", i + 1));
        part.name = if staff.name.is_empty() {
            format!("Part {}", i + 1)
        } else {
            staff.name.clone()
        };

        let measures = split_events_into_measures(&staff.events);
        part.measures = measures;
        parts.push((i, part));
    }

    // Build score children based on groups
    let mut staff_assigned: Vec<bool> = vec![false; state.staves.len()];

    for (ctx_type, _name, staff_indices) in &state.groups {
        if staff_indices.is_empty() {
            continue;
        }

        match ctx_type {
            ContextType::PianoStaff | ContextType::GrandStaff => {
                if staff_indices.len() >= 2 {
                    // Multi-staff instrument: merge staves into one part
                    let mut merged_part = parts[staff_indices[0]].1.clone();
                    merged_part.staves = staff_indices.len() as u8;

                    // Set staff numbers on voice elements
                    for m in &mut merged_part.measures {
                        for v in &mut m.voices {
                            for e in &mut v.elements {
                                set_staff_number(e, 1);
                            }
                        }
                    }

                    // Merge voices from other staves
                    for (si, &staff_idx) in staff_indices.iter().enumerate().skip(1) {
                        let staff_num = si as u8 + 1;
                        let other = &parts[staff_idx].1;
                        for (mi, om) in other.measures.iter().enumerate() {
                            if mi < merged_part.measures.len() {
                                for v in &om.voices {
                                    let mut v2 = v.clone();
                                    for e in &mut v2.elements {
                                        set_staff_number(e, staff_num);
                                    }
                                    merged_part.measures[mi].voices.push(v2);
                                }
                                // Merge directions
                                merged_part.measures[mi]
                                    .directions
                                    .extend(om.directions.iter().cloned());
                            }
                        }
                    }

                    let group_type = match ctx_type {
                        ContextType::PianoStaff => "PianoStaff",
                        ContextType::GrandStaff => "GrandStaff",
                        _ => "StaffGroup",
                    };
                    let mut pg = PartGroup::new(group_type);
                    pg.bracket = "brace".to_string();
                    pg.children.push(ScoreChild::Part(merged_part));
                    score.children.push(ScoreChild::PartGroup(pg));

                    for &si in staff_indices {
                        staff_assigned[si] = true;
                    }
                } else {
                    // Single staff in group — just add as part
                    let si = staff_indices[0];
                    score.children.push(ScoreChild::Part(parts[si].1.clone()));
                    staff_assigned[si] = true;
                }
            }
            ContextType::StaffGroup | ContextType::ChoirStaff => {
                let group_type = ctx_type.ly_name();
                let mut pg = PartGroup::new(group_type);
                for &si in staff_indices {
                    pg.children.push(ScoreChild::Part(parts[si].1.clone()));
                    staff_assigned[si] = true;
                }
                score.children.push(ScoreChild::PartGroup(pg));
            }
            _ => {}
        }
    }

    // Add any unassigned staves as standalone parts
    for (i, assigned) in staff_assigned.iter().enumerate() {
        if !assigned {
            score.children.push(ScoreChild::Part(parts[i].1.clone()));
        }
    }

    // Synchronize time/key signatures across parts
    synchronize_attributes(&mut score);

    score
}

/// Set staff number on a VoiceElement.
fn set_staff_number(elem: &mut VoiceElement, staff: u8) {
    match elem {
        VoiceElement::Note(n) => n.staff = staff,
        VoiceElement::Rest(r) => r.staff = staff,
        VoiceElement::Chord(c) => {
            c.staff = staff;
            for n in &mut c.notes {
                n.staff = staff;
            }
        }
    }
}

/// Split a flat list of timed events into measures based on time signatures.
fn split_events_into_measures(events: &[(Frac, TimedEvent)]) -> Vec<Measure> {
    if events.is_empty() {
        return Vec::new();
    }

    // First pass: collect time signature changes
    let mut time_sig_changes: Vec<(Frac, TimeSignature)> = Vec::new();

    for (time, event) in events {
        if let TimedEvent::TimeSignature(ts) = event {
            time_sig_changes.push((*time, ts.clone()));
        }
    }

    // Compute measure boundaries
    let max_time = events
        .iter()
        .map(|(t, e)| *t + event_duration(e))
        .max()
        .unwrap_or(Frac::from_integer(0));

    let boundaries = compute_measure_boundaries(&time_sig_changes, max_time);

    if boundaries.len() < 2 {
        // Not enough boundaries — put everything in one measure
        let mut m = Measure::new(1);
        let voice = events_to_voice(events, 1, Frac::from_integer(0), max_time);
        if !voice.elements.is_empty() {
            m.voices.push(voice);
        }
        apply_attributes_to_measure(&mut m, events, Frac::from_integer(0), max_time);
        return vec![m];
    }

    // Create measures
    let mut measures = Vec::new();

    for i in 0..boundaries.len() - 1 {
        let start = boundaries[i].0;
        let end = boundaries[i + 1].0;
        let measure_num = (i + 1) as u32;

        let mut m = Measure::new(measure_num);

        // Set time signature if it changes at this boundary
        if let Some(ts) = &boundaries[i].1 {
            let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
            ma.time = Some(ts.clone());
        }

        // Collect voice-grouped events for this measure
        let voice_events = collect_voice_events(events, start, end);

        for (voice_num, v_events) in &voice_events {
            let voice = build_voice_from_events(v_events, *voice_num);
            if !voice.elements.is_empty() {
                m.voices.push(voice);
            }
        }

        // Apply non-voice attributes (key, clef, directions, figured bass, harmony, barlines)
        apply_attributes_to_measure(&mut m, events, start, end);

        measures.push(m);
    }

    measures
}

/// Measure boundary: (time, optional time signature at this boundary)
type MeasureBoundary = (Frac, Option<TimeSignature>);

/// Compute measure boundaries from time signature changes.
pub(super) fn compute_measure_boundaries(
    time_sig_changes: &[(Frac, TimeSignature)],
    max_time: Frac,
) -> Vec<MeasureBoundary> {
    let zero = Frac::from_integer(0);
    if max_time <= zero {
        return vec![];
    }

    let mut boundaries: Vec<MeasureBoundary> = Vec::new();

    // Start with default 4/4 if no time sig at t=0
    let has_initial_ts = time_sig_changes.first().map(|(t, _)| *t) == Some(zero);
    let initial_ts = if has_initial_ts {
        time_sig_changes[0].1.clone()
    } else {
        TimeSignature::default()
    };

    let mut current_ts_frac = initial_ts.beats_fraction();
    // Only store the time sig on the boundary if it was explicit
    boundaries.push((
        zero,
        if has_initial_ts {
            Some(initial_ts)
        } else {
            None
        },
    ));

    let mut pos = zero;

    // Process time signature changes in order
    let mut ts_idx = 0;

    // Skip the initial time sig change at t=0 (already handled above)
    while ts_idx < time_sig_changes.len() && time_sig_changes[ts_idx].0 == zero {
        ts_idx += 1;
    }

    loop {
        let next_bar = pos + current_ts_frac;

        // Check if a time sig change happens before the next natural bar
        let next_change = time_sig_changes
            .iter()
            .skip(ts_idx)
            .find(|(t, _)| *t > pos && *t <= next_bar);

        if let Some((change_time, new_ts)) = next_change {
            // change_time is in (pos, next_bar] — split the measure there.
            boundaries.push((*change_time, Some(new_ts.clone())));
            current_ts_frac = new_ts.beats_fraction();
            pos = *change_time;
            // Advance ts_idx past all changes at this position.
            while ts_idx < time_sig_changes.len() && time_sig_changes[ts_idx].0 <= *change_time {
                ts_idx += 1;
            }
        } else {
            // No time sig change before next bar
            if next_bar >= max_time {
                boundaries.push((next_bar, None));
                break;
            }
            boundaries.push((next_bar, None));
            pos = next_bar;
        }
    }

    boundaries
}

/// Group events by voice number within a time range.
fn collect_voice_events(
    events: &[(Frac, TimedEvent)],
    start: Frac,
    end: Frac,
) -> Vec<(u8, Vec<&(Frac, TimedEvent)>)> {
    let mut voice_map: HashMap<u8, Vec<&(Frac, TimedEvent)>> = HashMap::new();

    for ev in events {
        let voice_num = match &ev.1 {
            TimedEvent::Note { voice, .. }
            | TimedEvent::Chord { voice, .. }
            | TimedEvent::Rest { voice, .. }
            | TimedEvent::Skip { voice, .. } => *voice,
            _ => continue, // non-voice events handled separately
        };

        let ev_time = ev.0;
        let ev_end = ev_time + event_duration(&ev.1);

        // Include event if it overlaps with [start, end). Zero-duration
        // events (grace notes) belong to the measure they start in.
        if ev_time < end && (ev_end > start || (ev_end == start && ev_time == start)) {
            voice_map.entry(voice_num).or_default().push(ev);
        }
    }

    let mut result: Vec<(u8, Vec<&(Frac, TimedEvent)>)> = voice_map.into_iter().collect();
    result.sort_by_key(|(v, _)| *v);
    result
}

/// Get the duration of a timed event. Grace notes consume no measure time.
fn event_duration(event: &TimedEvent) -> Frac {
    match event {
        TimedEvent::Note { grace, .. } | TimedEvent::Chord { grace, .. } if grace.is_some() => {
            Frac::from_integer(0)
        }
        TimedEvent::Note { duration, .. }
        | TimedEvent::Chord { duration, .. }
        | TimedEvent::Rest { duration, .. }
        | TimedEvent::Skip { duration, .. } => duration.actual_duration(),
        _ => Frac::from_integer(0),
    }
}

/// Build a Voice from a slice of already-filtered timed events.
fn build_voice_from_events(events: &[&(Frac, TimedEvent)], voice_num: u8) -> Voice {
    let mut voice = Voice::new(voice_num);

    for ev in events {
        let elem = timed_event_to_voice_element(&ev.1, voice_num, 1);
        if let Some(e) = elem {
            voice.elements.push(e);
        }
    }

    voice
}

/// Legacy helper: build a single voice from all events in a time range.
fn events_to_voice(events: &[(Frac, TimedEvent)], voice_num: u8, start: Frac, end: Frac) -> Voice {
    let mut voice = Voice::new(voice_num);
    for ev in events {
        if ev.0 >= start && ev.0 < end {
            if let Some(elem) = timed_event_to_voice_element(&ev.1, voice_num, 1) {
                voice.elements.push(elem);
            }
        }
    }
    voice
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
    }
}

/// Apply non-voice attributes (key, clef, directions, etc.) to a measure.
fn apply_attributes_to_measure(
    measure: &mut Measure,
    events: &[(Frac, TimedEvent)],
    start: Frac,
    end: Frac,
) {
    for (time, event) in events {
        if *time < start || *time >= end {
            continue;
        }
        match event {
            TimedEvent::KeySignature(ks) => {
                let ma = measure
                    .attributes
                    .get_or_insert_with(MeasureAttributes::default);
                ma.key = Some(*ks);
            }
            TimedEvent::Clef(clef) => {
                let ma = measure
                    .attributes
                    .get_or_insert_with(MeasureAttributes::default);
                ma.clefs.insert(1, *clef); // default to staff 1
            }
            TimedEvent::Direction(dir) => {
                measure.directions.push(*dir.clone());
            }
            TimedEvent::Barline(barline) => {
                if barline.location == "left" {
                    measure.left_barline = Some(barline.clone());
                } else {
                    measure.right_barline = Some(barline.clone());
                }
            }
            TimedEvent::FiguredBass(fb) => {
                measure.figured_bass.push(fb.clone());
            }
            TimedEvent::Harmony(h) => {
                measure.harmonies.push(h.clone());
            }
            _ => {}
        }
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
