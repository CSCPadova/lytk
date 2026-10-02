//! Stems and beams where the source left them to the engraver, drawn as
//! LilyPond draws them by default. The readers keep only what a source says;
//! a writer that must spell stems or beams out (MusicXML, ABC, kern) engraves
//! a copy with [`engrave`], and the LilyPond writer compares a source's stems
//! with [`default_stems`] to write only those LilyPond wouldn't draw itself.

use std::collections::{BTreeMap, HashMap};

use crate::ir::articulation::BeamEvent;
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::{Clef, ClefSign, TimeSignature};
use crate::ir::note::{Note, VoiceElement};
use crate::ir::score::Score;
use crate::ir::Part;

/// Return the beam level for a note duration:
/// 0 = not beamable (quarter or longer), 1 = eighth, 2 = 16th, 3 = 32nd, 4 = 64th.
pub(crate) fn beam_level_for_duration(dur: &Duration) -> u8 {
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

/// Fill in what every part's source left to the engraver: stem directions
/// and beams. A note's own stem and beams are kept, and so is the beaming of
/// a note whose source decided it (`no_auto_beam`: `\autoBeamOff`, a
/// MusicXML file that beams, ABC's spacing). Source beams of the first level
/// only (`c16[ d e f]`) get their secondary beams.
pub(crate) fn engrave(score: &mut Score) {
    for part in score.parts_mut() {
        engrave_part(part);
    }
}

/// The part with the stems LilyPond would draw with no stem commands: its
/// own stems cleared, then engraved.
pub(crate) fn default_stems(part: &Part) -> Part {
    let mut part = part.clone();
    for e in part
        .measures
        .iter_mut()
        .flat_map(|m| &mut m.voices)
        .flat_map(|v| &mut v.elements)
    {
        for n in e.notes_mut() {
            n.stem_direction.clear();
        }
    }
    engrave_part(&mut part);
    part
}

/// An element's stem as the source gave it ("" when it didn't).
pub(crate) fn stem_of(e: &VoiceElement) -> &str {
    match e {
        VoiceElement::Note(n) => &n.stem_direction,
        VoiceElement::Chord(c) => c.notes.first().map_or("", |n| &n.stem_direction),
        VoiceElement::Rest(_) => "",
    }
}

fn engrave_part(part: &mut Part) {
    let mut meter: Option<TimeSignature> = None;
    let mut clefs: HashMap<u8, Clef> = HashMap::new(); // treble by default
    for (mi, measure) in part.measures.iter_mut().enumerate() {
        if let Some(attrs) = &measure.attributes {
            if let Some(ts) = &attrs.time {
                meter = Some(ts.clone());
            }
            clefs.extend(attrs.clefs.iter().map(|(s, c)| (*s, *c)));
        }
        let rules = Beaming::of(&meter.clone().unwrap_or_default());
        // A pickup bar is the end of a full one.
        let len = measure.content_length();
        let start = if mi == 0 && measure.implicit && len < rules.length {
            rules.length - len
        } else {
            Frac::from_integer(0)
        };
        // Voices sounding together on a staff take turns, up and down
        // (LilyPond's \voiceOne, \voiceTwo, ...).
        let mut on_staff: BTreeMap<u8, Vec<(u8, usize)>> = BTreeMap::new();
        for (vi, v) in measure.voices.iter().enumerate() {
            if v.elements.iter().any(sounds) {
                on_staff
                    .entry(voice_staff(&v.elements))
                    .or_default()
                    .push((v.number, vi));
            }
        }
        let mut forced: HashMap<usize, &'static str> = HashMap::new();
        for voices in on_staff.values_mut().filter(|v| v.len() > 1) {
            voices.sort_unstable();
            for (k, &(_, vi)) in voices.iter().enumerate() {
                forced.insert(vi, if k % 2 == 0 { "up" } else { "down" });
            }
        }
        // Clef changes inside the bar, by staff, in order.
        let mut changes: Vec<(u8, Frac, Clef)> = measure
            .directions
            .iter()
            .filter_map(|d| Some((d.staff.max(1), d.offset_frac, d.clef?)))
            .collect();
        changes.sort_by_key(|c| c.1);
        for (vi, voice) in measure.voices.iter_mut().enumerate() {
            let staff = voice_staff(&voice.elements);
            // Each element's middle line: the clef in force where it starts.
            let mut at = Frac::from_integer(0);
            let middles: Vec<i32> = voice
                .elements
                .iter()
                .map(|e| {
                    let clef = changes
                        .iter()
                        .rev()
                        .find(|c| c.0 == staff && c.1 <= at)
                        .map(|c| c.2)
                        .or_else(|| clefs.get(&staff).copied())
                        .unwrap_or_default();
                    at += e.metric_duration();
                    middle_line(&clef)
                })
                .collect();
            let groups = beam_voice(&mut voice.elements, &rules, start);
            stem_voice(
                &mut voice.elements,
                &groups,
                &middles,
                forced.get(&vi).copied(),
            );
        }
        clefs.extend(changes.into_iter().map(|(staff, _, c)| (staff, c)));
    }
}

fn sounds(e: &VoiceElement) -> bool {
    !matches!(e, VoiceElement::Rest(r) if r.is_spacer)
}

fn voice_staff(elements: &[VoiceElement]) -> u8 {
    elements
        .iter()
        .map(VoiceElement::staff)
        .find(|&s| s != 0)
        .unwrap_or(1)
}

/// The note that carries an element's beams (a chord's first).
fn lead(e: &VoiceElement) -> Option<&Note> {
    e.notes().first()
}

fn duration(e: &VoiceElement) -> &Duration {
    match e {
        VoiceElement::Note(n) => &n.duration,
        VoiceElement::Chord(c) => &c.duration,
        VoiceElement::Rest(r) => &r.duration,
    }
}

// ---------------------------------------------------------------------------
// Beams
// ---------------------------------------------------------------------------

/// Where beams may end in a bar, from LilyPond's defaults for its meter
/// (`scm/time-signature-settings.scm`): the beat structure, unless an
/// exception for the beam's shortest note says otherwise.
#[derive(Debug, Clone, PartialEq)]
struct Beaming {
    /// The bar's length.
    length: Frac,
    /// Ends of the beats (the last is the bar's end).
    beats: Vec<Frac>,
    /// (note length, ends): an exception applies to beams whose shortest
    /// note is as long as its length or shorter, unless a shorter
    /// exception is nearer.
    exceptions: Vec<(Frac, Vec<Frac>)>,
}

impl Beaming {
    fn of(ts: &TimeSignature) -> Self {
        let den = i64::from(ts.beat_type.max(1));
        let terms: Vec<i64> = ts
            .beats
            .split('+')
            .filter_map(|t| t.trim().parse().ok())
            .filter(|&t: &i64| t > 0)
            .collect();
        let num: i64 = terms.iter().sum::<i64>().max(1);
        let unit = Frac::new(1, den);
        let cumulative = |unit: Frac, counts: &[i64]| -> Vec<Frac> {
            counts
                .iter()
                .scan(Frac::from_integer(0), |at, &c| {
                    *at += unit * c;
                    Some(*at)
                })
                .collect()
        };
        let exception = |len: (i64, i64), counts: &[i64]| {
            let unit = Frac::new(len.0, len.1);
            (unit, cumulative(unit, counts))
        };
        let structure: Vec<i64> = match (num, den, terms.len()) {
            (_, _, n) if n > 1 => terms.clone(),
            (4, 8, _) => vec![2, 2],
            (5, 8, _) => vec![3, 2],
            (8, 8, _) => vec![3, 3, 2],
            (n, _, _) if n > 3 && n % 3 == 0 => vec![3; (n / 3) as usize],
            (n, _, _) => vec![1; n as usize],
        };
        let exceptions = match (num, den) {
            (2, 2) => vec![exception((1, 32), &[8; 4])],
            (2, 8) => vec![exception((1, 8), &[2])],
            (3, 2) => vec![exception((1, 32), &[8; 6])],
            (3, 4) => vec![exception((1, 8), &[6]), exception((1, 12), &[3; 3])],
            (3, 8) => vec![exception((1, 8), &[3])],
            (4, 2) => vec![exception((1, 16), &[4; 8])],
            (4, 4) if terms.len() <= 1 => {
                vec![exception((1, 8), &[4, 4]), exception((1, 12), &[3; 4])]
            }
            (6, 4) => vec![exception((1, 16), &[4; 6])],
            (9, 4) => vec![exception((1, 32), &[8; 9])],
            (12, 4) => vec![exception((1, 32), &[8; 12])],
            _ => Vec::new(),
        };
        Beaming {
            length: unit * num,
            beats: cumulative(unit, &structure),
            exceptions,
        }
    }

    /// Where a beam whose shortest note is `shortest` long, started at
    /// `from`, must end at the latest.
    fn end_after(&self, from: Frac, shortest: Frac) -> Frac {
        let ends = self
            .exceptions
            .iter()
            .filter(|(len, _)| *len >= shortest)
            .min_by_key(|(len, _)| *len)
            .map_or(&self.beats, |(_, ends)| ends);
        // Past the bar's length (an overfull bar), the pattern repeats.
        let bars = if self.length > Frac::from_integer(0) {
            (from / self.length).to_integer()
        } else {
            0
        };
        let offset = self.length * bars;
        ends.iter()
            .map(|e| *e + offset)
            .find(|e| *e > from)
            .unwrap_or(offset + self.length)
    }
}

/// Beam a voice: its source groups (secondary beams filled in) and, for the
/// notes left to the engraver, LilyPond's automatic beams. Returns every
/// beam group (element indices) for the stems.
fn beam_voice(elements: &mut [VoiceElement], rules: &Beaming, start: Frac) -> Vec<Vec<usize>> {
    let mut positions = Vec::with_capacity(elements.len());
    let mut at = start;
    for e in elements.iter() {
        positions.push(at);
        at += e.metric_duration();
    }
    let mut groups = source_groups(elements);
    for g in &groups {
        if g.iter()
            .all(|&i| lead(&elements[i]).is_none_or(|n| n.beams.iter().all(|b| b.number == 1)))
        {
            set_beams(elements, g, &positions);
        }
    }

    let open = |e: &VoiceElement| {
        lead(e).is_some_and(|n| n.beams.is_empty() && !n.no_auto_beam)
            && beam_level_for_duration(duration(e)) > 0
    };
    // Runs of notes LilyPond's auto-beamer would join: a rest, a gap, a
    // quarter or a note beamed or left unbeamed by the source ends one;
    // grace notes in between are passed over.
    let mut runs: Vec<Vec<usize>> = vec![Vec::new()];
    let mut grace_runs: Vec<Vec<usize>> = vec![Vec::new()];
    for (i, e) in elements.iter().enumerate() {
        let grace = lead(e).is_some_and(|n| n.is_grace);
        if grace {
            if open(e) {
                grace_runs.last_mut().unwrap().push(i);
            } else {
                grace_runs.push(Vec::new());
            }
            continue;
        }
        grace_runs.push(Vec::new());
        if open(e) {
            runs.last_mut().unwrap().push(i);
        } else {
            runs.push(Vec::new());
        }
    }
    let mut auto = Vec::new();
    for run in runs.iter().filter(|r| r.len() > 1) {
        // Split where the meter ends a beam for the run's shortest note so
        // far; a shorter note can end one earlier than the beam had reached.
        let mut first = 0;
        let mut k = 1;
        while k < run.len() {
            let shortest = run[first..=k]
                .iter()
                .map(|&i| duration(&elements[i]).actual_duration())
                .min()
                .unwrap_or_default();
            let end = rules.end_after(positions[run[first]], shortest);
            if positions[run[k]] >= end {
                let cut = (first..=k).find(|&j| positions[run[j]] >= end).unwrap_or(k);
                auto.push(run[first..cut].to_vec());
                first = cut;
                k = first + 1;
            } else {
                k += 1;
            }
        }
        auto.push(run[first..].to_vec());
    }
    // A grace group is beamed as one.
    auto.extend(grace_runs);
    for g in auto.into_iter().filter(|g| g.len() > 1) {
        set_beams(elements, &g, &positions);
        groups.push(g);
    }
    groups
}

/// The groups the source beamed: from a first-level begin to its end, the
/// elements between included (a beam given by its ends only, as the Music
/// tree gives it); grace notes and main notes beam apart.
fn source_groups(elements: &[VoiceElement]) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    let mut open: Option<(bool, Vec<usize>)> = None;
    for (i, e) in elements.iter().enumerate() {
        let grace = lead(e).is_some_and(|n| n.is_grace);
        let level1 = lead(e)
            .and_then(|n| n.beams.iter().find(|b| b.number == 1))
            .map(|b| b.beam_type.as_str());
        match (level1, &mut open) {
            (Some("begin"), _) => open = Some((grace, vec![i])),
            (Some("end"), Some((g, _))) if *g == grace => {
                let (_, mut group) = open.take().unwrap_or_default();
                group.push(i);
                groups.push(group);
            }
            (_, Some((g, group))) if *g == grace => group.push(i),
            _ => {}
        }
    }
    groups
}

/// Beam a group at every level its notes reach: the first across the
/// group, each further one across the notes that have it (not broken at
/// beats: LilyPond doesn't subdivide by default), a lone note a hook.
fn set_beams(elements: &mut [VoiceElement], group: &[usize], positions: &[Frac]) {
    let levels: Vec<u8> = group
        .iter()
        .map(|&i| beam_level_for_duration(duration(&elements[i])).max(1))
        .collect();
    let last = group.len() - 1;
    let mut beams: Vec<Vec<BeamEvent>> = vec![Vec::new(); group.len()];
    let event = |t: &str, number| BeamEvent {
        beam_type: t.to_string(),
        number,
    };
    for level in 1..=levels.iter().copied().max().unwrap_or(1) {
        let mut j = 0;
        while j <= last {
            if levels[j] < level {
                j += 1;
                continue;
            }
            let from = j;
            while j < last && levels[j + 1] >= level {
                j += 1;
            }
            if from == j {
                // A hook points to the note it belongs with: forward from
                // the group's first note or a note on its own beat, else
                // back.
                let own = duration(&elements[group[j]]).base * 2;
                let on_beat = (positions[group[j]] / own).is_integer();
                let forward = j == 0 || (j < last && on_beat);
                let hook = if forward {
                    "forward hook"
                } else {
                    "backward hook"
                };
                beams[j].push(event(hook, level));
            } else {
                beams[from].push(event("begin", level));
                for b in &mut beams[from + 1..j] {
                    b.push(event("continue", level));
                }
                beams[j].push(event("end", level));
            }
            j += 1;
        }
    }
    for (&i, b) in group.iter().zip(beams) {
        if let Some(n) = elements[i].notes_mut().first_mut() {
            n.beams = b;
        }
    }
}

// ---------------------------------------------------------------------------
// Stems
// ---------------------------------------------------------------------------

/// The middle staff line of a clef, in diatonic steps (C4 = 28).
fn middle_line(clef: &Clef) -> i32 {
    let (on_line, line) = match clef.sign {
        ClefSign::G => (32, clef.line), // G4
        ClefSign::F => (24, clef.line), // F3
        ClefSign::C => (28, clef.line), // C4
        _ => (34, 3),                   // percussion, TAB: B4
    };
    on_line + (3 - i32::from(line)) * 2 + i32::from(clef.octave_change) * 7
}

/// Staff positions of an element's note heads, from the middle line.
fn positions_on_staff(e: &VoiceElement, middle: i32) -> Vec<i32> {
    e.notes()
        .iter()
        .map(|n| n.pitch.octave * 7 + n.pitch.step.index() - middle)
        .collect()
}

/// The direction LilyPond gives stems over these note heads: away from the
/// head farthest from the middle line, down when they are as far.
fn default_direction(heads: &[i32]) -> Option<&'static str> {
    let top = *heads.iter().max()?;
    let bottom = *heads.iter().min()?;
    Some(if top.max(0) >= -bottom.min(0) {
        "down"
    } else {
        "up"
    })
}

fn has_stem(e: &VoiceElement) -> bool {
    !matches!(e, VoiceElement::Rest(_)) && duration(e).base < Frac::from_integer(1)
}

/// Give every stem the source left open its direction: a beam group one
/// direction for all, a voice sharing its staff its voice's, grace notes up,
/// others away from their outermost note.
fn stem_voice(
    elements: &mut [VoiceElement],
    groups: &[Vec<usize>],
    middles: &[i32],
    forced: Option<&'static str>,
) {
    let mut decided: HashMap<usize, &'static str> = HashMap::new();
    for g in groups {
        let given = g
            .iter()
            .map(|&i| stem_of(&elements[i]))
            .find(|s| !s.is_empty())
            .map(|s| if s == "up" { "up" } else { "down" });
        let heads: Vec<i32> = g
            .iter()
            .flat_map(|&i| positions_on_staff(&elements[i], middles[i]))
            .collect();
        let grace = g
            .iter()
            .all(|&i| lead(&elements[i]).is_some_and(|n| n.is_grace));
        let dir = given
            .or(forced)
            .or(grace.then_some("up"))
            .or_else(|| group_direction(elements, g, middles, &heads));
        if let Some(d) = dir {
            for &i in g {
                decided.insert(i, d);
            }
        }
    }
    for (i, e) in elements.iter_mut().enumerate() {
        if !has_stem(e) || e.notes().iter().all(|n| !n.stem_direction.is_empty()) {
            continue;
        }
        let grace = lead(e).is_some_and(|n| n.is_grace);
        let dir = decided
            .get(&i)
            .copied()
            .or(forced)
            .or(grace.then_some("up"));
        let Some(dir) = dir.or_else(|| default_direction(&positions_on_staff(e, middles[i])))
        else {
            continue;
        };
        for n in e.notes_mut() {
            if n.stem_direction.is_empty() {
                n.stem_direction = dir.to_string();
            }
        }
    }
}

/// A beam group's direction: from its outermost note heads; when they are
/// as far from the middle line, from the most notes' own directions; down
/// when those are as many.
fn group_direction(
    elements: &[VoiceElement],
    group: &[usize],
    middles: &[i32],
    heads: &[i32],
) -> Option<&'static str> {
    let top = (*heads.iter().max()?).max(0);
    let bottom = -(*heads.iter().min()?).min(0);
    if top != bottom {
        return Some(if top > bottom { "down" } else { "up" });
    }
    let ups = group
        .iter()
        .filter(|&&i| {
            default_direction(&positions_on_staff(&elements[i], middles[i])) == Some("up")
        })
        .count();
    Some(if ups * 2 > group.len() { "up" } else { "down" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::measure::{Measure, MeasureAttributes};
    use crate::ir::note::{Chord, Rest};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{PartGroup, ScoreChild};
    use crate::ir::voice::Voice;

    fn note(step: PitchStep, octave: i32, den: i64) -> VoiceElement {
        VoiceElement::Note(Box::new(Note::new(
            Pitch::new(step, octave),
            Duration::new(Frac::new(1, den)),
        )))
    }

    fn rest(den: i64) -> VoiceElement {
        VoiceElement::Rest(Rest::new(Duration::new(Frac::new(1, den))))
    }

    fn time(beats: &str, beat_type: u8) -> TimeSignature {
        TimeSignature {
            beats: beats.to_string(),
            beat_type,
            symbol: None,
        }
    }

    /// One bar of one voice (more with `voices`), engraved.
    fn engraved(ts: TimeSignature, voices: Vec<Vec<VoiceElement>>) -> Measure {
        let mut m = Measure::new(1);
        m.attributes = Some(MeasureAttributes {
            time: Some(ts),
            ..Default::default()
        });
        for (k, elements) in voices.into_iter().enumerate() {
            let mut v = Voice::new(k as u8 + 1);
            v.elements = elements;
            m.voices.push(v);
        }
        let mut part = Part::new("P1");
        part.measures.push(m);
        engrave_part(&mut part);
        part.measures.remove(0)
    }

    /// Level-1 beams as LilyPond brackets: `[`, `]`, `-` (continue), ` `.
    fn brackets(m: &Measure) -> String {
        m.voices[0]
            .elements
            .iter()
            .map(|e| {
                let b = lead(e).and_then(|n| n.beams.iter().find(|b| b.number == 1));
                match b.map(|b| b.beam_type.as_str()) {
                    Some("begin") => '[',
                    Some("end") => ']',
                    Some("continue") => '-',
                    _ => '.',
                }
            })
            .collect()
    }

    fn stems(m: &Measure, v: usize) -> Vec<String> {
        m.voices[v]
            .elements
            .iter()
            .map(|e| stem_of(e).to_string())
            .collect()
    }

    use PitchStep::*;

    #[test]
    fn meters_beam_as_lilypond_does() {
        let eighths = |n| (0..n).map(|_| note(C, 5, 8)).collect::<Vec<_>>();
        assert_eq!(
            brackets(&engraved(time("4", 4), vec![eighths(8)])),
            "[--][--]"
        );
        assert_eq!(
            brackets(&engraved(time("3", 4), vec![eighths(6)])),
            "[----]"
        );
        assert_eq!(brackets(&engraved(time("2", 4), vec![eighths(4)])), "[][]");
        assert_eq!(brackets(&engraved(time("3", 8), vec![eighths(3)])), "[-]");
        assert_eq!(
            brackets(&engraved(time("6", 8), vec![eighths(6)])),
            "[-][-]"
        );
        assert_eq!(brackets(&engraved(time("5", 8), vec![eighths(5)])), "[-][]");
        assert_eq!(
            brackets(&engraved(time("3+2", 8), vec![eighths(5)])),
            "[-][]"
        );
        assert_eq!(
            brackets(&engraved(time("2", 2), vec![eighths(8)])),
            "[--][--]"
        );
        // Sixteenths in 4/4 go by the beat.
        let sixteenths: Vec<_> = (0..8).map(|_| note(C, 5, 16)).collect();
        assert_eq!(
            brackets(&engraved(time("4", 4), vec![sixteenths])),
            "[--][--]"
        );
    }

    #[test]
    fn rests_quarters_and_shorter_notes_end_beams() {
        let m = engraved(
            time("6", 8),
            vec![vec![
                note(C, 5, 8),
                rest(8),
                note(D, 5, 8),
                note(E, 5, 8),
                note(F, 5, 8),
                note(G, 5, 8),
            ]],
        );
        assert_eq!(brackets(&m), "...[-]");
        let m = engraved(
            time("2", 2),
            vec![vec![
                note(C, 5, 8),
                note(D, 5, 4),
                note(E, 5, 8),
                note(C, 5, 2),
            ]],
        );
        assert_eq!(brackets(&m), "....");
        // A sixteenth ends the eighths' half-bar beam at the beat.
        let m = engraved(
            time("4", 4),
            vec![vec![
                note(C, 5, 8),
                note(D, 5, 8),
                note(E, 5, 8),
                note(F, 5, 16),
                note(G, 5, 16),
                note(C, 5, 2),
            ]],
        );
        assert_eq!(brackets(&m), "[][-].");
        // An off-beat start beams with the rest of its beat group.
        let m = engraved(
            time("4", 4),
            vec![vec![
                rest(8),
                note(C, 5, 8),
                note(D, 5, 8),
                note(E, 5, 8),
                note(C, 5, 2),
            ]],
        );
        assert_eq!(brackets(&m), ".[-].");
    }

    #[test]
    fn a_pickup_beams_from_where_it_sits_in_the_bar() {
        // A 2/4 pickup of three eighths starts on the second half of beat
        // 1: the first eighth is alone, the other two share beat 2.
        let mut m = Measure::new(0);
        m.implicit = true;
        m.attributes = Some(MeasureAttributes {
            time: Some(time("2", 4)),
            ..Default::default()
        });
        let mut v = Voice::new(1);
        v.elements = vec![note(C, 5, 8), note(D, 5, 8), note(E, 5, 8)];
        m.voices.push(v);
        let mut part = Part::new("P1");
        part.measures.push(m);
        engrave_part(&mut part);
        assert_eq!(brackets(&part.measures[0]), ".[]");
    }

    #[test]
    fn secondary_beams_and_hooks() {
        let dotted = |step, den| {
            VoiceElement::Note(Box::new(Note::new(
                Pitch::new(step, 5),
                Duration::dotted(Frac::new(1, den), 1),
            )))
        };
        let m = engraved(
            time("2", 4),
            vec![vec![
                dotted(C, 8),
                note(D, 5, 16),
                note(E, 5, 16),
                dotted(F, 8),
            ]],
        );
        let level2: Vec<String> = m.voices[0]
            .elements
            .iter()
            .map(|e| {
                lead(e)
                    .and_then(|n| n.beams.iter().find(|b| b.number == 2))
                    .map_or(String::new(), |b| b.beam_type.clone())
            })
            .collect();
        assert_eq!(level2, ["", "backward hook", "forward hook", ""]);
        // Manual first-level beams get their second level.
        let mut sixteenths: Vec<_> = (0..4).map(|_| note(C, 5, 16)).collect();
        for (e, t) in sixteenths
            .iter_mut()
            .zip(["begin", "continue", "continue", "end"])
        {
            e.notes_mut()[0].beams = vec![BeamEvent {
                beam_type: t.to_string(),
                number: 1,
            }];
        }
        let m = engraved(time("4", 4), vec![sixteenths]);
        assert!(m.voices[0]
            .elements
            .iter()
            .all(|e| lead(e).is_some_and(|n| n.beams.len() == 2)));
    }

    #[test]
    fn stems_follow_staff_positions_groups_and_voices() {
        // B4 is the treble middle line: down. A4 below it: up.
        let m = engraved(
            time("4", 4),
            vec![vec![note(B, 4, 4), note(A, 4, 4), note(C, 4, 2)]],
        );
        assert_eq!(stems(&m, 0), ["down", "up", "up"]);
        // A beam group takes one direction from its farthest note: G4 is 2
        // below, D5 2 above... E5 3 above wins.
        let m = engraved(
            time("2", 4),
            vec![vec![note(G, 4, 8), note(E, 5, 8), note(C, 5, 4)]],
        );
        assert_eq!(stems(&m, 0), ["down", "down", "down"]);
        // A chord: its outermost notes (C4 is 6 below, G5 5 above).
        let quarter = || Duration::new(Frac::new(1, 4));
        let chord = VoiceElement::Chord(Chord::new(
            quarter(),
            vec![
                Note::new(Pitch::new(C, 4), quarter()),
                Note::new(Pitch::new(G, 5), quarter()),
            ],
        ));
        let m = engraved(time("1", 4), vec![vec![chord]]);
        assert_eq!(stems(&m, 0), ["up"]);
        // Two voices on a staff: up and down, whatever their notes.
        let m = engraved(time("1", 4), vec![vec![note(C, 4, 4)], vec![note(G, 5, 4)]]);
        assert_eq!(
            (stems(&m, 0), stems(&m, 1)),
            (vec!["up".into()], vec!["down".into()])
        );
        // A whole note has no stem.
        let m = engraved(time("4", 4), vec![vec![note(C, 5, 1)]]);
        assert_eq!(stems(&m, 0), [""]);
    }

    #[test]
    fn stems_follow_each_staffs_clef_in_grouped_parts() {
        // A piano inside a group; E3 on staff 2 (bass clef, middle line D3)
        // points down, the same E3 on staff 1 (treble) up.
        let e3 = |staff| {
            let mut n = Note::new(Pitch::new(PitchStep::E, 3), Duration::new(Frac::new(1, 4)));
            n.staff = staff;
            VoiceElement::Note(Box::new(n))
        };
        let mut m = Measure::new(1);
        let mut attrs = MeasureAttributes::default();
        attrs.clefs.insert(1, Clef::default());
        attrs.clefs.insert(
            2,
            Clef {
                sign: ClefSign::F,
                line: 4,
                ..Clef::default()
            },
        );
        m.attributes = Some(attrs);
        for (n, staff) in [(1, 1), (2, 2)] {
            let mut v = Voice::new(n);
            v.elements.push(e3(staff));
            m.voices.push(v);
        }
        let mut part = Part::new("P1");
        part.staves = 2;
        part.measures.push(m);
        let mut group = PartGroup::new("StaffGroup");
        group.children.push(ScoreChild::Part(part));
        let mut score = Score::new();
        score.children.push(ScoreChild::PartGroup(group));

        engrave(&mut score);
        let stems: Vec<String> = score.parts()[0].measures[0]
            .voices
            .iter()
            .map(|v| stem_of(&v.elements[0]).to_string())
            .collect();
        assert_eq!(stems, ["up", "down"]);
    }
}
