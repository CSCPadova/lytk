//! Recursive Music tree walking logic.
//!
//! Functions that walk Music nodes and produce timed events into `LowerState`.

use super::super::direction::Direction;
use super::super::music::{ContextType, Music, RepeatType};
use super::state::{LowerState, StaffBuilder, TimedEvent};

/// Recursively walk a Music node, accumulating timed events.
pub(super) fn walk_music(music: &Music, state: &mut LowerState) {
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
                grace: state.in_grace,
            });
            // Grace notes do not consume time.
            if state.in_grace.is_none() {
                state.time += dur_frac;
            }
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
                grace: state.in_grace,
            });
            if state.in_grace.is_none() {
                state.time += dur_frac;
            }
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

        Music::Grace { content, slash } => {
            // Grace notes don't advance time; the notes inside are flagged so
            // they come out of the lowering with is_grace/grace_slash set.
            let saved_grace = state.in_grace;
            state.in_grace = Some(*slash);
            walk_music(content, state);
            state.in_grace = saved_grace;
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

        // All group contexts behave identically; the stored `context_type`
        // is what later distinguishes PianoStaff/GrandStaff from StaffGroup/ChoirStaff.
        ContextType::PianoStaff
        | ContextType::GrandStaff
        | ContextType::StaffGroup
        | ContextType::ChoirStaff => {
            let group_idx = state.groups.len();
            state.groups.push((
                context_type.clone(),
                name.map(|s| s.to_string()),
                Vec::new(),
            ));
            let staff_count_before = state.staves.len();
            walk_music(content, state);
            let staff_count_after = state.staves.len();
            // Record which staves belong to this group.
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
        // Volta and Unfold both unfold the body for score layout (the MusicXML
        // emitter handles volta brackets separately). An empty `alternatives`
        // simply skips the alternative pass.
        RepeatType::Volta | RepeatType::Unfold => {
            for i in 0..count as usize {
                walk_music(body, state);
                if !alternatives.is_empty() {
                    let alt_idx = i.min(alternatives.len() - 1);
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
