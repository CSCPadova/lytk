//! Helpers shared by the walk and score assembly: beaming, tuplets, piano
//! direction placement, and the few score-level passes that still work on
//! measures (tempo propagation, barline sharing).

use crate::ir::articulation::{Placement, StartStop, TupletDisplay};
use crate::ir::direction::Direction;
use crate::ir::duration::Duration;
use crate::ir::note::VoiceElement;

use super::state::PartBuild;
use super::timeline::Event;

/// Return the beam level for a note duration:
/// 0 = not beamable (quarter or longer), 1 = eighth, 2 = 16th, 3 = 32nd, 4 = 64th.
pub(super) fn beam_level_for_duration(dur: &Duration) -> u8 {
    let d = *dur.base.denom();
    let n = *dur.base.numer();
    if n != 1 {
        return 0;
    }
    match d {
        8 => 1,
        16 => 2,
        32 => 3,
        64 => 4,
        128 => 5,
        _ => 0,
    }
}

/// Assign a multi-staff `<staff>` and placement to a direction by content,
/// for piano grand staves: the sustain pedal goes below the bottom staff;
/// dynamics and hairpins go below the top staff (between the staves); text,
/// tempo and rehearsal marks stay unattached (above). `staff_count` is the
/// number of staves in the combined part.
pub(super) fn assign_piano_direction_staff(dir: &mut Direction, staff_count: u8) {
    if dir.pedal.is_some() {
        dir.staff = staff_count;
        dir.placement = Placement::Below;
    } else if dir.dynamic.is_some() || dir.wedge.is_some() {
        dir.staff = 1;
        dir.placement = Placement::Below;
    }
    // Text / tempo / rehearsal / coda / segno: leave placement and staff unset
    // so they render above, unattached to a specific staff.
}

/// Ensure the first part has a tempo marking (MusicXML convention).
///
/// If any part has a tempo direction in its first measure but the first part
/// doesn't, copy it there.
pub(super) fn propagate_first_tempo(score: &mut crate::ir::score::Score) {
    use crate::ir::direction::TempoDirection;

    // Find the first tempo direction across all parts' first measures
    let mut first_tempo: Option<TempoDirection> = None;

    for part in score.parts() {
        if let Some(m) = part.measures.first() {
            for dir in &m.directions {
                if let Some(ref tempo) = dir.tempo {
                    first_tempo = Some(tempo.clone());
                    break;
                }
            }
        }
        if first_tempo.is_some() {
            break;
        }
    }

    if let Some(tempo) = first_tempo {
        // Ensure the first part has it
        let parts = score.parts_mut();
        if let Some(part) = parts.into_iter().next() {
            if let Some(m) = part.measures.first_mut() {
                let has_tempo = m.directions.iter().any(|d| d.tempo.is_some());
                if !has_tempo {
                    m.directions.push(Direction {
                        tempo: Some(tempo),
                        ..Default::default()
                    });
                }
            }
        }
    }
}

/// Synchronize barlines across all parts in a score.
///
/// When a custom barline (e.g. `\bar "||"`) appears in one part, it should
/// appear at the same measure in all parts.  This mirrors the approach used
/// by `synchronize_time_signatures`.
pub(super) fn synchronize_barlines(score: &mut crate::ir::score::Score) {
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

    // Collect unified barlines: first non-None from any part wins.
    let mut unified_left: Vec<Option<crate::ir::direction::Barline>> = vec![None; max_measures];
    let mut unified_right: Vec<Option<crate::ir::direction::Barline>> = vec![None; max_measures];

    for part in score.parts().iter() {
        for (mi, m) in part.measures.iter().enumerate() {
            if m.left_barline.is_some() && unified_left[mi].is_none() {
                unified_left[mi] = m.left_barline.clone();
            }
            if m.right_barline.is_some() && unified_right[mi].is_none() {
                unified_right[mi] = m.right_barline.clone();
            }
        }
    }

    // Apply to all parts
    for part in score.parts_mut().iter_mut() {
        for (mi, m) in part.measures.iter_mut().enumerate() {
            if mi >= max_measures {
                break;
            }
            if m.left_barline.is_none() {
                if let Some(ref bl) = unified_left[mi] {
                    m.left_barline = Some(bl.clone());
                }
            }
            if m.right_barline.is_none() {
                if let Some(ref bl) = unified_right[mi] {
                    m.right_barline = Some(bl.clone());
                }
            }
        }
    }
}

/// Apply tuplet ratio to a voice element's duration.
/// Called from `push_voice_element` so that measure duration tracking is correct.
pub(super) fn apply_tuplet_ratio(elem: &mut VoiceElement, actual: u8, normal: u8) {
    match elem {
        VoiceElement::Note(n) => {
            n.duration.tuplet_actual = actual;
            n.duration.tuplet_normal = normal;
        }
        VoiceElement::Rest(r) => {
            r.duration.tuplet_actual = actual;
            r.duration.tuplet_normal = normal;
        }
        VoiceElement::Chord(c) => {
            c.duration.tuplet_actual = actual;
            c.duration.tuplet_normal = normal;
            // The MusicXML exporter reads each chord member's own duration for
            // both <duration> and <time-modification>, so the ratio must reach
            // the inner notes too — otherwise tuplet chords emit their full
            // un-scaled duration and overflow the bar.
            for note in &mut c.notes {
                note.duration.tuplet_actual = actual;
                note.duration.tuplet_normal = normal;
            }
        }
    }
}

/// Set tuplet display markers (start/stop brackets) on a slice of voice elements.
pub(super) fn apply_tuplet_display(elements: &mut [VoiceElement], _actual: u8) {
    let len = elements.len();
    for (i, elem) in elements.iter_mut().enumerate() {
        let is_first = i == 0;
        let is_last = i == len - 1;
        if !is_first && !is_last {
            continue;
        }
        match elem {
            VoiceElement::Note(n) => {
                if is_first {
                    n.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Start,
                        bracket: true,
                        show_number: "actual".to_string(),
                    });
                } else if is_last {
                    n.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Stop,
                        bracket: true,
                        show_number: String::new(),
                    });
                }
            }
            VoiceElement::Rest(r) => {
                if is_first {
                    r.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Start,
                        bracket: true,
                        show_number: "actual".to_string(),
                    });
                } else if is_last {
                    r.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Stop,
                        bracket: true,
                        show_number: String::new(),
                    });
                }
            }
            VoiceElement::Chord(c) => {
                if let Some(first_note) = c.notes.first_mut() {
                    if is_first {
                        first_note.tuplet = Some(TupletDisplay {
                            tuplet_type: StartStop::Start,
                            bracket: true,
                            show_number: "actual".to_string(),
                        });
                    } else if is_last {
                        first_note.tuplet = Some(TupletDisplay {
                            tuplet_type: StartStop::Stop,
                            bracket: true,
                            show_number: String::new(),
                        });
                    }
                }
            }
        }
    }
}

/// Is this a Dynamics lane rather than a real (possibly resting) instrument
/// part? Dynamics contexts hold only spacers (`s`/`\skip`) and exist to carry
/// directions. A part whose lanes hold *real* rests and no directions is a
/// genuine resting instrument — folding it away would delete score content.
pub(super) fn part_is_dynamics_only(pb: &PartBuild) -> bool {
    let mut all_spacers = true;
    for (_, e) in pb.tl.lanes.values().flatten() {
        match e {
            VoiceElement::Note(_) | VoiceElement::Chord(_) => return false,
            VoiceElement::Rest(r) if !r.is_spacer => all_spacers = false,
            _ => {}
        }
    }
    let has_direction = pb
        .tl
        .events
        .iter()
        .any(|(_, ev)| matches!(ev, Event::Direction(_)));
    pb.context == "Dynamics" || all_spacers || has_direction
}
