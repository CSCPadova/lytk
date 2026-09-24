//! Positioned, measure-free content of a part under construction, and the one
//! bar-splitter that turns it into measures.
//!
//! The walk never builds measures. Every note, rest and chord is placed at its
//! absolute onset in a voice *lane*; every attribute, direction, barline,
//! harmony and figure is an event at an absolute position. Simultaneous music
//! (`<< … \\ … >>`, `<< {…} {…} >>`, parallel variables, the staves of a
//! PianoStaff) overlays by position, so there is nothing to merge by index.
//!
//! Bars are made once, at score assembly, from a score-wide [`Grid`] — the
//! LilyPond model, where `Timing` lives in the Score context: bar lines fall on
//! the meter grid (anchored at the start, at `\partial` and at every `\time`),
//! an explicit barline (`\bar`, repeat and volta edges) adds a boundary without
//! moving the grid, and a `\cadenzaOn … \cadenzaOff` span is one free bar. Bar
//! checks (`|`) are only checks, as in LilyPond.

use std::collections::BTreeMap;

use crate::ir::direction::{Barline, BarlineType, Direction};
use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::{FiguredBass, Harmony};
use crate::ir::measure::{Clef, KeySignature, Measure, MeasureAttributes, TimeSignature};
use crate::ir::note::{Rest, VoiceElement};
use crate::ir::voice::Voice;

/// Something that happens at a position but is not a voice element.
#[derive(Clone, Debug)]
pub(super) enum Event {
    Time(TimeSignature),
    /// `\set Score.measureLength`: changes the bar length without a signature.
    MeasureLength(Frac),
    Key(KeySignature),
    /// Clef for a staff (1 until the part is folded into a PianoStaff).
    Clef(u8, Clef),
    Direction(Box<Direction>),
    /// A barline at the start of the bar beginning here.
    LeftBarline(Barline),
    /// A barline at the end of the bar ending here.
    RightBarline(Barline),
    Harmony(Harmony),
    FiguredBass(FiguredBass),
    CadenzaOn,
    CadenzaOff,
    /// `\partial d`: the bar in progress here has only `d` left.
    Partial(Frac),
}

/// Positioned content of one part (or of a variable, relative to 0).
#[derive(Clone, Debug, Default)]
pub(super) struct Timeline {
    /// Voice lanes (lane number = IR voice number), each onset-ordered.
    pub(super) lanes: BTreeMap<u8, Vec<(Frac, VoiceElement)>>,
    pub(super) events: Vec<(Frac, Event)>,
}

fn zero() -> Frac {
    Frac::from_integer(0)
}

impl Event {
    pub(super) fn direction(d: Direction) -> Event {
        Event::Direction(Box::new(d))
    }
}

impl Timeline {
    pub(super) fn add(&mut self, pos: Frac, ev: Event) {
        self.events.push((pos, ev));
    }

    /// Position after the last element or event.
    pub(super) fn end(&self) -> Frac {
        let lanes = self
            .lanes
            .values()
            .flat_map(|l| l.iter().map(|(on, e)| *on + e.metric_duration()));
        let events = self.events.iter().map(|(p, _)| *p);
        lanes.chain(events).max().unwrap_or_else(zero)
    }

    pub(super) fn has_lane_content(&self) -> bool {
        self.lanes.values().any(|l| !l.is_empty())
    }

    /// Place a contiguous run of elements starting at `start` in `lane` — or,
    /// when that lane already holds music overlapping the run (a second
    /// simultaneous voice), in the lowest-numbered lane above it that is free
    /// over the run. Returns the lane used.
    pub(super) fn place_run(&mut self, lane: u8, start: Frac, elems: Vec<VoiceElement>) -> u8 {
        if elems.is_empty() {
            return lane;
        }
        let end = start
            + elems
                .iter()
                .map(VoiceElement::metric_duration)
                .sum::<Frac>();
        let mut lane = lane.max(1);
        while self
            .lanes
            .get(&lane)
            .is_some_and(|l| overlaps(l, start, end))
        {
            lane = lane.saturating_add(1);
        }
        let slot = self.lanes.entry(lane).or_default();
        let mut pos = start;
        for mut e in elems {
            set_voice_number(&mut e, lane);
            let d = e.metric_duration();
            // After every element already at or before `pos`, so equal onsets
            // (grace notes before their main note) keep walk order.
            let at = slot.partition_point(|(on, _)| *on <= pos);
            slot.insert(at, (pos, e));
            pos += d;
        }
        lane
    }

    /// Splice a variable's timeline in at `at`. Its `main_lane` (the lane the
    /// variable's music was written in) lands in `lane`; other lanes keep their
    /// numbers, moving up past any lane their music would overlap.
    pub(super) fn splice(&mut self, frag: &Timeline, at: Frac, main_lane: u8, lane: u8) {
        for (&l, elems) in &frag.lanes {
            let target = if l == main_lane { lane } else { l };
            for (start, run) in contiguous_runs(elems) {
                self.place_run(target, at + start, run);
            }
        }
        for (p, ev) in &frag.events {
            self.events.push((at + *p, ev.clone()));
        }
    }

    /// Shift every position by `by` (a variable spliced in at `by`).
    pub(super) fn shifted(mut self, by: Frac) -> Timeline {
        if by != zero() {
            for lane in self.lanes.values_mut() {
                for (on, _) in lane.iter_mut() {
                    *on += by;
                }
            }
            for (p, _) in &mut self.events {
                *p += by;
            }
        }
        self
    }

    /// Stamp a staff number on every element, clef and direction (a staff
    /// being folded into a multi-staff part), and move lanes up by `offset`
    /// so they stay disjoint from the staves before it.
    pub(super) fn into_staff(self, staff: u8, offset: u8) -> Timeline {
        let lanes = self
            .lanes
            .into_iter()
            .map(|(l, elems)| {
                let lane = l.saturating_add(offset);
                let elems = elems
                    .into_iter()
                    .map(|(on, mut e)| {
                        set_staff(&mut e, staff);
                        set_voice_number(&mut e, lane);
                        (on, e)
                    })
                    .collect();
                (lane, elems)
            })
            .collect();
        let events = self
            .events
            .into_iter()
            .map(|(p, ev)| {
                let ev = match ev {
                    Event::Clef(_, c) => Event::Clef(staff, c),
                    Event::Direction(mut d) => {
                        if d.staff == 0 {
                            d.staff = staff;
                        }
                        Event::Direction(d)
                    }
                    other => other,
                };
                (p, ev)
            })
            .collect();
        Timeline { lanes, events }
    }

    /// Absorb another timeline's lanes (numbers unchanged) and events.
    pub(super) fn absorb(&mut self, other: Timeline) {
        for (l, elems) in other.lanes {
            let slot = self.lanes.entry(l).or_default();
            slot.extend(elems);
            slot.sort_by_key(|(on, _)| *on);
        }
        self.events.extend(other.events);
    }

    /// Lanes holding only spacers (`s`, `\skip`, or `R` next to real music) are
    /// not voices: they carry dynamics, hairpins and timing. Turn what they
    /// carry into directions at the spacer's position and drop the lane.
    pub(super) fn fold_spacer_lanes(&mut self) {
        let has_music = self
            .lanes
            .values()
            .any(|l| l.iter().any(|(_, e)| is_music(e)));
        let spacer_lanes: Vec<u8> = self
            .lanes
            .iter()
            .filter(|(_, l)| {
                !l.is_empty()
                    && l.iter().all(|(_, e)| match e {
                        VoiceElement::Rest(r) => r.is_spacer || (has_music && r.is_measure_rest),
                        _ => false,
                    })
            })
            .map(|(&n, _)| n)
            .collect();
        for n in spacer_lanes {
            for (on, e) in self.lanes.remove(&n).unwrap_or_default() {
                if let VoiceElement::Rest(r) = e {
                    for dynamic in r.dynamics {
                        self.events.push((
                            on,
                            Event::direction(Direction {
                                dynamic: Some(dynamic),
                                ..Default::default()
                            }),
                        ));
                    }
                    for wedge in r.wedges {
                        self.events.push((
                            on,
                            Event::direction(Direction {
                                wedge: Some(wedge),
                                ..Default::default()
                            }),
                        ));
                    }
                }
            }
        }
    }
}

/// A note, chord or real (visible) rest — anything but a spacer.
fn is_music(e: &VoiceElement) -> bool {
    !matches!(e, VoiceElement::Rest(r) if r.is_spacer || r.is_measure_rest)
}

/// Does `[start, end)` overlap an element of `lane`? A zero-length run (grace
/// notes only) collides only with an element sounding strictly across it.
///
/// A lane never holds overlapping elements and is onset-ordered, so only the
/// elements starting inside the run, and the last sounding element before it,
/// can collide: a binary search and a short local scan, not a lane-long one.
fn overlaps(lane: &[(Frac, VoiceElement)], start: Frac, end: Frac) -> bool {
    let first_after = lane.partition_point(|(on, _)| *on < end.max(start));
    let mut i = first_after;
    while i > 0 {
        i -= 1;
        let (on, e) = &lane[i];
        let dur = e.metric_duration();
        if *on < start {
            // The last element starting before the run: does it sound into it?
            // (Grace notes take no time, so look past them.)
            if dur == zero() {
                continue;
            }
            return *on + dur > start;
        }
        // Starts inside the run.
        if end > start && dur > zero() && *on < end {
            return true;
        }
    }
    false
}

/// Split a lane into maximal runs of back-to-back elements.
fn contiguous_runs(elems: &[(Frac, VoiceElement)]) -> Vec<(Frac, Vec<VoiceElement>)> {
    let mut runs: Vec<(Frac, Vec<VoiceElement>)> = Vec::new();
    let mut cursor: Option<Frac> = None;
    for (on, e) in elems {
        if cursor != Some(*on) {
            runs.push((*on, Vec::new()));
        }
        cursor = Some(*on + e.metric_duration());
        runs.last_mut().unwrap().1.push(e.clone());
    }
    runs
}

fn set_voice_number(e: &mut VoiceElement, n: u8) {
    match e {
        VoiceElement::Note(note) => note.voice = n,
        VoiceElement::Rest(r) => r.voice = n,
        VoiceElement::Chord(c) => {
            c.voice = n;
            for note in &mut c.notes {
                note.voice = n;
            }
        }
    }
}

fn set_staff(e: &mut VoiceElement, staff: u8) {
    match e {
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

fn element_staff(e: &VoiceElement) -> u8 {
    match e {
        VoiceElement::Note(n) => n.staff,
        VoiceElement::Rest(r) => r.staff,
        VoiceElement::Chord(c) => c.staff,
    }
}

// ---------------------------------------------------------------------------
// The grid: where the bars are, for every part of a score
// ---------------------------------------------------------------------------

/// Score-wide bar layout.
pub(super) struct Grid {
    /// `[start, end)` of each bar, contiguous from 0.
    pub(super) bars: Vec<(Frac, Frac)>,
    /// Bars inside a cadenza.
    senza: Vec<bool>,
    /// Meter in force from each `\time` (for parts that don't declare it).
    meters: Vec<(Frac, TimeSignature)>,
    /// The first bar is a pickup (`\partial` at the start).
    pickup: bool,
}

/// Grid control point, in the order they are applied at one position.
enum Ctl {
    CadenzaOff,
    Time(Frac, Option<TimeSignature>),
    Partial(Frac),
    CadenzaOn,
    Barline,
}

impl Ctl {
    fn rank(&self) -> u8 {
        match self {
            Ctl::CadenzaOff => 0,
            Ctl::Time(..) => 1,
            Ctl::Partial(_) => 2,
            Ctl::CadenzaOn => 3,
            Ctl::Barline => 4,
        }
    }
}

impl Grid {
    pub(super) fn build<'a>(timelines: impl IntoIterator<Item = &'a Timeline>) -> Grid {
        let mut end = zero();
        // The last point where anything starts: bar lines past it would only
        // cut an over-long last note into an empty bar.
        let mut last_start = zero();
        let mut ctls: Vec<(Frac, Ctl)> = Vec::new();
        for tl in timelines {
            end = end.max(tl.end());
            let onsets = tl.lanes.values().flat_map(|l| l.iter().map(|(on, _)| *on));
            let openers = tl.events.iter().filter_map(|(p, ev)| match ev {
                Event::RightBarline(_) | Event::CadenzaOff | Event::Partial(_) => None,
                Event::Direction(d) if d.layout_break.is_some() => None,
                _ => Some(*p),
            });
            last_start = onsets.chain(openers).fold(last_start, Frac::max);
            for (p, ev) in &tl.events {
                let ctl = match ev {
                    Event::Time(ts) => Ctl::Time(ts.beats_fraction(), Some(ts.clone())),
                    Event::MeasureLength(len) => Ctl::Time(*len, None),
                    Event::Partial(d) => Ctl::Partial(*d),
                    Event::CadenzaOn => Ctl::CadenzaOn,
                    Event::CadenzaOff => Ctl::CadenzaOff,
                    Event::LeftBarline(_) | Event::RightBarline(_) => Ctl::Barline,
                    _ => continue,
                };
                ctls.push((*p, ctl));
            }
        }
        ctls.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.rank().cmp(&b.1.rank())));

        let mut cuts: Vec<Frac> = vec![zero()];
        let mut free_spans: Vec<(Frac, Frac)> = Vec::new();
        let mut meters: Vec<(Frac, TimeSignature)> = Vec::new();
        let mut bar_len = Frac::from_integer(1);
        let mut anchor = zero(); // a bar line falls here and every bar_len after
        let mut free_from: Option<Frac> = None;
        let mut pickup = false;
        // Meter-grid bar lines in (from, to).
        let grid_lines =
            |cuts: &mut Vec<Frac>, anchor: Frac, bar_len: Frac, from: Frac, to: Frac| {
                if bar_len <= zero() {
                    return;
                }
                let mut b = anchor;
                if b <= from {
                    let n = ((from - anchor) / bar_len).to_integer();
                    b = anchor + bar_len * Frac::from_integer(n);
                    while b <= from {
                        b += bar_len;
                    }
                }
                while b < to {
                    cuts.push(b);
                    b += bar_len;
                }
            };

        let mut at = zero();
        for (p, ctl) in ctls {
            if free_from.is_none() {
                grid_lines(&mut cuts, anchor, bar_len, at, p);
            }
            at = p;
            match ctl {
                Ctl::Time(len, ts) => {
                    // A bar line where the meter changes (LilyPond warns about a
                    // mid-bar `\time`; the new meter counts from here).
                    cuts.push(p);
                    bar_len = len;
                    anchor = p;
                    if let Some(ts) = ts {
                        if meters.last().is_none_or(|(q, t)| *q != p || *t != ts) {
                            meters.retain(|(q, _)| *q != p);
                            meters.push((p, ts));
                        }
                    }
                }
                Ctl::Partial(d) => {
                    if p == zero() && d < bar_len {
                        pickup = true;
                    }
                    anchor = p + d;
                    cuts.push(p + d);
                }
                Ctl::CadenzaOn => {
                    if free_from.is_none() {
                        cuts.push(p);
                        free_from = Some(p);
                    }
                }
                Ctl::CadenzaOff => {
                    if let Some(from) = free_from.take() {
                        free_spans.push((from, p));
                        cuts.push(p);
                        anchor = p;
                    }
                }
                Ctl::Barline => cuts.push(p),
            }
        }
        if let Some(from) = free_from {
            free_spans.push((from, end.max(from)));
        } else {
            grid_lines(&mut cuts, anchor, bar_len, at, end);
        }

        cuts.retain(|&c| c <= end && (c == zero() || c <= last_start));
        cuts.sort();
        cuts.dedup();
        let mut bars: Vec<(Frac, Frac)> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
        let last = *cuts.last().unwrap();
        if end > last || bars.is_empty() {
            bars.push((last, end.max(last)));
        }
        let senza = bars
            .iter()
            .map(|(s, _)| free_spans.iter().any(|(a, b)| s >= a && s < b))
            .collect();
        Grid {
            bars,
            senza,
            meters,
            pickup,
        }
    }

    /// Index of the bar containing `pos` (the one starting at `pos` on a boundary).
    pub(super) fn bar_at(&self, pos: Frac) -> usize {
        self.bars
            .partition_point(|(s, _)| *s <= pos)
            .saturating_sub(1)
    }

    /// Index of the bar ending at `pos` (or containing it, off a boundary).
    fn bar_ending_at(&self, pos: Frac) -> usize {
        self.bars
            .partition_point(|(_, e)| *e < pos)
            .min(self.bars.len() - 1)
    }
}

// ---------------------------------------------------------------------------
// The bar-splitter
// ---------------------------------------------------------------------------

/// Offset from a bar start in `divisions` per quarter note.
fn divisions(within: Frac, per_quarter: i64) -> i32 {
    (within * Frac::from_integer(4 * per_quarter)).to_integer() as i32
}

/// Combine two barlines on the same edge of the same bar: the one with a
/// non-regular style wins the style; repeat and ending fields are united.
fn combine(into: &mut Barline, b: Barline) {
    if into.style == BarlineType::Regular {
        into.style = b.style;
    }
    into.repeat_direction = into.repeat_direction.or(b.repeat_direction);
    into.repeat_times = into.repeat_times.or(b.repeat_times);
    if into.ending_type.is_none() {
        into.ending_type = b.ending_type;
        into.ending_number = b.ending_number;
    }
}

fn put_barline(slot: &mut Option<Barline>, b: Barline) {
    match slot {
        Some(existing) => combine(existing, b),
        None => *slot = Some(b),
    }
}

fn attrs(m: &mut Measure) -> &mut MeasureAttributes {
    m.attributes.get_or_insert_with(MeasureAttributes::default)
}

/// Build a part's measures on the score's grid.
pub(super) fn split(
    tl: Timeline,
    grid: &Grid,
    harmony_divs: i64,
    figure_divs: i64,
) -> Vec<Measure> {
    if grid.bars.is_empty() || (!tl.has_lane_content() && tl.events.is_empty()) {
        return Vec::new();
    }
    let mut measures: Vec<Measure> = grid
        .bars
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut m = Measure::new(i as u32 + 1);
            m.senza_misura = grid.senza[i];
            m.implicit = i == 0 && grid.pickup;
            m
        })
        .collect();

    // Every part carries the score's meter changes.
    for (p, ts) in &grid.meters {
        let i = grid.bar_at(*p);
        attrs(&mut measures[i]).time = Some(ts.clone());
    }

    for (pos, ev) in tl.events {
        let i = grid.bar_at(pos);
        let within = pos - grid.bars[i].0;
        match ev {
            Event::Time(ts) => attrs(&mut measures[i]).time = Some(ts),
            Event::Key(k) => attrs(&mut measures[i]).key = Some(k),
            Event::Clef(staff, c) => {
                attrs(&mut measures[i]).clefs.insert(staff, c);
            }
            Event::Direction(mut d) => {
                // A line/page break is a break before the bar it lands in
                // (MusicXML `<print new-system>`), like every other direction.
                d.offset_frac = within;
                measures[i].directions.push(*d);
            }
            Event::LeftBarline(b) => put_barline(&mut measures[i].left_barline, b),
            Event::RightBarline(b) => {
                let i = grid.bar_ending_at(pos);
                put_barline(&mut measures[i].right_barline, b);
            }
            Event::Harmony(mut h) => {
                h.offset = divisions(within, harmony_divs);
                measures[i].harmonies.push(h);
            }
            Event::FiguredBass(mut fb) => {
                fb.offset = divisions(within, figure_divs);
                measures[i].figured_bass.push(fb);
            }
            Event::MeasureLength(_) | Event::CadenzaOn | Event::CadenzaOff | Event::Partial(_) => {}
        }
    }

    for (lane, elems) in tl.lanes {
        // Onsets are sorted, so the bar only moves forward: walk the bars with
        // a pointer and fill one bar's voice at a time, gaps as spacers.
        let mut bar = 0usize;
        let mut current: Option<(usize, Vec<VoiceElement>)> = None;
        let mut cursor = zero(); // end of the lane's previous element
        for (on, e) in elems {
            while bar + 1 < grid.bars.len() && grid.bars[bar + 1].0 <= on {
                bar += 1;
            }
            let mut i = bar;
            // An after-grace at a bar line trails the note before it.
            if matches!(&e, VoiceElement::Note(n) if n.after_grace)
                && on == grid.bars[i].0
                && current.as_ref().is_some_and(|(j, _)| *j + 1 == i)
            {
                i -= 1;
            }
            if current.as_ref().map(|(j, _)| *j) != Some(i) {
                if let Some((j, elements)) = current.take() {
                    measures[j].voices.push(Voice {
                        number: lane,
                        elements,
                    });
                }
                cursor = cursor.max(grid.bars[i].0);
                current = Some((i, Vec::new()));
            }
            let slot = &mut current.as_mut().unwrap().1;
            if on > cursor {
                let mut gap = Rest::new(Duration::new(on - cursor));
                gap.is_spacer = true;
                gap.voice = lane;
                gap.staff = element_staff(&e);
                slot.push(VoiceElement::Rest(gap));
            }
            cursor = cursor.max(on + e.metric_duration());
            slot.push(e);
        }
        if let Some((j, elements)) = current {
            measures[j].voices.push(Voice {
                number: lane,
                elements,
            });
        }
    }
    for m in &mut measures {
        m.voices.sort_by_key(|v| v.number);
    }
    measures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::note::Note;
    use crate::ir::pitch::{Pitch, PitchStep};

    fn note(q: i64) -> VoiceElement {
        VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::new(Frac::new(q, 4)),
        )))
    }

    fn ts(b: u8, t: u8) -> TimeSignature {
        TimeSignature {
            beats: b.to_string(),
            beat_type: t,
            symbol: None,
        }
    }

    #[test]
    fn overlapping_run_moves_to_the_next_free_lane() {
        let mut tl = Timeline::default();
        assert_eq!(tl.place_run(1, zero(), vec![note(2), note(2)]), 1);
        assert_eq!(tl.place_run(1, zero(), vec![note(4)]), 2);
        // After both, lane 1 is free again.
        assert_eq!(tl.place_run(1, Frac::from_integer(1), vec![note(4)]), 1);
    }

    #[test]
    fn grid_follows_meter_partial_and_barlines() {
        let mut tl = Timeline::default();
        tl.add(zero(), Event::Time(ts(3, 4)));
        tl.add(zero(), Event::Partial(Frac::new(1, 4)));
        tl.place_run(1, zero(), (0..13).map(|_| note(1)).collect()); // 13 quarters
        tl.add(Frac::new(5, 4), Event::RightBarline(Barline::default())); // mid-bar
        let g = Grid::build([&tl]);
        let starts: Vec<Frac> = g.bars.iter().map(|b| b.0).collect();
        let q = |n| Frac::new(n, 4);
        // pickup 1/4, bar 1/4..4/4, split at 5/4, then the 3/4 grid from 1/4.
        assert_eq!(starts, vec![q(0), q(1), q(4), q(5), q(7), q(10)]);
        assert!(g.pickup);
    }

    #[test]
    fn over_long_last_note_makes_no_empty_bar() {
        let mut tl = Timeline::default();
        tl.add(zero(), Event::Time(ts(3, 8)));
        tl.place_run(1, zero(), vec![note(1), note(2)]); // 1/4 + 1/2 in 3/8
        tl.add(Frac::new(3, 4), Event::RightBarline(Barline::default()));
        let g = Grid::build([&tl]);
        assert_eq!(g.bars, vec![(zero(), Frac::new(3, 4))]);
    }

    #[test]
    fn cadenza_is_one_free_bar() {
        let mut tl = Timeline::default();
        tl.place_run(1, zero(), (0..12).map(|_| note(1)).collect());
        tl.add(Frac::from_integer(1), Event::CadenzaOn);
        tl.add(Frac::from_integer(2), Event::CadenzaOff);
        let g = Grid::build([&tl]);
        assert_eq!(g.bars.len(), 3);
        assert_eq!(g.senza, vec![false, true, false]);
    }

    #[test]
    fn split_fills_gaps_and_places_events() {
        let mut tl = Timeline::default();
        tl.place_run(1, zero(), vec![note(4), note(4)]);
        tl.place_run(2, Frac::new(1, 2), vec![note(2)]); // enters mid-bar
        let g = Grid::build([&tl]);
        let ms = split(tl, &g, 4, 4);
        assert_eq!(ms.len(), 2);
        let v2 = &ms[0].voices[1];
        assert!(matches!(&v2.elements[0], VoiceElement::Rest(r) if r.is_spacer));
        assert_eq!(v2.elements.len(), 2);
    }
}
