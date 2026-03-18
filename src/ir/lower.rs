//! Lower a Music tree (Layer 1) to a Score layout (Layer 2).
//!
//! This pass walks the Music tree, tracking cumulative time, and produces
//! the measure-based `Score → Part → Measure → Voice → VoiceElement` structure
//! needed by the MusicXML and MIDI exporters.
//!
//! # Algorithm
//! 1. Collect staff contexts from the Music tree into `StaffBuilder`s.
//! 2. Each `StaffBuilder` accumulates timed events (notes, rests, attribute changes).
//! 3. After collection, split events into measures based on time signatures.
//! 4. Assign voice numbers for simultaneous content.
//! 5. Build the Part/PartGroup hierarchy from the context tree.

use std::collections::HashMap;

use super::annotation::Annotation;
use super::articulation::*;
use super::direction::*;
use super::duration::{Duration, Frac};
use super::harmony::{FiguredBass, Harmony};
use super::measure::*;
use super::music::{ContextType, Music, MusicDocument, RepeatType};
use super::note::{Chord, Note, Rest, VoiceElement};
use super::part::Part;
use super::pitch::Pitch;
use super::score::*;
use super::voice::Voice;

/// A timed event: something that happens at a specific moment.
#[derive(Debug, Clone)]
enum TimedEvent {
    Note {
        pitch: Pitch,
        duration: Duration,
        annotations: Vec<Annotation>,
        voice: u8,
    },
    Chord {
        pitches: Vec<(Pitch, Vec<Annotation>)>,
        duration: Duration,
        annotations: Vec<Annotation>,
        voice: u8,
    },
    Rest {
        duration: Duration,
        is_measure_rest: bool,
        voice: u8,
    },
    Skip {
        duration: Duration,
        voice: u8,
    },
    TimeSignature(TimeSignature),
    KeySignature(KeySignature),
    Clef(Clef),
    Direction(Box<Direction>),
    Barline(Barline),
    FiguredBass(FiguredBass),
    Harmony(Harmony),
}

/// Collects events for a single staff with their absolute time offsets.
#[derive(Debug)]
struct StaffBuilder {
    name: String,
    #[allow(dead_code)]
    staff_number: u8,
    events: Vec<(Frac, TimedEvent)>,
}

impl StaffBuilder {
    fn new(name: String, staff_number: u8) -> Self {
        Self {
            name,
            staff_number,
            events: Vec::new(),
        }
    }

    fn push(&mut self, time: Frac, event: TimedEvent) {
        self.events.push((time, event));
    }
}

/// Tracks the structure being built from the Music tree.
struct LowerState {
    /// Stack of staff builders — one per Staff context encountered.
    staves: Vec<StaffBuilder>,
    /// Groups: (context_type, name, staff_indices)
    groups: Vec<(ContextType, Option<String>, Vec<usize>)>,
    /// Current time offset for sequential processing.
    time: Frac,
    /// Current voice number (incremented in Simultaneous).
    voice: u8,
    /// Current time signature (for measure splitting).
    current_time_sig: Frac,
    /// Current staff index we're adding events to.
    current_staff: Option<usize>,
    /// Metadata collected from the document.
    metadata: ScoreMetadata,
}

impl LowerState {
    fn new(metadata: ScoreMetadata) -> Self {
        Self {
            staves: Vec::new(),
            groups: Vec::new(),
            time: Frac::from_integer(0),
            voice: 1,
            current_time_sig: Frac::new(1, 1), // default 4/4
            current_staff: None,
            metadata,
        }
    }

    fn ensure_staff(&mut self) -> usize {
        if let Some(idx) = self.current_staff {
            idx
        } else {
            let idx = self.staves.len();
            self.staves
                .push(StaffBuilder::new(String::new(), idx as u8 + 1));
            self.current_staff = Some(idx);
            idx
        }
    }

    fn push_event(&mut self, event: TimedEvent) {
        let idx = self.ensure_staff();
        let time = self.time;
        self.staves[idx].push(time, event);
    }
}

/// Convert a `MusicDocument` to a `Score`.
pub fn lower_to_score(doc: &MusicDocument) -> Score {
    let mut state = LowerState::new(doc.metadata.clone());
    walk_music(&doc.music, &mut state);
    build_score(&mut state)
}

/// Convert a bare `Music` tree to a `Score` with default metadata.
pub fn lower_music_to_score(music: &Music) -> Score {
    let mut state = LowerState::new(ScoreMetadata::default());
    walk_music(music, &mut state);
    build_score(&mut state)
}

/// Recursively walk a Music node, accumulating timed events.
fn walk_music(music: &Music, state: &mut LowerState) {
    match music {
        Music::Sequential(children) => {
            for child in children {
                walk_music(child, state);
            }
        }

        Music::Simultaneous(children) => {
            let saved_time = state.time;
            let saved_voice = state.voice;
            let mut max_time = state.time;

            for (i, child) in children.iter().enumerate() {
                state.time = saved_time;
                state.voice = saved_voice + i as u8;
                walk_music(child, state);
                if state.time > max_time {
                    max_time = state.time;
                }
            }

            state.time = max_time;
            state.voice = saved_voice;
        }

        Music::Context {
            context_type,
            name,
            content,
        } => {
            walk_context(context_type, name.as_deref(), content, state);
        }

        Music::Note {
            pitch,
            duration,
            annotations,
        } => {
            let dur_frac = duration.actual_duration();
            state.push_event(TimedEvent::Note {
                pitch: *pitch,
                duration: duration.clone(),
                annotations: annotations.clone(),
                voice: state.voice,
            });
            state.time += dur_frac;
        }

        Music::Chord {
            pitches,
            duration,
            annotations,
        } => {
            let dur_frac = duration.actual_duration();
            state.push_event(TimedEvent::Chord {
                pitches: pitches.clone(),
                duration: duration.clone(),
                annotations: annotations.clone(),
                voice: state.voice,
            });
            state.time += dur_frac;
        }

        Music::Rest {
            duration,
            is_measure_rest,
        } => {
            let dur_frac = duration.actual_duration();
            state.push_event(TimedEvent::Rest {
                duration: duration.clone(),
                is_measure_rest: *is_measure_rest,
                voice: state.voice,
            });
            state.time += dur_frac;
        }

        Music::Skip { duration } => {
            let dur_frac = duration.actual_duration();
            state.push_event(TimedEvent::Skip {
                duration: duration.clone(),
                voice: state.voice,
            });
            state.time += dur_frac;
        }

        Music::TimeSignature(ts) => {
            state.current_time_sig = ts.beats_fraction();
            state.push_event(TimedEvent::TimeSignature(ts.clone()));
        }

        Music::KeySignature(ks) => {
            state.push_event(TimedEvent::KeySignature(*ks));
        }

        Music::Clef(clef) => {
            state.push_event(TimedEvent::Clef(*clef));
        }

        Music::Tempo(tempo) => {
            let dir = Direction {
                tempo: Some(tempo.clone()),
                ..Direction::default()
            };
            state.push_event(TimedEvent::Direction(Box::new(dir)));
        }

        Music::Direction(dir) => {
            state.push_event(TimedEvent::Direction(dir.clone()));
        }

        Music::Barline(barline) => {
            state.push_event(TimedEvent::Barline(barline.clone()));
        }

        Music::Grace { content, slash: _ } => {
            // Grace notes don't advance time. Walk content but restore time after.
            let saved_time = state.time;
            // TODO: Mark resulting notes as grace in the timed events
            walk_music(content, state);
            state.time = saved_time;
        }

        Music::Tuplet {
            normal: _,
            actual: _,
            content,
        } => {
            // Walk content — the tuplet scaling is already in the Duration objects
            walk_music(content, state);
        }

        Music::Repeat {
            repeat_type,
            count,
            body,
            alternatives,
        } => {
            walk_repeat(repeat_type, *count, body, alternatives, state);
        }

        Music::Variable { content, .. } => {
            walk_music(content, state);
        }

        Music::FiguredBass(fb) => {
            state.push_event(TimedEvent::FiguredBass(fb.clone()));
        }

        Music::Harmony(h) => {
            state.push_event(TimedEvent::Harmony(h.clone()));
        }

        Music::Lyric(_) => {
            // Lyrics are handled during note annotation conversion
        }
    }
}

/// Walk a context node, creating staff builders as needed.
fn walk_context(
    context_type: &ContextType,
    name: Option<&str>,
    content: &Music,
    state: &mut LowerState,
) {
    match context_type {
        ContextType::Staff | ContextType::TabStaff => {
            let idx = state.staves.len();
            state.staves.push(StaffBuilder::new(
                name.unwrap_or("").to_string(),
                idx as u8 + 1,
            ));
            let saved_staff = state.current_staff;
            state.current_staff = Some(idx);
            walk_music(content, state);
            state.current_staff = saved_staff;
        }

        ContextType::PianoStaff | ContextType::GrandStaff => {
            let group_idx = state.groups.len();
            state.groups.push((
                context_type.clone(),
                name.map(|s| s.to_string()),
                Vec::new(),
            ));
            let staff_count_before = state.staves.len();
            walk_music(content, state);
            let staff_count_after = state.staves.len();
            // Record which staves belong to this group
            let staff_indices: Vec<usize> = (staff_count_before..staff_count_after).collect();
            state.groups[group_idx].2 = staff_indices;
        }

        ContextType::StaffGroup | ContextType::ChoirStaff => {
            let group_idx = state.groups.len();
            state.groups.push((
                context_type.clone(),
                name.map(|s| s.to_string()),
                Vec::new(),
            ));
            let staff_count_before = state.staves.len();
            walk_music(content, state);
            let staff_count_after = state.staves.len();
            let staff_indices: Vec<usize> = (staff_count_before..staff_count_after).collect();
            state.groups[group_idx].2 = staff_indices;
        }

        ContextType::Voice | ContextType::TabVoice => {
            // Voice context just sets the voice number
            walk_music(content, state);
        }

        ContextType::Dynamics => {
            // Dynamics context: events go to the nearest staff
            walk_music(content, state);
        }

        ContextType::Score => {
            walk_music(content, state);
        }

        ContextType::Lyrics | ContextType::FiguredBass | ContextType::ChordNames => {
            walk_music(content, state);
        }
    }
}

/// Walk a repeat structure.
fn walk_repeat(
    repeat_type: &RepeatType,
    count: u16,
    body: &Music,
    alternatives: &[Music],
    state: &mut LowerState,
) {
    match repeat_type {
        RepeatType::Volta => {
            // For volta repeats, we unfold for the score layout
            // (MusicXML emitter handles volta brackets separately)
            if alternatives.is_empty() {
                for _ in 0..count {
                    walk_music(body, state);
                }
            } else {
                for i in 0..count as usize {
                    walk_music(body, state);
                    let alt_idx = if i < alternatives.len() {
                        i
                    } else {
                        alternatives.len() - 1
                    };
                    walk_music(&alternatives[alt_idx], state);
                }
            }
        }
        RepeatType::Unfold => {
            for i in 0..count as usize {
                walk_music(body, state);
                if !alternatives.is_empty() {
                    let alt_idx = if i < alternatives.len() {
                        i
                    } else {
                        alternatives.len() - 1
                    };
                    walk_music(&alternatives[alt_idx], state);
                }
            }
        }
        RepeatType::Percent | RepeatType::Tremolo => {
            // Just walk the body once for now
            walk_music(body, state);
        }
    }
}

/// Build the final Score from collected staff events.
fn build_score(state: &mut LowerState) -> Score {
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
                    pg.children
                        .push(ScoreChild::Part(merged_part));
                    score.children.push(ScoreChild::PartGroup(pg));

                    for &si in staff_indices {
                        staff_assigned[si] = true;
                    }
                } else {
                    // Single staff in group — just add as part
                    let si = staff_indices[0];
                    score
                        .children
                        .push(ScoreChild::Part(parts[si].1.clone()));
                    staff_assigned[si] = true;
                }
            }
            ContextType::StaffGroup | ContextType::ChoirStaff => {
                let group_type = ctx_type.ly_name();
                let mut pg = PartGroup::new(group_type);
                for &si in staff_indices {
                    pg.children
                        .push(ScoreChild::Part(parts[si].1.clone()));
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
            score
                .children
                .push(ScoreChild::Part(parts[i].1.clone()));
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
        VoiceElement::Forward(f) => f.staff = staff,
        VoiceElement::Backup(_) => {}
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
        .map(|(t, e)| {
            *t + match e {
                TimedEvent::Note { duration, .. }
                | TimedEvent::Chord { duration, .. }
                | TimedEvent::Rest { duration, .. }
                | TimedEvent::Skip { duration, .. } => duration.actual_duration(),
                _ => Frac::from_integer(0),
            }
        })
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
            let voice = build_voice_from_events(v_events, *voice_num, start, end);
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
fn compute_measure_boundaries(
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
    boundaries.push((zero, if has_initial_ts { Some(initial_ts) } else { None }));

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
            if *change_time < next_bar {
                // Time sig change mid-measure — split here
                boundaries.push((*change_time, Some(new_ts.clone())));
                current_ts_frac = new_ts.beats_fraction();
                pos = *change_time;
                // Advance ts_idx past all changes at this position
                while ts_idx < time_sig_changes.len()
                    && time_sig_changes[ts_idx].0 <= *change_time
                {
                    ts_idx += 1;
                }
            } else {
                // Time sig change exactly at next bar
                boundaries.push((next_bar, Some(new_ts.clone())));
                current_ts_frac = new_ts.beats_fraction();
                pos = next_bar;
                while ts_idx < time_sig_changes.len()
                    && time_sig_changes[ts_idx].0 <= next_bar
                {
                    ts_idx += 1;
                }
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

        // Include event if it overlaps with [start, end)
        if ev_time < end && ev_end > start {
            voice_map.entry(voice_num).or_default().push(ev);
        }
    }

    let mut result: Vec<(u8, Vec<&(Frac, TimedEvent)>)> = voice_map.into_iter().collect();
    result.sort_by_key(|(v, _)| *v);
    result
}

/// Get the duration of a timed event.
fn event_duration(event: &TimedEvent) -> Frac {
    match event {
        TimedEvent::Note { duration, .. }
        | TimedEvent::Chord { duration, .. }
        | TimedEvent::Rest { duration, .. }
        | TimedEvent::Skip { duration, .. } => duration.actual_duration(),
        _ => Frac::from_integer(0),
    }
}

/// Build a Voice from timed events within a measure range.
fn build_voice_from_events(
    events: &[&(Frac, TimedEvent)],
    voice_num: u8,
    _measure_start: Frac,
    _measure_end: Frac,
) -> Voice {
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
fn events_to_voice(
    events: &[(Frac, TimedEvent)],
    voice_num: u8,
    start: Frac,
    end: Frac,
) -> Voice {
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
            ..
        } => {
            let mut n = Note::new(*pitch, duration.clone());
            n.voice = voice_num;
            n.staff = staff_num;
            apply_annotations_to_note(&mut n, annotations);
            Some(VoiceElement::Note(Box::new(n)))
        }

        TimedEvent::Chord {
            pitches,
            duration,
            annotations,
            ..
        } => {
            let notes: Vec<Note> = pitches
                .iter()
                .map(|(p, anns)| {
                    let mut n = Note::new(*p, duration.clone());
                    n.voice = voice_num;
                    n.staff = staff_num;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::music::Music;
    use crate::ir::pitch::PitchStep;

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
        let music = Music::Sequential(vec![c4_quarter()])
            .in_context(ContextType::Staff, None);
        let score = lower_music_to_score(&music);
        assert_eq!(score.parts().len(), 1);
        assert!(!score.parts()[0].measures.is_empty());
    }

    #[test]
    fn test_lower_sequential_notes() {
        let music = Music::Sequential(vec![
            c4_quarter(),
            d4_quarter(),
            c4_quarter(),
            d4_quarter(),
        ])
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
        let m1_key = parts[0].measures[0]
            .attributes
            .as_ref()
            .and_then(|a| a.key);
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

        let lh = Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 3),
                duration: Duration::whole(),
                annotations: vec![],
            },
        ])
        .in_context(ContextType::Staff, Some("lh".to_string()));

        let music = Music::Simultaneous(vec![rh, lh])
            .in_context(ContextType::PianoStaff, None);

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

        let p2 = Music::Sequential(vec![
            Music::Skip {
                duration: Duration::new(Frac::new(3, 4)),
            },
        ])
        .in_context(ContextType::Staff, None);

        let music = Music::Simultaneous(vec![p1, p2])
            .in_context(ContextType::Score, None);

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
        assert!(boundaries.len() >= 3, "got {} boundaries: {:?}", boundaries.len(), boundaries);
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
}
