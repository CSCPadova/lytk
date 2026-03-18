use crate::ir::articulation::{BeamEvent, StartStop};
use crate::ir::duration::Frac;
use crate::ir::measure::{Clef, ClefSign, TimeSignature};
use crate::ir::note::VoiceElement;
use crate::ir::score::{Score, ScoreChild};

use super::merge::beam_level_for_duration;

/// Apply automatic beaming and stem directions to all parts in a score.
/// Only applies to notes that don't already have explicit beams/stems.
pub(super) fn post_process_beams_and_stems(score: &mut Score) {
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            let mut current_ts: Option<TimeSignature> = None;
            let mut current_clef = Clef::default(); // treble by default
            for measure in &mut part.measures {
                // Track time signature and clef changes
                if let Some(ref attrs) = measure.attributes {
                    if let Some(ref ts) = attrs.time {
                        current_ts = Some(ts.clone());
                    }
                    if let Some(clef) = attrs.clefs.get(&1) {
                        current_clef = *clef;
                    }
                }
                let ts = current_ts
                    .clone()
                    .unwrap_or(TimeSignature {
                        beats: "4".to_string(),
                        beat_type: 4,
                        symbol: None,
                    });
                for voice in &mut measure.voices {
                    auto_beam_voice(&mut voice.elements, &ts);
                    auto_stem_voice(&mut voice.elements, &current_clef);
                }
            }
        }
    }
}

/// Apply automatic beaming to a voice's elements.
///
/// Beaming rules:
/// - Group consecutive beamable notes (8th or shorter) within a beam span.
/// - In 4/4: 8th notes group per half note (4 per group); 16ths group per
///   quarter note (4 per group). Mixed groups use the shorter span.
/// - In other simple meters: group per beat (1/beat_type).
/// - In compound meters (6/8, 9/8, 12/8): group per dotted beat (3/beat_type).
/// - Tuplet notes only beam within their own tuplet group.
/// - Don't beam single notes (group must have >=2 beamable notes).
/// - Skip notes that already have explicit beams.
fn auto_beam_voice(elements: &mut [VoiceElement], ts: &TimeSignature) {
    let (eighth_span, sub_span) = compute_beam_spans(ts);
    if sub_span <= Frac::from_integer(0) {
        return;
    }

    // Collect beamable note indices with position and tuplet group info
    struct NoteInfo {
        idx: usize,
        position: Frac,
        beam_level: u8,
        has_explicit_beam: bool,
        tuplet_group: u32, // 0 = not in tuplet; same non-zero value = same tuplet
    }

    let mut infos: Vec<NoteInfo> = Vec::new();
    let mut pos = Frac::from_integer(0);
    let mut tuplet_counter: u32 = 0;
    let mut current_tuplet: u32 = 0;

    for (idx, elem) in elements.iter().enumerate() {
        match elem {
            VoiceElement::Note(n) => {
                // Track tuplet groups
                if let Some(ref td) = n.tuplet {
                    if td.tuplet_type == StartStop::Start {
                        tuplet_counter += 1;
                        current_tuplet = tuplet_counter;
                    }
                }
                let in_tuplet = if n.duration.tuplet_actual != 1 {
                    current_tuplet
                } else {
                    0
                };

                let level = beam_level_for_duration(&n.duration);
                if level > 0 && !n.is_grace {
                    infos.push(NoteInfo {
                        idx,
                        position: pos,
                        beam_level: level,
                        has_explicit_beam: !n.beams.is_empty() || n.no_auto_beam,
                        tuplet_group: in_tuplet,
                    });
                }
                if !n.is_grace {
                    pos += n.duration.actual_duration();
                }

                if let Some(ref td) = n.tuplet {
                    if td.tuplet_type == StartStop::Stop {
                        current_tuplet = 0;
                    }
                }
            }
            VoiceElement::Rest(r) => {
                // Track tuplet group for rests too
                if let Some(ref td) = r.tuplet {
                    if td.tuplet_type == StartStop::Start {
                        tuplet_counter += 1;
                        current_tuplet = tuplet_counter;
                    }
                }
                pos += r.duration.actual_duration();
                if let Some(ref td) = r.tuplet {
                    if td.tuplet_type == StartStop::Stop {
                        current_tuplet = 0;
                    }
                }
            }
            VoiceElement::Chord(c) => {
                let first_tuplet = c.notes.first().and_then(|n| n.tuplet.as_ref());
                if let Some(td) = first_tuplet {
                    if td.tuplet_type == StartStop::Start {
                        tuplet_counter += 1;
                        current_tuplet = tuplet_counter;
                    }
                }
                let in_tuplet = if c.duration.tuplet_actual != 1 {
                    current_tuplet
                } else {
                    0
                };

                let level = beam_level_for_duration(&c.duration);
                let has_beam = c.notes.first().is_some_and(|n| !n.beams.is_empty() || n.no_auto_beam);
                if level > 0 {
                    infos.push(NoteInfo {
                        idx,
                        position: pos,
                        beam_level: level,
                        has_explicit_beam: has_beam,
                        tuplet_group: in_tuplet,
                    });
                }
                pos += c.duration.actual_duration();

                let last_tuplet = c.notes.first().and_then(|n| n.tuplet.as_ref());
                if let Some(td) = last_tuplet {
                    if td.tuplet_type == StartStop::Stop {
                        current_tuplet = 0;
                    }
                }
            }
            VoiceElement::Forward(f) => {
                pos += f.duration.actual_duration();
            }
            VoiceElement::Backup(b) => {
                pos -= b.duration.actual_duration();
            }
        }
    }

    let span_of = |pos: Frac, span: Frac| -> i64 {
        if span <= Frac::from_integer(0) { return 0; }
        (pos / span).to_integer()
    };

    // Group consecutive beamable notes that share the same tuplet group
    // and fall within the same beam span.
    let mut i = 0;
    while i < infos.len() {
        if infos[i].has_explicit_beam {
            i += 1;
            continue;
        }

        let group_start = i;
        let tuplet_g = infos[i].tuplet_group;

        if tuplet_g != 0 {
            // Tuplet group: extend to end of same tuplet
            i += 1;
            while i < infos.len()
                && infos[i].tuplet_group == tuplet_g
                && !infos[i].has_explicit_beam
            {
                i += 1;
            }
        } else {
            // Non-tuplet: determine the effective span for this group.
            let beat = span_of(infos[i].position, sub_span);
            i += 1;
            while i < infos.len()
                && infos[i].tuplet_group == 0
                && !infos[i].has_explicit_beam
                && span_of(infos[i].position, sub_span) == beat
            {
                i += 1;
            }
        }

        let group_end = i;
        let group_slice = &infos[group_start..group_end];
        let group_len = group_slice.len();

        if group_len < 2 {
            continue;
        }

        // For non-tuplet 8th-only groups, try to merge with the next beat group
        let all_eighths = group_slice.iter().all(|n| n.beam_level == 1);
        let mut merged_end = group_end;

        if tuplet_g == 0 && all_eighths && eighth_span > sub_span {
            let eighth_beat = span_of(infos[group_start].position, eighth_span);
            while merged_end < infos.len()
                && infos[merged_end].tuplet_group == 0
                && !infos[merged_end].has_explicit_beam
                && infos[merged_end].beam_level == 1
                && span_of(infos[merged_end].position, eighth_span) == eighth_beat
            {
                merged_end += 1;
            }
            if merged_end > group_end {
                i = merged_end;
            }
        }

        let final_slice = &infos[group_start..merged_end];
        let final_len = final_slice.len();

        if final_len < 2 {
            continue;
        }

        // Build beam assignments
        let max_level = final_slice.iter().map(|n| n.beam_level).max().unwrap_or(1);

        let mut assignments: Vec<(usize, Vec<BeamEvent>)> = Vec::new();
        for info in final_slice {
            assignments.push((info.idx, Vec::new()));
        }

        // Level 1: beam across the entire group
        for (gi, _) in final_slice.iter().enumerate() {
            let bt = if gi == 0 {
                "begin"
            } else if gi == final_len - 1 {
                "end"
            } else {
                "continue"
            };
            assignments[gi].1.push(BeamEvent {
                beam_type: bt.to_string(),
                number: 1,
            });
        }

        // Level 2+: break at sub_span boundaries
        for level in 2..=max_level {
            let mut si = 0;
            while si < final_len {
                let info = &final_slice[si];
                if info.beam_level < level {
                    si += 1;
                    continue;
                }
                let sub_beat = span_of(info.position, sub_span);
                let sub_start = si;
                si += 1;
                while si < final_len {
                    let ni = &final_slice[si];
                    if ni.beam_level < level
                        || span_of(ni.position, sub_span) != sub_beat
                    {
                        break;
                    }
                    si += 1;
                }
                let sub_len = si - sub_start;
                if sub_len < 2 {
                    // Single note at this level: use a hook
                    let is_at_end = sub_start + 1 >= final_len;
                    let hook = if is_at_end { "backward hook" } else { "forward hook" };
                    assignments[sub_start].1.push(BeamEvent {
                        beam_type: hook.to_string(),
                        number: level,
                    });
                    continue;
                }
                for (j, assignment) in assignments[sub_start..si].iter_mut().enumerate() {
                    let bt = if j == 0 {
                        "begin"
                    } else if sub_start + j == si - 1 {
                        "end"
                    } else {
                        "continue"
                    };
                    assignment.1.push(BeamEvent {
                        beam_type: bt.to_string(),
                        number: level,
                    });
                }
            }
        }

        // Apply to elements
        for (elem_idx, beams) in assignments {
            if beams.is_empty() { continue; }
            match &mut elements[elem_idx] {
                VoiceElement::Note(n) => n.beams = beams,
                VoiceElement::Chord(c) => {
                    if let Some(first) = c.notes.first_mut() {
                        first.beams = beams;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Compute beam grouping spans for auto-beaming.
/// Returns (primary_span, sub_span):
/// - primary_span: grouping unit for level-1 beams (8ths)
/// - sub_span: grouping unit for level-2+ beams (16ths, 32nds)
fn compute_beam_spans(ts: &TimeSignature) -> (Frac, Frac) {
    let beats: i64 = ts
        .beats
        .split('+')
        .filter_map(|b| b.trim().parse::<i64>().ok())
        .sum();
    let bt = ts.beat_type as i64;

    if bt == 0 || beats == 0 {
        let q = Frac::new(1, 4);
        return (q, q);
    }

    // Compound meters: dotted beat for all levels
    if beats % 3 == 0 && beats > 3 && (bt == 8 || bt == 16) {
        let dotted = Frac::new(3, bt);
        return (dotted, dotted);
    }

    let beat = Frac::new(1, bt);

    // Simple quadruple: 8ths group per half note, sub-beams per beat
    if beats == 4 && bt == 4 {
        let half = Frac::new(1, 2);
        return (half, beat);
    }

    // Other simple meters: both levels group per beat
    (beat, beat)
}

/// Apply automatic stem directions to notes that don't have explicit stems.
fn auto_stem_voice(elements: &mut [VoiceElement], clef: &Clef) {
    let mid = middle_line_midi(clef);
    for elem in elements {
        match elem {
            VoiceElement::Note(n) => {
                if n.stem_direction.is_empty() && !n.is_grace {
                    n.stem_direction = auto_stem_for_midi(n.pitch.midi_number(), mid);
                }
            }
            VoiceElement::Chord(c) => {
                if c.notes.is_empty() {
                    continue;
                }
                let has_explicit = c.notes.iter().any(|n| !n.stem_direction.is_empty());
                if !has_explicit {
                    let avg_midi: f64 = c.notes.iter().map(|n| n.pitch.midi_number() as f64).sum::<f64>()
                        / c.notes.len() as f64;
                    let dir = if avg_midi >= mid as f64 { "down" } else { "up" };
                    for n in &mut c.notes {
                        n.stem_direction = dir.to_string();
                    }
                }
            }
            _ => {}
        }
    }
}

/// MIDI number of the middle staff line for a given clef.
fn middle_line_midi(clef: &Clef) -> i32 {
    let base = match clef.sign {
        ClefSign::G => {
            67 + (3 - clef.line as i32) * 2
        }
        ClefSign::F => {
            53 + (3 - clef.line as i32) * 2
        }
        ClefSign::C => {
            60 + (3 - clef.line as i32) * 2
        }
        _ => 71, // default to treble
    };
    base + clef.octave_change as i32 * 12
}

/// Determine automatic stem direction based on MIDI number and middle line.
fn auto_stem_for_midi(midi: i32, middle: i32) -> String {
    if midi >= middle {
        "down".to_string()
    } else {
        "up".to_string()
    }
}
