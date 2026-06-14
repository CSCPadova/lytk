use crate::ir::articulation::{BeamEvent, SlurEvent, StartStop, TieEvent};
use crate::ir::duration::Frac;
use crate::ir::measure::{Clef, ClefSign, TimeSignature};
use crate::ir::note::VoiceElement;
use crate::ir::pitch::Pitch;
use crate::ir::score::{Score, ScoreChild};
use crate::ir::Part;

use super::merge::beam_level_for_duration;

/// Resolve tie *stops*. The LY walk records only a tie *start* (from `~`) on the
/// note/chord that precedes the tie; the destination note never receives a
/// matching `<tie type="stop">`, leaving every tie dangling. This pass walks
/// each voice's note stream (across measure boundaries) and, for every tie-start
/// at pitch P, adds a tie-stop to the next element in that voice that contains
/// pitch P.
///
/// Must run after `q` chord-repeat expansion so a tie whose destination is a
/// `q`-repeat has a real note to stop on.
pub(super) fn resolve_ties(score: &mut Score) {
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            resolve_ties_in_part(part);
        }
    }
}

fn same_pitch(a: &Pitch, b: &Pitch) -> bool {
    a.step == b.step && a.alter == b.alter && a.octave == b.octave
}

/// Pitches carrying a tie-start on this element.
fn tie_start_pitches(elem: &VoiceElement) -> Vec<Pitch> {
    let has_start = |ties: &[TieEvent]| ties.iter().any(|t| t.tie_type == StartStop::Start);
    match elem {
        VoiceElement::Note(n) if has_start(&n.ties) => vec![n.pitch],
        VoiceElement::Chord(c) => c
            .notes
            .iter()
            .filter(|n| has_start(&n.ties))
            .map(|n| n.pitch)
            .collect(),
        _ => Vec::new(),
    }
}

fn element_has_pitch(elem: &VoiceElement, p: &Pitch) -> bool {
    match elem {
        VoiceElement::Note(n) => same_pitch(&n.pitch, p),
        VoiceElement::Chord(c) => c.notes.iter().any(|n| same_pitch(&n.pitch, p)),
        _ => false,
    }
}

/// Add a tie-stop to the note matching `p` (inserted before any existing
/// tie-start so a mid-chain note emits stop-then-start, per MusicXML).
fn add_tie_stop(elem: &mut VoiceElement, p: &Pitch) {
    let stop = TieEvent {
        tie_type: StartStop::Stop,
    };
    let apply = |n: &mut crate::ir::Note| {
        if n.ties.iter().any(|t| t.tie_type == StartStop::Stop) {
            return; // already stopped
        }
        n.ties.insert(0, stop.clone());
    };
    match elem {
        VoiceElement::Note(n) if same_pitch(&n.pitch, p) => apply(n),
        VoiceElement::Chord(c) => {
            if let Some(n) = c.notes.iter_mut().find(|n| same_pitch(&n.pitch, p)) {
                apply(n);
            }
        }
        _ => {}
    }
}

/// Assign MusicXML slur `number`s. The LY walk hardcodes every slur to
/// `number = 1`. In a piano part (one MusicXML part, two staves) a slur in the
/// right hand and one in the left hand are then both `number=1`; importers that
/// match slur start/stop by number across the whole part pair the RH start with
/// the LH stop, producing a giant slur swooping across both staves.
///
/// This pass allocates numbers so that no two *concurrently open* slurs anywhere
/// in the part share one, while a start and its matching stop (same voice, LIFO)
/// get the same number.
pub(super) fn assign_slur_numbers(score: &mut Score) {
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            assign_slur_numbers_in_part(part);
        }
    }
}

fn set_slur_numbers(slurs: &mut [SlurEvent], lane: u8) {
    for s in slurs {
        s.number = lane;
    }
}

fn assign_slur_numbers_in_part(part: &mut Part) {
    use std::collections::{BTreeSet, HashMap};
    // Voices carrying any slur, in stable order. Each gets a distinct "lane"
    // number so that slurs in different voices (e.g. the two hands of a piano)
    // never share a `number` and cannot be cross-paired by a renderer. Slurs
    // within a single voice are sequential (LilyPond requires `\=` for true
    // overlap, which this fixture does not use), so one lane per voice suffices
    // and a start/stop pair in that voice keeps the same number.
    let mut voices_with_slurs: BTreeSet<u8> = BTreeSet::new();
    for m in &part.measures {
        for v in &m.voices {
            let has_slur = v.elements.iter().any(|e| match e {
                VoiceElement::Note(n) => !n.slurs.is_empty(),
                VoiceElement::Chord(c) => c.notes.iter().any(|n| !n.slurs.is_empty()),
                VoiceElement::Rest(_) => false,
            });
            if has_slur {
                voices_with_slurs.insert(v.number);
            }
        }
    }
    let lane: HashMap<u8, u8> = voices_with_slurs
        .iter()
        .enumerate()
        .map(|(i, &vn)| (vn, (i as u8).saturating_add(1)))
        .collect();
    for m in &mut part.measures {
        for v in &mut m.voices {
            let ln = lane.get(&v.number).copied().unwrap_or(1);
            for elem in &mut v.elements {
                match elem {
                    VoiceElement::Note(n) => set_slur_numbers(&mut n.slurs, ln),
                    VoiceElement::Chord(c) => {
                        for note in &mut c.notes {
                            set_slur_numbers(&mut note.slurs, ln);
                        }
                    }
                    VoiceElement::Rest(_) => {}
                }
            }
        }
    }
}

fn resolve_ties_in_part(part: &mut Part) {
    use std::collections::BTreeMap;
    // Per voice number, the ordered (measure, voice-index, element-index) of
    // every Note/Chord in document order.
    let mut by_voice: BTreeMap<u8, Vec<(usize, usize, usize)>> = BTreeMap::new();
    for (mi, m) in part.measures.iter().enumerate() {
        for (vi, v) in m.voices.iter().enumerate() {
            for (ei, e) in v.elements.iter().enumerate() {
                if matches!(e, VoiceElement::Note(_) | VoiceElement::Chord(_)) {
                    by_voice.entry(v.number).or_default().push((mi, vi, ei));
                }
            }
        }
    }
    // Collect the stops to apply (deferred to avoid overlapping borrows).
    let mut stops: Vec<(usize, usize, usize, Pitch)> = Vec::new();
    for seq in by_voice.values() {
        for w in seq.windows(2) {
            let (m0, v0, e0) = w[0];
            let (m1, v1, e1) = w[1];
            let starts = tie_start_pitches(&part.measures[m0].voices[v0].elements[e0]);
            for p in starts {
                if element_has_pitch(&part.measures[m1].voices[v1].elements[e1], &p) {
                    stops.push((m1, v1, e1, p));
                }
            }
        }
    }
    for (m, v, e, p) in stops {
        add_tie_stop(&mut part.measures[m].voices[v].elements[e], &p);
    }
}

/// Ensure every staff of a multi-staff part has an initial clef.
///
/// LilyPond leaves the default clef (treble) implicit, so a piano part whose
/// upper staff has no explicit `\clef` records no clef for it. MusicXML then
/// emits a single clef with no `number=` and the upper staff shows the wrong
/// (or default) clef. This fills the first measure with a treble clef for any
/// staff that has none, so each staff gets a numbered `<clef>`. Defaulting a
/// missing staff to treble is also correct when that staff changes clef later
/// (it was treble until the change).
pub(super) fn ensure_staff_clefs(score: &mut Score) {
    use crate::ir::measure::MeasureAttributes;
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            if part.staves <= 1 {
                continue;
            }
            if let Some(m) = part.measures.first_mut() {
                let attrs = m.attributes.get_or_insert_with(MeasureAttributes::default);
                for staff in 1..=part.staves {
                    attrs.clefs.entry(staff).or_insert_with(Clef::default);
                }
            }
        }
    }
}

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
                let ts = current_ts.clone().unwrap_or(TimeSignature {
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
                let has_beam = c
                    .notes
                    .first()
                    .is_some_and(|n| !n.beams.is_empty() || n.no_auto_beam);
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
        }
    }

    let span_of = |pos: Frac, span: Frac| -> i64 {
        if span <= Frac::from_integer(0) {
            return 0;
        }
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
                    if ni.beam_level < level || span_of(ni.position, sub_span) != sub_beat {
                        break;
                    }
                    si += 1;
                }
                let sub_len = si - sub_start;
                if sub_len < 2 {
                    // Single note at this level: use a hook
                    let is_at_end = sub_start + 1 >= final_len;
                    let hook = if is_at_end {
                        "backward hook"
                    } else {
                        "forward hook"
                    };
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
            if beams.is_empty() {
                continue;
            }
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
                    let avg_midi: f64 = c
                        .notes
                        .iter()
                        .map(|n| n.pitch.midi_number() as f64)
                        .sum::<f64>()
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
        ClefSign::G => 67 + (3 - clef.line as i32) * 2,
        ClefSign::F => 53 + (3 - clef.line as i32) * 2,
        ClefSign::C => 60 + (3 - clef.line as i32) * 2,
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
