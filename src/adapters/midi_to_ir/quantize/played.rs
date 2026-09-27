//! Placing a played staff's onsets: a Viterbi search over candidate points.
//!
//! Written from the description of MuseScore's MIDI import (an onset goes
//! where it is near and where a note of its value is expected in the bar),
//! reduced to one search over the whole staff:
//!
//! - Notes starting within a 64th are one cluster, placed together.
//! - A cluster's expected value is the meter step its length is closest to
//!   from below, at 5/4 of it (played notes run short): what sounds of it in
//!   its bar, stretched to the next cluster but no more than doubled (a long
//!   rest after it says nothing), and no longer than the time since the
//!   cluster before (a 16th or more: a note soon after another may start
//!   anywhere). A dotted length takes the step below.
//! - Its candidate points are the plain grid (32nds, or as asked) and the
//!   triplet grid of the beats within a beat of it. A point costs its
//!   distance, plus a quarter of the expected value for each metric level it
//!   is weaker than expected: a quarter played 40 ticks late stays on the
//!   beat, a note that sounds like a 16th may go to a 16th.
//! - A beat is plain or triplet throughout, and a triplet beat needs two
//!   onsets off the 16th grid (a quarter–eighth triplet is no evidence: it is
//!   also a dotted rhythm). (ponytail: triplets are a beat long; quarter-note
//!   triplets, two beats long, would need the evidence over two beats.) Changing between them costs a little, so one
//!   sloppy beat inside triplets stays triplets. Two clusters may take one
//!   point at a cost (a rolled chord).

use std::collections::BTreeMap;

use super::{Quantizer, PLAIN};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Plain,
    Triplet,
}

/// A candidate point for a cluster: where, which grid it is on, how many
/// distinct onsets off the 16th grid its beat has so far (up to 2), and the
/// best way there.
#[derive(Clone, Copy, Debug)]
struct State {
    p: i64,
    family: Family,
    evidence: u8,
    cost: i64,
    back: usize,
}

impl Quantizer {
    /// The bar change holding `p`.
    fn change_at(&self, p: i64) -> usize {
        self.bars.partition_point(|b| b.0 <= p).max(1) - 1
    }

    /// The meter's steps in units, bar first: the bar, the half bar of 4
    /// beats or the dotted beat of a compound meter, the beat, then halves
    /// down to the finest grid.
    fn steps(&self, k: usize) -> Vec<i64> {
        let (num, den) = self.meters.get(k).copied().unwrap_or((4, 4));
        let beat = self.whole / den;
        let mut out = vec![beat * num];
        if num % 3 == 0 && num >= 6 {
            if num == 12 {
                out.push(beat * 6);
            }
            out.push(beat * 3);
        } else if num == 4 {
            out.push(beat * 2);
        }
        out.push(beat);
        let finest = self.quarter() / self.finest;
        let mut step = beat;
        while step % 2 == 0 && step / 2 >= finest {
            step /= 2;
            out.push(step);
        }
        out.dedup();
        out
    }

    /// Where the beats of the bar holding `p` count from: its start, or for a
    /// pickup the start of the full bar it ends.
    fn origin(&self, p: i64, steps: &[i64]) -> i64 {
        let start = self.start_of(p);
        let k = self.change_at(p);
        match self.bars.get(k) {
            Some(&(anchor, len)) if k == 0 && anchor == start && len < steps[0] => {
                start + len - steps[0]
            }
            _ => start,
        }
    }

    /// How weak a point is: 0 on the bar line, one more for each finer
    /// step it first lies on; triplet points one or two below the beat.
    fn depth(&self, p: i64, steps: &[i64]) -> i64 {
        let x = p - self.origin(p, steps);
        if let Some(i) = steps.iter().position(|&s| x.rem_euclid(s) == 0) {
            return i as i64;
        }
        let q = self.quarter();
        let beat = steps.iter().position(|&s| s == q).unwrap_or(steps.len()) as i64;
        if x.rem_euclid(q / 3) == 0 {
            beat + 1
        } else if x.rem_euclid(q / 6) == 0 {
            beat + 2
        } else {
            steps.len() as i64 + 1
        }
    }

    /// The end of the bar holding `p` (a meter change ends it early).
    fn bar_end(&self, p: i64) -> i64 {
        let k = self.change_at(p);
        let (_, len) = self.bars.get(k).copied().unwrap_or((0, self.whole));
        let end = self.start_of(p) + len.max(1);
        self.bars.get(k + 1).map_or(end, |next| end.min(next.0))
    }

    /// The step (and its depth) a note sounding `len`, `dur` of it counting
    /// (after the note before), is expected to start on.
    fn expected(&self, len: i64, dur: i64, steps: &[i64]) -> (i64, i64) {
        let finest = self.quarter() / self.finest;
        let mut i = steps
            .iter()
            .position(|&s| 4 * s <= 5 * dur)
            .unwrap_or(steps.len() - 1);
        // A dotted length: the step below.
        let dotted = |d: i64| 5 * d >= 7 * steps[i] && 5 * d <= 8 * steps[i];
        if dotted(dur) && dotted(len) && i + 1 < steps.len() {
            i += 1;
        }
        while steps[i] < finest && i > 0 {
            i -= 1;
        }
        (steps[i], i as i64)
    }

    /// Onsets of one played staff (`notes` as `(on, off, grace)` in units):
    /// each note's placed onset, and each beat's grid (steps a quarter).
    pub(super) fn place_played(
        &self,
        notes: &[(i64, i64, Option<crate::ir::duration::Frac>)],
    ) -> (Vec<i64>, BTreeMap<i64, i64>) {
        let q = self.quarter();
        let tol = self.whole / 64;
        let (merge, switch) = (self.whole / 64, self.whole / 256);
        let steps: Vec<Vec<i64>> = (0..self.bars.len().max(1)).map(|k| self.steps(k)).collect();
        let steps_at = |p: i64| &steps[self.change_at(p).min(steps.len() - 1)];

        // Clusters: notes starting within a 64th of the first.
        let mut order: Vec<usize> = (0..notes.len()).filter(|&i| notes[i].2.is_none()).collect();
        order.sort_by_key(|&i| notes[i].0);
        let mut clusters: Vec<Vec<usize>> = Vec::new();
        for i in order {
            match clusters.last_mut() {
                Some(c) if notes[i].0 - notes[c[0]].0 <= tol => c.push(i),
                _ => clusters.push(vec![i]),
            }
        }

        // The span a point is in: the quarter spans of its bar.
        let span_end = |s: i64| {
            let next = self.span_of(s + q);
            if next > s {
                next
            } else {
                s + q
            }
        };
        let off_16ths = |p: i64| (p - self.span_of(p)) * 16 % q != 0;
        let triplets = self.finest >= 2;

        let mut table: Vec<Vec<State>> = Vec::with_capacity(clusters.len());
        for (c, members) in clusters.iter().enumerate() {
            let r = notes[members[0]].0;
            let shortest = members
                .iter()
                .map(|&i| notes[i].1 - notes[i].0)
                .min()
                .unwrap_or(0);
            // What sounds of it in its bar, and no more than the time since
            // the note before when that is a 16th or more (MuseScore's rule:
            // a note soon after another may start anywhere; a rolled chord's
            // notes are no such thing; the rest after a note says nothing).
            // (A note crossing its bar line by a 16th or more, from a 32nd or
            // more before it, is tied: its written start is what is left of
            // the bar. A note played late, or a downbeat played early, is no
            // such thing.)
            let left = self.bar_end(r) - r;
            let crosses = shortest - left >= q / 4 && left >= q / 8;
            let in_bar = if crosses { left } else { shortest };
            let since = c
                .checked_sub(1)
                .map(|b| r - notes[clusters[b][0]].0)
                .filter(|&d| d >= q / 4)
                .unwrap_or(i64::MAX);
            // The time to the next cluster counts too, up to twice the
            // note's own length (a rest after it says nothing more).
            let gap = clusters.get(c + 1).map_or(0, |n| notes[n[0]].0 - r);
            let dur = in_bar.max(gap).min(2 * in_bar).min(since).max(1);
            let own = steps_at(r);
            let (value, want) = self.expected(in_bar, dur, own);

            // Candidates: the grids of the spans within a beat of it.
            let mut points: Vec<(i64, Family)> = Vec::new();
            let mut s = self.span_of(r - q);
            let last = self.span_of(r + q);
            while s <= last {
                let end = span_end(s);
                let plain = q / self.finest;
                let mut p = s;
                while p < end {
                    points.push((p, Family::Plain));
                    p += plain;
                }
                // Triplets where the meter counts quarters (not 6/8).
                if triplets && end - s == q && steps_at(s).contains(&q) {
                    let t = if self.finest >= 4 { q / 6 } else { q / 3 };
                    let mut p = s;
                    while p < end {
                        points.push((p, Family::Triplet));
                        p += t;
                    }
                }
                s = end;
            }
            points.push((s, Family::Plain));
            // Nothing before the music starts.
            points.retain(|&(p, _)| p >= 0);
            points.sort_unstable();
            points.dedup();

            let emission = |p: i64| {
                let weaker = (self.depth(p, steps_at(p)) - want).max(0);
                (r - p).abs() + value / 4 * weaker
            };

            // Best way to each (point, family, evidence), in that order.
            let prev = table.last();
            let prev_spans: Vec<i64> = prev.map_or(Vec::new(), |pr| {
                pr.iter().map(|st| self.span_of(st.p)).collect()
            });
            let mut states: Vec<State> = Vec::new();
            for &(p, family) in &points {
                let e = emission(p);
                let fresh = u8::from(family == Family::Triplet && off_16ths(p));
                let span = self.span_of(p);
                let mut slot = [(i64::MAX, 0usize); 3];
                match prev {
                    None => {
                        let cost = e + if family == Family::Triplet { switch } else { 0 };
                        slot[usize::from(fresh)] = (cost, 0);
                    }
                    Some(prev) => {
                        // States are in point order: none past `p` can lead here.
                        let upto = prev.partition_point(|st| st.p <= p);
                        for (b, st) in prev[..upto].iter().enumerate() {
                            let step = if p == st.p {
                                // Two clusters on one point.
                                (family == st.family).then_some((st.evidence, merge))
                            } else if span == prev_spans[b] {
                                (family == st.family).then(|| ((st.evidence + fresh).min(2), 0))
                            } else if st.family == Family::Triplet && st.evidence < 2 {
                                None
                            } else {
                                Some((fresh, if family == st.family { 0 } else { switch }))
                            };
                            if let Some((evidence, extra)) = step {
                                let cost = st.cost + e + extra;
                                let s = &mut slot[usize::from(evidence)];
                                if cost < s.0 {
                                    *s = (cost, b);
                                }
                            }
                        }
                    }
                }
                for (evidence, &(cost, back)) in slot.iter().enumerate() {
                    if cost < i64::MAX {
                        states.push(State {
                            p,
                            family,
                            evidence: evidence as u8,
                            cost,
                            back,
                        });
                    }
                }
            }
            table.push(states);
        }

        // Back from the cheapest end (a triplet beat must have its evidence).
        let mut ons = vec![0i64; notes.len()];
        let mut chosen: Vec<(i64, Family)> = Vec::new();
        if let Some(last) = table.last() {
            let mut k = (0..last.len())
                .filter(|&k| last[k].family == Family::Plain || last[k].evidence >= 2)
                .fold(None, |b: Option<usize>, k| match b {
                    Some(b) if last[b].cost <= last[k].cost => Some(b),
                    _ => Some(k),
                })
                .unwrap_or(0);
            for (members, states) in clusters.iter().zip(&table).rev() {
                let st = states[k];
                for &i in members {
                    ons[i] = st.p;
                }
                chosen.push((st.p, st.family));
                k = st.back;
            }
        }
        // Graces (none in played music) keep their onset.
        for (i, n) in notes.iter().enumerate() {
            if n.2.is_some() {
                ons[i] = n.0;
            }
        }

        // Each beat's grid: triplets (6 when a point is a sixth), or the
        // coarsest plain one holding every point.
        let mut beats: BTreeMap<i64, (Family, Vec<i64>)> = BTreeMap::new();
        for (p, family) in chosen {
            let s = self.span_of(p);
            let e = beats.entry(s).or_insert((family, Vec::new()));
            if family == Family::Triplet {
                e.0 = Family::Triplet;
            }
            e.1.push(p - s);
        }
        let grids = beats
            .into_iter()
            .map(|(s, (family, xs))| {
                let n = match family {
                    Family::Triplet if xs.iter().any(|&x| x % (q / 3) != 0) => 6,
                    Family::Triplet => 3,
                    Family::Plain => PLAIN
                        .iter()
                        .copied()
                        .filter(|&n| n <= self.finest)
                        .find(|&n| xs.iter().all(|&x| x % (q / n) == 0))
                        .unwrap_or(self.finest),
                };
                (s, n)
            })
            .collect();
        (ons, grids)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bars, Quantizer};
    use crate::ir::duration::Frac;

    fn bars(meter: (u8, u8)) -> Bars {
        Bars {
            changes: vec![(
                Frac::from_integer(0),
                Frac::new(i64::from(meter.0), i64::from(meter.1)),
            )],
            meters: vec![meter],
        }
    }

    #[test]
    fn meter_steps_and_depths() {
        let qz = Quantizer::new(&bars((4, 4)), 1920, false);
        let u = super::super::SCALE;
        let steps = qz.steps(0);
        let ticks: Vec<i64> = steps.iter().map(|s| s / u).collect();
        assert_eq!(ticks, [1920, 960, 480, 240, 120, 60]);
        let depth = |t: i64| qz.depth(t * u, &steps);
        assert_eq!(
            [0, 960, 480, 240, 120, 60, 160, 80].map(depth),
            [0, 1, 2, 3, 4, 5, 3, 4]
        );
        let qz = Quantizer::new(&bars((6, 8)), 1920, false);
        let ticks: Vec<i64> = qz.steps(0).iter().map(|s| s / u).collect();
        assert_eq!(ticks, [1440, 720, 240, 120, 60]);
    }
}
