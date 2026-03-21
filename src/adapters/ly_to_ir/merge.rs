use std::collections::BTreeMap;

use crate::ir::articulation::{StartStop, TupletDisplay};
use crate::ir::direction::Direction;
use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::FiguredBass;
use crate::ir::measure::{Measure, MeasureAttributes, TimeSignature};
use crate::ir::note::VoiceElement;
use crate::ir::voice::Voice;
use crate::ir::Part;

use super::MeasureMeta;

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

pub(super) fn voice_element_duration(elem: &VoiceElement) -> Frac {
    match elem {
        VoiceElement::Note(n) => n.duration.actual_duration(),
        VoiceElement::Rest(r) => r.duration.actual_duration(),
        VoiceElement::Chord(c) => c.duration.actual_duration(),
    }
}

/// Check if a part contains only rests/spacers and directions (no actual notes or chords).
/// This identifies parts created from Dynamics contexts, which may have non-spacer rests
/// (e.g. from unhandled \skip commands) but never have pitched content.
pub(super) fn part_is_dynamics_only(part: &Part) -> bool {
    for m in &part.measures {
        for voice in &m.voices {
            for elem in &voice.elements {
                match elem {
                    VoiceElement::Note(_) | VoiceElement::Chord(_) => return false,
                    _ => {} // Rests (spacer or not), Forward, Backup are OK
                }
            }
        }
    }
    true
}

pub(super) fn measures_are_spacer_only(measures: &[Measure]) -> bool {
    for m in measures {
        for voice in &m.voices {
            for elem in &voice.elements {
                match elem {
                    VoiceElement::Rest(r) if r.is_spacer || r.is_measure_rest => {}
                    VoiceElement::Rest(_) | VoiceElement::Note(_) | VoiceElement::Chord(_) => {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Like `measures_are_spacer_only` but also allows regular rests (non-spacer).
/// Returns true if measures contain only rests/spacers — no notes or chords.
pub(super) fn measures_have_no_pitched_content(measures: &[Measure]) -> bool {
    for m in measures {
        for voice in &m.voices {
            for elem in &voice.elements {
                match elem {
                    VoiceElement::Rest(_) => {}
                    VoiceElement::Note(_) | VoiceElement::Chord(_) => {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Merge attributes, directions, and barlines from spacer-only measures into
/// existing measures. This handles the LilyPond pattern `<<\music \forma>>`
/// where `forma` carries time/key/tempo attributes with spacer rests.
pub(super) fn merge_spacer_measures(target: &mut [Measure], spacer: &[Measure]) {
    for (i, sm) in spacer.iter().enumerate() {
        if i < target.len() {
            let tm = &mut target[i];
            // Merge attributes
            if let Some(ref sa) = sm.attributes {
                let ta = tm.attributes.get_or_insert_with(MeasureAttributes::default);
                if sa.key.is_some() && ta.key.is_none() {
                    ta.key = sa.key;
                }
                if sa.time.is_some() && ta.time.is_none() {
                    ta.time = sa.time.clone();
                }
                if !sa.clefs.is_empty() && ta.clefs.is_empty() {
                    ta.clefs = sa.clefs.clone();
                }
            }
            // Merge directions (tempo, dynamics, pedal, etc.)
            if !sm.directions.is_empty() {
                tm.directions.extend(sm.directions.iter().cloned());
            }
            // Propagate dynamics from spacer rest elements to measure-level directions.
            for voice in &sm.voices {
                for elem in &voice.elements {
                    if let VoiceElement::Rest(r) = elem {
                        for dyn_mark in &r.dynamics {
                            tm.directions.push(Direction {
                                dynamic: Some(dyn_mark.clone()),
                                ..Direction::default()
                            });
                        }
                        for wedge in &r.wedges {
                            tm.directions.push(Direction {
                                wedge: Some(wedge.clone()),
                                ..Direction::default()
                            });
                        }
                    }
                }
            }
            // Merge barlines
            if sm.right_barline.is_some() && tm.right_barline.is_none() {
                tm.right_barline = sm.right_barline.clone();
            }
            if sm.left_barline.is_some() && tm.left_barline.is_none() {
                tm.left_barline = sm.left_barline.clone();
            }
        }
        // If spacer has more measures than target, we don't append them
        // (they're just spacers and don't contribute music)
    }
}

/// Merge directions and barlines from spacer-only measures into target measures
/// using cumulative duration alignment. This is needed when spacer measures have
/// different boundaries than target measures (e.g. dynamics pre-parsed at a
/// different time signature).
pub(super) fn merge_spacer_by_duration(target: &mut [Measure], spacer: &[Measure]) {
    // Compute cumulative duration boundaries for target measures
    let mut target_boundaries: Vec<Frac> = Vec::with_capacity(target.len() + 1);
    let mut cumul = Frac::from_integer(0);
    target_boundaries.push(cumul);
    for tm in target.iter() {
        let dur = measure_voice_duration(tm);
        cumul += dur;
        target_boundaries.push(cumul);
    }

    // Walk spacer measures, accumulating duration and placing directions
    // (both measure-level and those attached to spacer rest elements)
    // into the correct target measure.
    let mut spacer_pos = Frac::from_integer(0);
    for sm in spacer.iter() {
        let sm_dur = measure_voice_duration(sm);

        // Find the target measure that contains spacer_pos
        let target_idx = target_boundaries
            .windows(2)
            .position(|w| spacer_pos >= w[0] && spacer_pos < w[1])
            .unwrap_or_else(|| target.len().saturating_sub(1));

        if target_idx < target.len() {
            // Merge attributes (time sig, key, clef) from spacer into target
            if let Some(ref sa) = sm.attributes {
                let tm = &mut target[target_idx];
                let ta = tm
                    .attributes
                    .get_or_insert_with(crate::ir::measure::MeasureAttributes::default);
                if sa.key.is_some() && ta.key.is_none() {
                    ta.key = sa.key;
                }
                if sa.time.is_some() && ta.time.is_none() {
                    ta.time = sa.time.clone();
                }
                if !sa.clefs.is_empty() && ta.clefs.is_empty() {
                    ta.clefs = sa.clefs.clone();
                }
            }
            // Merge measure-level directions, adjusting offset_frac to be within the target measure.
            // Each direction is distributed individually to the target measure that contains its
            // absolute position — not just to target_idx (which is based on spacer_pos).
            // This is critical when the pedal variable has no time sig: all events are in one
            // big spacer measure (spacer_pos=0) but must fan out across many target measures.
            for dir in sm.directions.iter() {
                let abs_pos = spacer_pos + dir.offset_frac;
                let t_idx = target_boundaries
                    .windows(2)
                    .position(|w| abs_pos >= w[0] && abs_pos < w[1])
                    .unwrap_or_else(|| target.len().saturating_sub(1));
                if t_idx < target.len() {
                    let target_start = target_boundaries[t_idx];
                    let mut d = dir.clone();
                    d.offset_frac = if abs_pos >= target_start {
                        abs_pos - target_start
                    } else {
                        Frac::from_integer(0)
                    };
                    target[t_idx].directions.push(d);
                }
            }
            // Merge barlines into the spacer's primary target measure
            let tm = &mut target[target_idx];
            if sm.right_barline.is_some() && tm.right_barline.is_none() {
                tm.right_barline = sm.right_barline.clone();
            }
            if sm.left_barline.is_some() && tm.left_barline.is_none() {
                tm.left_barline = sm.left_barline.clone();
            }
        }

        // Also distribute dynamics attached to individual spacer rest elements.
        // When a spacer variable has no time signature, all its rests land in a
        // single pre-parsed measure. We need to walk them by cumulative duration
        // so each dynamic ends up in the correct target measure.
        for voice in &sm.voices {
            let mut vpos = spacer_pos;
            for elem in &voice.elements {
                let dur = voice_element_duration(elem);
                if let VoiceElement::Rest(r) = elem {
                    if !r.dynamics.is_empty() || !r.wedges.is_empty() {
                        let tidx = target_boundaries
                            .windows(2)
                            .position(|w| vpos >= w[0] && vpos < w[1])
                            .unwrap_or_else(|| target.len().saturating_sub(1));
                        if tidx < target.len() {
                            let target_start = target_boundaries[tidx];
                            let within_offset = if vpos >= target_start {
                                vpos - target_start
                            } else {
                                Frac::from_integer(0)
                            };
                            for dyn_mark in &r.dynamics {
                                target[tidx].directions.push(Direction {
                                    dynamic: Some(dyn_mark.clone()),
                                    offset_frac: within_offset,
                                    ..Direction::default()
                                });
                            }
                            for wedge in &r.wedges {
                                target[tidx].directions.push(Direction {
                                    wedge: Some(wedge.clone()),
                                    offset_frac: within_offset,
                                    ..Direction::default()
                                });
                            }
                        }
                    }
                }
                vpos += dur;
            }
        }

        spacer_pos += sm_dur;
    }
}

/// Compute the duration of a measure from its first voice's elements.
pub(super) fn measure_voice_duration(m: &Measure) -> Frac {
    if let Some(voice) = m.voices.first() {
        voice
            .elements
            .iter()
            .map(voice_element_duration)
            .fold(Frac::from_integer(0), |acc, d| acc + d)
    } else {
        Frac::from_integer(0)
    }
}

// ---------------------------------------------------------------------------
// Multi-voice merge helpers
// ---------------------------------------------------------------------------

/// Set the voice number on all Voice objects and their contained elements.
pub(super) fn renumber_voices_in_measures(measures: Vec<Measure>, voice_num: u8) -> Vec<Measure> {
    measures
        .into_iter()
        .map(|mut m| {
            for voice in &mut m.voices {
                voice.number = voice_num;
                for elem in &mut voice.elements {
                    match elem {
                        VoiceElement::Note(n) => n.voice = voice_num,
                        VoiceElement::Rest(r) => r.voice = voice_num,
                        VoiceElement::Chord(c) => {
                            c.voice = voice_num;
                            for n in &mut c.notes {
                                n.voice = voice_num;
                            }
                        }
                    }
                }
            }
            m
        })
        .collect()
}

/// Merge N voice-specific measure streams into a single stream.
///
/// For each measure index, combines all voices from all streams into
/// a single measure. Attributes are merged (first non-None wins),
/// directions are concatenated, barlines take first non-None.
pub(super) fn merge_voice_measure_streams(streams: &[Vec<Measure>]) -> Vec<Measure> {
    if streams.is_empty() {
        return Vec::new();
    }
    let max_len = streams.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut result = Vec::with_capacity(max_len);

    for i in 0..max_len {
        // Start with the first stream's measure as the base
        let mut merged = if let Some(m) = streams[0].get(i) {
            m.clone()
        } else {
            Measure::new((i + 1) as u32)
        };

        // Merge voices from subsequent streams
        for stream in &streams[1..] {
            if let Some(m) = stream.get(i) {
                // Add this stream's voices to the merged measure
                merged.voices.extend(m.voices.iter().cloned());

                // Merge attributes: first non-None wins for each field
                if let Some(ref src_attrs) = m.attributes {
                    let dst = merged
                        .attributes
                        .get_or_insert_with(MeasureAttributes::default);
                    if src_attrs.key.is_some() && dst.key.is_none() {
                        dst.key = src_attrs.key;
                    }
                    if src_attrs.time.is_some() && dst.time.is_none() {
                        dst.time = src_attrs.time.clone();
                    }
                    if !src_attrs.clefs.is_empty() && dst.clefs.is_empty() {
                        dst.clefs = src_attrs.clefs.clone();
                    }
                }

                // Merge directions (extend, don't replace)
                merged.directions.extend(m.directions.iter().cloned());

                // Barlines: first non-None wins
                if m.left_barline.is_some() && merged.left_barline.is_none() {
                    merged.left_barline = m.left_barline.clone();
                }
                if m.right_barline.is_some() && merged.right_barline.is_none() {
                    merged.right_barline = m.right_barline.clone();
                }
            }
        }

        result.push(merged);
    }

    result
}

// ---------------------------------------------------------------------------
// Post-processing: attribute merging and tempo propagation
// ---------------------------------------------------------------------------

/// Merge attribute-only leading measures into the first measure with actual voices.
///
/// When `\global` (containing `\key` / `\time` but no notes) is resolved before
/// the music variable, the part gets leading measures with attributes but empty
/// voices. This merges those attributes into the first real-music measure and
/// removes the empty leading measures.
pub(super) fn merge_leading_attribute_measures(part: &mut Part) {
    // Find the first measure with non-empty voice content
    let first_music_idx = part
        .measures
        .iter()
        .position(|m| m.voices.iter().any(|v| !v.elements.is_empty()));

    let idx = match first_music_idx {
        Some(0) | None => return, // nothing to merge or no music at all
        Some(i) => i,
    };

    // Collect attributes, directions, and figured bass from all leading attribute-only measures
    let mut merged_key = None;
    let mut merged_time = None;
    let mut merged_clefs = std::collections::HashMap::new();
    let mut merged_dirs: Vec<Direction> = Vec::new();
    let mut merged_left_barline = None;
    let mut merged_figured_bass: Vec<FiguredBass> = Vec::new();

    for m in &part.measures[..idx] {
        if let Some(ref attrs) = m.attributes {
            if attrs.key.is_some() {
                merged_key = attrs.key;
            }
            if attrs.time.is_some() {
                merged_time = attrs.time.clone();
            }
            if !attrs.clefs.is_empty() {
                merged_clefs.extend(attrs.clefs.iter().map(|(&k, v)| (k, *v)));
            }
        }
        merged_dirs.extend(m.directions.iter().cloned());
        merged_figured_bass.extend(m.figured_bass.iter().cloned());
        if m.left_barline.is_some() && merged_left_barline.is_none() {
            merged_left_barline = m.left_barline.clone();
        }
    }

    // Apply collected attributes to the first music measure
    let target = &mut part.measures[idx];
    let ta = target
        .attributes
        .get_or_insert_with(MeasureAttributes::default);
    if merged_key.is_some() && ta.key.is_none() {
        ta.key = merged_key;
    }
    if merged_time.is_some() && ta.time.is_none() {
        ta.time = merged_time;
    }
    if !merged_clefs.is_empty() && ta.clefs.is_empty() {
        ta.clefs = merged_clefs;
    }

    // Prepend directions from attribute-only measures
    if !merged_dirs.is_empty() {
        let existing_dirs = std::mem::take(&mut target.directions);
        target.directions = merged_dirs;
        target.directions.extend(existing_dirs);
    }

    // Prepend figured bass from attribute-only measures
    if !merged_figured_bass.is_empty() {
        let existing_fb = std::mem::take(&mut target.figured_bass);
        target.figured_bass = merged_figured_bass;
        target.figured_bass.extend(existing_fb);
    }

    // Remove the leading empty measures and renumber
    part.measures.drain(0..idx);
    for (i, m) in part.measures.iter_mut().enumerate() {
        m.number = (i + 1) as u32;
    }
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

///
/// Dynamics contexts produce parts containing only spacer rests with Direction
/// annotations (dynamics, pedal markings). These should not be separate parts;
/// their directions should be merged into the nearest staff part.
pub(super) fn merge_dynamics_parts(parts: &mut Vec<(String, Part)>) {
    if parts.len() <= 1 {
        return;
    }
    // Identify spacer-only parts and their merge targets
    let mut merges: Vec<(usize, usize)> = Vec::new(); // (spacer_idx, target_idx)

    for i in 0..parts.len() {
        if parts[i].1.measures.is_empty() || !part_is_dynamics_only(&parts[i].1) {
            continue;
        }
        // Find nearest non-spacer part: prefer previous, fall back to next
        let target = if i > 0
            && !parts[i - 1].1.measures.is_empty()
            && !measures_are_spacer_only(&parts[i - 1].1.measures)
        {
            Some(i - 1)
        } else {
            (i + 1..parts.len()).find(|&j| {
                !parts[j].1.measures.is_empty() && !measures_are_spacer_only(&parts[j].1.measures)
            })
        };
        if let Some(t) = target {
            merges.push((i, t));
        }
    }

    // Perform merges (clone spacer measures first to avoid borrow conflicts)
    let spacer_data: Vec<(usize, Vec<Measure>)> = merges
        .iter()
        .map(|&(si, _)| (si, parts[si].1.measures.clone()))
        .collect();
    for ((_, ti), (_, spacer_measures)) in merges.iter().zip(spacer_data.iter()) {
        if spacer_measures.len() != parts[*ti].1.measures.len() {
            // Different measure counts — spacer was pre-parsed at a different
            // time sig, so merge by cumulative duration alignment.
            merge_spacer_by_duration(&mut parts[*ti].1.measures, spacer_measures);
        } else {
            merge_spacer_measures(&mut parts[*ti].1.measures, spacer_measures);
        }
    }

    // Remove merged spacer parts (in reverse order to maintain indices)
    let mut to_remove: Vec<usize> = merges.iter().map(|&(si, _)| si).collect();
    to_remove.sort_unstable();
    to_remove.dedup();
    for idx in to_remove.into_iter().rev() {
        parts.remove(idx);
    }
}

/// Re-split note measures to match the measure boundaries defined by spacer measures.
///
/// When a note variable (e.g. `IIvlIn`) was auto-split using the default time
/// signature during variable definition, but the spacer variable (`forma`) has
/// different time signatures, the measure boundaries are misaligned. This function
/// flattens all voice elements from the note measures and redistributes them into
/// new measures matching the spacer measures' actual durations.
pub(super) fn resplit_measures_to_match(
    note_measures: &[Measure],
    spacer_measures: &[Measure],
) -> Vec<Measure> {
    // 1. Collect per-voice element streams and note-measure attributes.
    // Each voice is tracked independently so multi-voice structure is preserved.
    let mut voice_streams: BTreeMap<u8, Vec<VoiceElement>> = BTreeMap::new();
    let mut note_measure_attrs: Vec<(Frac, MeasureAttributes)> = Vec::new();
    let mut cumul_dur = Frac::from_integer(0);

    for m in note_measures {
        if let Some(ref attrs) = m.attributes {
            note_measure_attrs.push((cumul_dur, attrs.clone()));
        }
        let mut measure_dur = Frac::from_integer(0);
        for v in &m.voices {
            let mut vdur = Frac::from_integer(0);
            let stream = voice_streams.entry(v.number).or_default();
            for e in &v.elements {
                vdur += voice_element_duration(e);
                stream.push(e.clone());
            }
            if vdur > measure_dur {
                measure_dur = vdur;
            }
        }
        cumul_dur += measure_dur;
    }

    // 2. Compute actual duration of each spacer measure from its content
    let spacer_durations: Vec<Frac> = spacer_measures
        .iter()
        .map(|m| {
            let mut dur = Frac::from_integer(0);
            for v in &m.voices {
                for e in &v.elements {
                    dur += voice_element_duration(e);
                }
            }
            dur
        })
        .collect();

    // 3. Re-distribute elements per voice into new measures
    let voice_nums: Vec<u8> = voice_streams.keys().copied().collect();
    let mut voice_indices: BTreeMap<u8, usize> =
        voice_nums.iter().map(|&vn| (vn, 0usize)).collect();

    let mut result: Vec<Measure> = Vec::new();
    let mut note_attr_idx = 0usize;
    let mut output_cumul = Frac::from_integer(0);

    for (si, sm) in spacer_measures.iter().enumerate() {
        let measure_dur = spacer_durations[si];
        let mut new_measure = Measure::new(sm.number);
        new_measure.implicit = sm.implicit;
        // Copy attributes from spacer (has correct time sig, key, etc.)
        new_measure.attributes = sm.attributes.clone();
        new_measure.directions = sm.directions.clone();
        new_measure.left_barline = sm.left_barline.clone();
        new_measure.right_barline = sm.right_barline.clone();

        // Fill each voice independently up to this measure's duration
        for &vn in &voice_nums {
            let stream = &voice_streams[&vn];
            let mut cur_idx = voice_indices[&vn];
            let mut elapsed = Frac::from_integer(0);
            let mut voice_elements: Vec<VoiceElement> = Vec::new();

            while cur_idx < stream.len() && measure_dur > Frac::from_integer(0) {
                let dur = voice_element_duration(&stream[cur_idx]);
                if elapsed + dur > measure_dur && elapsed > Frac::from_integer(0) {
                    break;
                }
                voice_elements.push(stream[cur_idx].clone());
                elapsed += dur;
                cur_idx += 1;
                if elapsed >= measure_dur {
                    break;
                }
            }

            *voice_indices.get_mut(&vn).unwrap() = cur_idx;

            if !voice_elements.is_empty() {
                new_measure.voices.push(Voice {
                    number: vn,
                    elements: voice_elements,
                });
            }
        }

        // Merge attributes from note measures whose cumulative position falls
        // within this output measure's duration range
        let output_end = output_cumul + measure_dur;
        while note_attr_idx < note_measure_attrs.len()
            && note_measure_attrs[note_attr_idx].0 < output_end
        {
            let (_attr_pos, ref note_attrs) = note_measure_attrs[note_attr_idx];
            let ma = new_measure
                .attributes
                .get_or_insert_with(MeasureAttributes::default);
            if !note_attrs.clefs.is_empty() && ma.clefs.is_empty() {
                ma.clefs = note_attrs.clefs.clone();
            }
            if note_attrs.time.is_some() && ma.time.is_none() {
                ma.time = note_attrs.time.clone();
            }
            if note_attrs.key.is_some() && ma.key.is_none() {
                ma.key = note_attrs.key;
            }
            note_attr_idx += 1;
        }

        output_cumul = output_end;
        result.push(new_measure);
    }

    // 4. Handle remaining elements beyond spacer measures
    let last_dur = spacer_durations.last().copied().unwrap_or(Frac::new(4, 4));
    loop {
        let any_remaining = voice_nums
            .iter()
            .any(|&vn| voice_indices[&vn] < voice_streams[&vn].len());
        if !any_remaining {
            break;
        }

        let mnum = result.len() as u32 + 1;
        let mut m = Measure::new(mnum);

        for &vn in &voice_nums {
            let stream = &voice_streams[&vn];
            let mut cur_idx = voice_indices[&vn];
            if cur_idx >= stream.len() {
                continue;
            }

            let mut elapsed = Frac::from_integer(0);
            let mut voice_elements: Vec<VoiceElement> = Vec::new();

            while cur_idx < stream.len() {
                let dur = voice_element_duration(&stream[cur_idx]);
                if elapsed + dur > last_dur && elapsed > Frac::from_integer(0) {
                    break;
                }
                voice_elements.push(stream[cur_idx].clone());
                elapsed += dur;
                cur_idx += 1;
                if elapsed >= last_dur {
                    break;
                }
            }

            *voice_indices.get_mut(&vn).unwrap() = cur_idx;

            if !voice_elements.is_empty() {
                m.voices.push(Voice {
                    number: vn,
                    elements: voice_elements,
                });
            }
        }

        result.push(m);
    }

    result
}

/// Re-split measures using a new time signature, preserving multi-voice structure.
///
/// When a variable was pre-parsed with one time signature (e.g. default 4/4) but
/// is resolved in a context with a different time signature (e.g. 6/8), the measure
/// boundaries are wrong. This function flattens each voice independently and
/// re-distributes elements into new measures at the correct boundaries.
pub(super) fn resplit_measures_for_time_sig(
    measures: &[Measure],
    target_time_sig: Frac,
) -> Vec<Measure> {
    if measures.is_empty() || target_time_sig <= Frac::from_integer(0) {
        return measures.to_vec();
    }

    // 1. Collect all voice numbers and their elements in order
    let mut voice_elements: BTreeMap<u8, Vec<VoiceElement>> = BTreeMap::new();
    // Also collect attributes/directions/barlines from original measures,
    // keyed by cumulative duration position (start of that measure in voice 1)
    let mut measure_attrs: Vec<MeasureMeta> = Vec::new();
    let mut cumulative_pos = Frac::from_integer(0);

    for m in measures {
        measure_attrs.push((
            cumulative_pos,
            m.attributes.clone(),
            m.directions.clone(),
            m.left_barline.clone(),
            m.right_barline.clone(),
        ));
        // Compute measure duration from the first (longest) voice
        let mut max_dur = Frac::from_integer(0);
        for v in &m.voices {
            let voice_num = v.number;
            let entry = voice_elements.entry(voice_num).or_default();
            let mut voice_dur = Frac::from_integer(0);
            for e in &v.elements {
                voice_dur += voice_element_duration(e);
                entry.push(e.clone());
            }
            if voice_dur > max_dur {
                max_dur = voice_dur;
            }
        }
        cumulative_pos += max_dur;
    }

    if voice_elements.is_empty() {
        return measures.to_vec();
    }

    // 2. Re-split each voice's elements into measures by the target time sig
    let voice_nums: Vec<u8> = voice_elements.keys().copied().collect();
    let mut voice_split: BTreeMap<u8, Vec<Vec<VoiceElement>>> = BTreeMap::new();

    for &vn in &voice_nums {
        let elements = voice_elements.remove(&vn).unwrap();
        let mut split_measures: Vec<Vec<VoiceElement>> = Vec::new();
        let mut current: Vec<VoiceElement> = Vec::new();
        let mut elapsed = Frac::from_integer(0);

        for elem in elements {
            let dur = voice_element_duration(&elem);
            // Check if adding this element would exceed the measure
            if elapsed >= target_time_sig && elapsed > Frac::from_integer(0) {
                split_measures.push(std::mem::take(&mut current));
                elapsed -= target_time_sig;
            }
            current.push(elem);
            elapsed += dur;
        }
        if !current.is_empty() {
            split_measures.push(current);
        }
        voice_split.insert(vn, split_measures);
    }

    // 3. Determine the number of output measures (max across all voices)
    let num_measures = voice_split.values().map(|v| v.len()).max().unwrap_or(0);

    // 4. Build output measures by combining voices
    let mut result: Vec<Measure> = Vec::new();
    // Track which original-measure attributes we've consumed
    let mut attr_idx = 0usize;
    let mut out_cumulative = Frac::from_integer(0);

    for mi in 0..num_measures {
        let mut m = Measure::new(mi as u32 + 1);

        // Apply attributes/directions from original measures whose position falls
        // within this output measure's range
        let out_end = out_cumulative + target_time_sig;
        while attr_idx < measure_attrs.len() && measure_attrs[attr_idx].0 < out_end {
            let (_, ref attrs, ref dirs, ref lbar, ref rbar) = measure_attrs[attr_idx];
            if let Some(ref a) = attrs {
                let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                if ma.key.is_none() {
                    ma.key = a.key;
                }
                if ma.time.is_none() {
                    ma.time = a.time.clone();
                }
                if ma.clefs.is_empty() {
                    ma.clefs = a.clefs.clone();
                }
                if ma.staves.is_none() {
                    ma.staves = a.staves;
                }
                if ma.divisions == 0 && a.divisions > 0 {
                    ma.divisions = a.divisions;
                }
            }
            m.directions.extend(dirs.iter().cloned());
            if m.left_barline.is_none() {
                m.left_barline = lbar.clone();
            }
            if m.right_barline.is_none() {
                m.right_barline = rbar.clone();
            }
            attr_idx += 1;
        }
        out_cumulative = out_end;

        // Add each voice
        for &vn in &voice_nums {
            if let Some(split) = voice_split.get(&vn) {
                if let Some(elems) = split.get(mi) {
                    if !elems.is_empty() {
                        m.voices.push(Voice {
                            number: vn,
                            elements: elems.clone(),
                        });
                    }
                }
            }
        }

        result.push(m);
    }

    result
}

/// Synchronize time signatures across all parts in a score.
///
/// In LilyPond, time signature changes in one staff of a PianoStaff (or any
/// grouping) automatically apply to all staves. Our parser treats each staff
/// independently, so a `\time 4/4` in `voicea` doesn't affect `voiceb`.
///
/// This function:
/// 1. Collects all time signature changes from all parts with their cumulative
///    duration positions
/// 2. Re-splits any part whose measures don't align with the unified timeline
pub(super) fn synchronize_time_signatures(score: &mut crate::ir::score::Score) {
    let num_parts = score.parts().len();
    if num_parts < 2 {
        return;
    }

    // Collect time sig and key sig events by measure index from all parts.
    // Use the longest part's measure count as reference.
    let max_measures = score
        .parts()
        .iter()
        .map(|p| p.measures.len())
        .max()
        .unwrap_or(0);

    // Build a unified timeline: for each measure index, the time sig and key sig
    // that should apply (first non-None from any part wins).
    let mut unified_time: Vec<Option<TimeSignature>> = vec![None; max_measures];
    let mut unified_key: Vec<Option<crate::ir::measure::KeySignature>> = vec![None; max_measures];

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

    // Apply unified attributes to all parts
    for part in score.parts_mut().iter_mut() {
        for (mi, m) in part.measures.iter_mut().enumerate() {
            if mi >= max_measures {
                break;
            }
            // Copy time sig if this measure doesn't have one but unified does
            if let Some(ref ts) = unified_time[mi] {
                let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                if ma.time.is_none() {
                    ma.time = Some(ts.clone());
                }
            }
            // Copy key sig if this measure doesn't have one but unified does
            if let Some(ref ks) = unified_key[mi] {
                let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                if ma.key.is_none() {
                    ma.key = Some(*ks);
                }
            }
        }
    }
}

/// Re-split a part's measures using a sequence of time signature changes.
///
/// Similar to `resplit_measures_for_time_sig` but handles multiple time
/// signature changes instead of a single target.
#[allow(dead_code)]
pub(super) fn resplit_measures_with_time_changes(
    measures: &[Measure],
    time_events: &[(Frac, TimeSignature, Frac)],
    initial_time_sig: Frac,
) -> Vec<Measure> {
    if measures.is_empty() {
        return Vec::new();
    }

    // 1. Flatten: collect all voice elements and measure metadata
    let mut voice_elements: BTreeMap<u8, Vec<VoiceElement>> = BTreeMap::new();
    let mut measure_attrs: Vec<MeasureMeta> = Vec::new();
    let mut cumul = Frac::from_integer(0);
    let mut current_ts = initial_time_sig;

    for m in measures {
        if let Some(ref attrs) = m.attributes {
            if let Some(ref ts) = attrs.time {
                current_ts = ts.beats_fraction();
            }
        }
        measure_attrs.push((
            cumul,
            m.attributes.clone(),
            m.directions.clone(),
            m.left_barline.clone(),
            m.right_barline.clone(),
        ));
        let mut max_dur = Frac::from_integer(0);
        for v in &m.voices {
            let entry = voice_elements.entry(v.number).or_default();
            let mut vdur = Frac::from_integer(0);
            for e in &v.elements {
                vdur += voice_element_duration(e);
                entry.push(e.clone());
            }
            if vdur > max_dur {
                max_dur = vdur;
            }
        }
        if max_dur == Frac::from_integer(0) {
            max_dur = current_ts;
        }
        cumul += max_dur;
    }

    let total_duration = cumul;
    if voice_elements.is_empty() {
        return measures.to_vec();
    }

    // 2. Build measure boundaries from time_events
    let mut boundaries: Vec<(Frac, Option<TimeSignature>)> = Vec::new(); // (start_pos, time_sig_change)
    let mut pos = Frac::from_integer(0);
    let mut current_ts = initial_time_sig;

    // Find the initial time sig from events at position 0
    for (epos, _ets, efrac) in time_events {
        if *epos == Frac::from_integer(0) {
            current_ts = *efrac;
            break;
        }
    }

    while pos < total_duration {
        // Check if there's a time sig change at this position
        let mut ts_change: Option<TimeSignature> = None;
        for (epos, ets, efrac) in time_events {
            if *epos == pos {
                ts_change = Some(ets.clone());
                current_ts = *efrac;
                break;
            }
        }
        boundaries.push((pos, ts_change));
        pos += current_ts;
    }

    // 3. Split each voice's elements at the computed boundaries
    let voice_nums: Vec<u8> = voice_elements.keys().copied().collect();
    let mut voice_split: BTreeMap<u8, Vec<Vec<VoiceElement>>> = BTreeMap::new();

    for &vn in &voice_nums {
        let elements = voice_elements.remove(&vn).unwrap();
        let mut split_measures: Vec<Vec<VoiceElement>> = Vec::new();
        let mut current: Vec<VoiceElement> = Vec::new();
        let mut elem_pos = Frac::from_integer(0);
        let mut boundary_idx = 1; // Start checking from 2nd boundary

        for elem in elements {
            let dur = voice_element_duration(&elem);
            // Check if this element starts at or past the next boundary
            while boundary_idx < boundaries.len() && elem_pos >= boundaries[boundary_idx].0 {
                split_measures.push(std::mem::take(&mut current));
                boundary_idx += 1;
            }
            current.push(elem);
            elem_pos += dur;
        }
        if !current.is_empty() {
            split_measures.push(current);
        }
        voice_split.insert(vn, split_measures);
    }

    // 4. Build output measures
    let num_measures = boundaries
        .len()
        .max(voice_split.values().map(|v| v.len()).max().unwrap_or(0));
    let mut result: Vec<Measure> = Vec::new();
    let mut attr_idx = 0usize;

    for mi in 0..num_measures {
        let mut m = Measure::new(mi as u32 + 1);

        // Apply time sig change from the unified timeline
        if mi < boundaries.len() {
            if let Some(ref ts) = boundaries[mi].1 {
                let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                if ma.time.is_none() {
                    ma.time = Some(ts.clone());
                }
            }
        }

        // Apply attrs/dirs from original measures at overlapping positions
        let out_start = if mi < boundaries.len() {
            boundaries[mi].0
        } else {
            Frac::from_integer(0)
        };
        let out_end = if mi + 1 < boundaries.len() {
            boundaries[mi + 1].0
        } else {
            total_duration
        };

        while attr_idx < measure_attrs.len() && measure_attrs[attr_idx].0 < out_end {
            let (apos, ref attrs, ref dirs, ref lbar, ref rbar) = measure_attrs[attr_idx];
            if apos >= out_start {
                if let Some(ref a) = attrs {
                    let ma = m.attributes.get_or_insert_with(MeasureAttributes::default);
                    if ma.key.is_none() {
                        ma.key = a.key;
                    }
                    // Don't override time sig from unified timeline
                    if ma.time.is_none() {
                        ma.time = a.time.clone();
                    }
                    if ma.clefs.is_empty() {
                        ma.clefs = a.clefs.clone();
                    }
                    if ma.staves.is_none() {
                        ma.staves = a.staves;
                    }
                    if ma.divisions == 0 && a.divisions > 0 {
                        ma.divisions = a.divisions;
                    }
                }
                m.directions.extend(dirs.iter().cloned());
                if m.left_barline.is_none() {
                    m.left_barline = lbar.clone();
                }
                if m.right_barline.is_none() {
                    m.right_barline = rbar.clone();
                }
            }
            attr_idx += 1;
        }

        // Add voices
        for &vn in &voice_nums {
            if let Some(split) = voice_split.get(&vn) {
                if let Some(elems) = split.get(mi) {
                    if !elems.is_empty() {
                        m.voices.push(Voice {
                            number: vn,
                            elements: elems.clone(),
                        });
                    }
                }
            }
        }

        result.push(m);
    }

    result
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

/// Renumber all measures in a part sequentially starting from 1.
pub(super) fn renumber_measures(part: &mut Part) {
    for (i, m) in part.measures.iter_mut().enumerate() {
        m.number = (i + 1) as u32;
    }
}
