//! Quantizing notation-exported MIDI: absolute positions, not durations.
//!
//! Every bar (from the time signatures) is cut into quarter-note spans, and
//! each span takes one grid for its onsets: the coarsest — plain (1, 2, 4, 8,
//! 16 steps a quarter) or tuplet (3, 5, 6, 7, 10, 12) — that puts every onset
//! within two ticks of a point. Music exported from notation lands on such a
//! grid exactly. When none fits that closely (a played file), the span takes
//! the coarsest plain grid within a 64th, else 32nds. Ends snap the same way,
//! preferring the later point, so a note never loses length to rounding.
//!
//! Grace notes, as LilyPond and lytk play them (9/40 of their value, just
//! before the beat), are notes of exactly such a length, off every grid,
//! that end where the next note starts.
//!
//! The arithmetic is on whole numbers: positions in ticks × [`SCALE`], which
//! puts every grid point and bar line of any meter on a whole unit.

use std::collections::BTreeMap;

use crate::ir::duration::Frac;

mod played;

/// Steps per quarter-note span, coarsest first.
const GRIDS: [i64; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16];
/// The plain ones, for played music.
const PLAIN: [i64; 4] = [1, 2, 4, 8];
/// Grids a grace note's onset misses, and the ones an end may snap to
/// exactly: up to 32nds (a LilyPond grace can fall near a 64th by chance).
const NOT_GRACE: [i64; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 10, 12];
/// Units a tick: every grid above divides 1680 steps a quarter, and the 32
/// keeps bar lines of meters down to 1/128 whole at any resolution.
const SCALE: i64 = 1680 * 32;

fn zero() -> Frac {
    Frac::from_integer(0)
}

/// Where bars start: `(anchor, bar length)` from each time signature (the
/// first a pickup when the file starts with one), in position order, and
/// each one's meter `(beats, beat value)` — a pickup's is the meter after it
/// (none given: the bar length's, a whole bar 4/4).
#[derive(Clone, Debug, Default)]
pub(super) struct Bars {
    pub(super) changes: Vec<(Frac, Frac)>,
    pub(super) meters: Vec<(u8, u8)>,
}

impl Bars {
    fn at(&self, p: Frac) -> (Frac, Frac, Option<Frac>) {
        let k = self.changes.partition_point(|(a, _)| *a <= p).max(1) - 1;
        let (anchor, len) = self
            .changes
            .get(k)
            .copied()
            .unwrap_or((zero(), Frac::from_integer(1)));
        let next = self.changes.get(k + 1).map(|c| c.0);
        (anchor, len, next)
    }

    /// Start of the bar holding `p`.
    pub(super) fn start_of(&self, p: Frac) -> Frac {
        let (anchor, len, _) = self.at(p);
        if len <= zero() || p <= anchor {
            return anchor;
        }
        anchor + len * ((p - anchor) / len).floor()
    }

    /// The beat at `p`: the dotted beat of a compound meter (6/8, 9/8,
    /// 12/8), else the meter's beat value.
    pub(super) fn beat_at(&self, p: Frac) -> Frac {
        let k = self.changes.partition_point(|(a, _)| *a <= p).max(1) - 1;
        match self.meters.get(k) {
            Some(&(n, d)) if n % 3 == 0 && n >= 6 => Frac::new(3, i64::from(d.max(1))),
            Some(&(_, d)) => Frac::new(1, i64::from(d.max(1))),
            None => Frac::new(1, 4),
        }
    }

    /// End of the bar holding `p` (a meter change ends it early).
    pub(super) fn end_of(&self, p: Frac) -> Frac {
        let (_, len, next) = self.at(p);
        let end = self.start_of(p) + len;
        next.map_or(end, |n| end.min(n))
    }
}

/// Steps a quarter for `n`, as the tuplet ratio its notes carry.
pub(super) fn ratio_of(n: i64) -> Option<(u8, u8)> {
    match n {
        3 | 6 | 12 => Some((3, 2)),
        5 | 10 => Some((5, 4)),
        7 => Some((7, 4)),
        _ => None,
    }
}

/// One note after quantizing.
#[derive(Clone, Debug)]
pub(super) struct QNote {
    pub(super) on: Frac,
    pub(super) off: Frac,
    pub(super) key: u8,
    pub(super) velocity: u8,
    /// The raw onset tick (lyrics match notes on it).
    pub(super) tick: u64,
    /// Steps a quarter of the grid the onset fell on.
    pub(super) grid: i64,
    /// A grace note: its written value (it sounds before the note at `on`).
    pub(super) grace: Option<Frac>,
    /// How long it sounds, as played (after any swing).
    pub(super) sounding: Frac,
}

pub(super) struct Quantizer {
    /// Units a whole note.
    whole: i64,
    /// `(anchor, bar length)` in units.
    bars: Vec<(i64, i64)>,
    /// Each bar change's meter `(beats, beat value)`.
    meters: Vec<(i64, i64)>,
    /// Whether the file is exported notation (most onsets on a grid exactly).
    pub(super) exported: bool,
    /// Steps a quarter of the finest plain grid for played music (8: 32nds).
    pub(super) finest: i64,
    /// Swung eighths are straightened (the off-beat of a beat with nothing
    /// else inside it moves to the half beat).
    pub(super) swing: bool,
}

impl Quantizer {
    /// A quantizer for music of `ticks_a_whole` (4 × ppq) cut into `bars`.
    pub(super) fn new(bars: &Bars, ticks_a_whole: i64, exported: bool) -> Self {
        let whole = ticks_a_whole * SCALE;
        let unit = |f: Frac| (f * Frac::from_integer(whole)).round().to_integer();
        let meter = |k: usize, len: Frac| match bars.meters.get(k) {
            Some(&(n, d)) => (i64::from(n.max(1)), i64::from(d.max(1))),
            None if len == Frac::from_integer(1) => (4, 4),
            None => (*len.numer(), *len.denom()),
        };
        Quantizer {
            whole,
            bars: bars
                .changes
                .iter()
                .map(|&(a, l)| (unit(a), unit(l)))
                .collect(),
            meters: bars
                .changes
                .iter()
                .enumerate()
                .map(|(k, &(_, l))| meter(k, l))
                .collect(),
            exported,
            finest: 8,
            swing: false,
        }
    }

    fn quarter(&self) -> i64 {
        self.whole / 4
    }

    /// How near a grid point notation lands: two ticks from 384 a quarter,
    /// below that half a tick (a tuplet point rounded to a whole tick: at 96
    /// or 120 a quarter nearly every tick is within a tick or two of some
    /// tuplet point, so a performance would pass for notation).
    fn exact(&self) -> i64 {
        let ppq = self.whole / SCALE / 4;
        if ppq >= 384 {
            2 * SCALE
        } else {
            SCALE / 2
        }
    }

    fn start_of(&self, p: i64) -> i64 {
        let k = self.bars.partition_point(|b| b.0 <= p).max(1) - 1;
        let (anchor, len) = self.bars.get(k).copied().unwrap_or((0, self.whole));
        if len <= 0 || p <= anchor {
            anchor
        } else {
            anchor + len * (p - anchor).div_euclid(len)
        }
    }

    /// Start of the quarter-note span holding `p`, counted from its bar.
    fn span_of(&self, p: i64) -> i64 {
        let b = self.start_of(p);
        b + self.quarter() * (p - b).div_euclid(self.quarter())
    }

    /// The span an onset belongs to: a note played just before a beat (up to
    /// a 64th; in exported music, two ticks) is that beat's first.
    fn beat_of(&self, p: i64) -> i64 {
        let early = if self.exported {
            self.exact()
        } else {
            self.whole / 64 - 1
        };
        self.span_of(p + early)
    }

    /// Each whole beat as played — from its downbeat (the first onset within
    /// a 32nd of the beat, early or late; else the beat) to the next one —
    /// with its onsets inside (chord notes merged: within a 64th of the one
    /// before; those within a 64th of either end on it): `(start, end, the
    /// one onset inside or None for more)`. Beats cut short by a bar line and
    /// beats with nothing inside are left out.
    fn played_beats(&self, ons: impl Iterator<Item = i64>) -> Vec<(i64, i64, Option<i64>)> {
        let (tol, q) = (self.whole / 64, self.quarter());
        let near = q / 8;
        let mut beats: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for p in ons {
            beats.entry(self.span_of(p + near - 1)).or_default().push(p);
        }
        for ps in beats.values_mut() {
            ps.sort_unstable();
        }
        let down = |s: i64| {
            beats
                .get(&s)
                .and_then(|ps| ps.first().copied())
                .filter(|&p| (p - s).abs() < near)
                .unwrap_or(s)
        };
        beats
            .iter()
            .filter(|&(&s, _)| self.span_of(s + q - 1) == s)
            .filter_map(|(&s, ps)| {
                let (start, end) = (down(s), down(s + q));
                let mut inside: Vec<i64> = Vec::new();
                for &p in ps {
                    let merged = inside.last().is_some_and(|&l| p - l <= tol);
                    if p - start > tol && end - p > tol && !merged {
                        inside.push(p);
                    }
                }
                match inside[..] {
                    [] => None,
                    [one] => Some((start, end, Some(one))),
                    _ => Some((start, end, None)),
                }
            })
            .collect()
    }

    /// Whether an off-beat lies where a swung pair puts it: 58 % to 80 % of
    /// the beat from `start` to `end` (swing 3:2 to shuffle 3:1).
    fn in_swing(start: i64, end: i64, p: i64) -> bool {
        let (x, len) = (p - start, end - start);
        50 * x >= 29 * len && 5 * x <= 4 * len
    }

    /// Swung and straight beats of one staff's onsets (in ticks): where in
    /// the beat each one onset inside lies in the swing band (a fraction of
    /// the beat), and how many lie at the half beat (within a 64th).
    pub(super) fn swing_votes(&self, ticks: &[u64]) -> (Vec<Frac>, usize) {
        let beats = self.played_beats(ticks.iter().map(|&t| t as i64 * SCALE));
        let mut swung = Vec::new();
        let mut straight = 0;
        for &(start, end, one) in &beats {
            let len = end - start;
            match one {
                Some(p) if Self::in_swing(start, end, p) => swung.push(Frac::new(p - start, len)),
                Some(p) if (32 * (p - start) - 16 * len).abs() <= 2 * len => straight += 1,
                _ => {}
            }
        }
        (swung, straight)
    }

    /// Distance from `p` to the nearest point of the `n`-step grid of the span
    /// starting at `s`, and that point (ties go to the later one when `late`).
    fn snap(&self, p: i64, s: i64, n: i64, late: bool) -> (i64, i64) {
        let step = self.quarter() / n;
        let r = (p - s).rem_euclid(step);
        let lo = p - r;
        if r < step - r || (r == step - r && !late) {
            (r, lo)
        } else {
            (step - r, lo + step)
        }
    }

    /// The coarsest grid of `grids` that puts every position in `ps` (all in
    /// the span starting at `s`) within `tol`.
    fn fit(&self, ps: &[i64], s: i64, grids: &[i64], tol: i64) -> Option<i64> {
        grids
            .iter()
            .copied()
            .find(|&n| ps.iter().all(|&p| self.snap(p, s, n, false).0 <= tol))
    }

    /// Whether `p` (in whole notes) lies exactly on some grid of its span.
    pub(super) fn is_exact(&self, p: Frac) -> bool {
        let u = (p * Frac::from_integer(self.whole)).round().to_integer();
        self.fit(&[u], self.span_of(u), &GRIDS, self.exact())
            .is_some()
    }

    /// The grid for the positions `ps` of one span of exported music: the
    /// coarsest they lie on exactly, else the coarsest plain one within a
    /// 64th.
    fn grid_for(&self, ps: &[i64], s: i64) -> i64 {
        if let Some(n) = self.fit(ps, s, &GRIDS, self.exact()) {
            return n;
        }
        let plain: Vec<i64> = PLAIN
            .iter()
            .copied()
            .filter(|&n| n <= self.finest)
            .collect();
        self.fit(ps, s, &plain, self.whole / 64)
            .unwrap_or(self.finest)
    }

    /// The written value of a grace note played `len` units long: LilyPond
    /// plays 9/40 of a quarter, eighth, 16th or 32nd, scaled inside a
    /// triplet. The nearest within a tick, if any.
    fn grace_value(&self, len: i64) -> Option<Frac> {
        let (len, whole, tick) = (len as i128, self.whole as i128, SCALE as i128);
        [4i128, 8, 16, 32]
            .iter()
            .flat_map(|&d| [(d, 1, 1), (d, 2, 3)])
            .map(|(d, p, q)| ((len * 40 * d * q - 9 * p * whole).abs(), 40 * d * q, d))
            .filter(|&(err, den, _)| len > 0 && err <= tick * den)
            .min_by(|a, b| (a.0 * b.1).cmp(&(b.0 * a.1)))
            .map(|(.., d)| Frac::new(1, d as i64))
    }

    /// Quantize one staff's notes: `(start, end, key, velocity)` in ticks.
    pub(super) fn quantize(&self, raw: &[(u64, u64, u8, u8)]) -> Vec<QNote> {
        let exact = self.exact();
        // (on, off, grace) in units, then the note.
        let mut notes: Vec<(i64, i64, Option<Frac>)> = raw
            .iter()
            .map(|&(a, b, ..)| (a as i64 * SCALE, b.max(a) as i64 * SCALE, None))
            .collect();

        // Grace notes: 9/40 of a value long (within a tick), off every grid,
        // ending where another note starts.
        if self.exported {
            let mut onsets: Vec<i64> = notes.iter().map(|n| n.0).collect();
            onsets.sort_unstable();
            let mut graces: Vec<usize> = Vec::new();
            for (k, n) in notes.iter_mut().enumerate() {
                let len = n.1 - n.0;
                // An exact 64th or 128th on its own grid is a note.
                let plain = len > 0
                    && [64, 128]
                        .iter()
                        .any(|&d| len * d == self.whole && n.0 % len == 0);
                let value = if plain { None } else { self.grace_value(len) };
                let Some(value) = value else { continue };
                if self
                    .fit(&[n.0], self.span_of(n.0), &NOT_GRACE, exact)
                    .is_some()
                {
                    continue;
                }
                let from = onsets.partition_point(|&o| o < n.1 - exact);
                let leads = onsets[from..]
                    .iter()
                    .take_while(|&&o| o <= n.1 + exact)
                    .any(|&o| o > n.0);
                if leads {
                    n.2 = Some(value);
                    graces.push(k);
                }
            }
            // A grace sounds before the note its group leads to: follow the
            // group (each grace ends where the next starts) to that note.
            let starts: Vec<(i64, i64)> =
                graces.iter().map(|&k| (notes[k].0, notes[k].1)).collect();
            for &k in &graces {
                let mut at = notes[k].1;
                for _ in 0..starts.len() {
                    match starts.iter().find(|(on, _)| (on - at).abs() <= exact) {
                        Some(&(_, off)) if off > at => at = off,
                        _ => break,
                    }
                }
                notes[k].0 = at;
                notes[k].1 = at;
            }
        }

        // Swing: a swung beat is straightened, its off-beat the pivot —
        // [downbeat, pivot] onto the first half, [pivot, next downbeat] onto
        // the second; notes within a 64th after the pivot (a strummed chord)
        // move with it.
        if self.swing {
            let swung: BTreeMap<i64, (i64, i64)> = self
                .played_beats(notes.iter().filter(|n| n.2.is_none()).map(|n| n.0))
                .into_iter()
                .filter_map(|(start, end, one)| {
                    one.filter(|&p| Self::in_swing(start, end, p))
                        .map(|p| (start, (end, p)))
                })
                .collect();
            let tol = self.whole / 64;
            let warp = |p: i64| -> i64 {
                let Some((&start, &(end, pivot))) = swung.range(..=p).next_back() else {
                    return p;
                };
                if p >= end {
                    return p;
                }
                let (p, start, end, pivot) = (p as i128, start as i128, end as i128, pivot as i128);
                let mid = (start + end) / 2;
                let moved = if p <= pivot {
                    start + (p - start) * (mid - start) / (pivot - start)
                } else if p <= pivot + tol as i128 {
                    mid + (p - pivot)
                } else {
                    mid + (p - pivot) * (end - mid) / (end - pivot)
                };
                moved as i64
            };
            for n in notes.iter_mut().filter(|n| n.2.is_none()) {
                let (on, off) = (n.0, n.1);
                n.0 = warp(on);
                n.1 = warp(off).max(n.0);
            }
        }

        // Onsets: exported music on one grid per span; played music by the
        // search in `played`.
        let (placed, grids) = if self.exported {
            let mut spans: BTreeMap<i64, Vec<i64>> = Default::default();
            for n in notes.iter().filter(|n| n.2.is_none()) {
                spans.entry(self.beat_of(n.0)).or_default().push(n.0);
            }
            let grids: BTreeMap<i64, i64> = spans
                .iter()
                .map(|(&s, ps)| (s, self.grid_for(ps, s)))
                .collect();
            let placed: Vec<i64> = notes
                .iter()
                .map(|n| {
                    let s = self.beat_of(n.0);
                    let g = grids
                        .get(&s)
                        .copied()
                        .unwrap_or_else(|| self.grid_for(&[n.0], s));
                    self.snap(n.0, s, g, false).1
                })
                .collect();
            (placed, grids)
        } else {
            self.place_played(&notes)
        };
        notes
            .iter()
            .zip(raw)
            .zip(placed)
            .map(|((&(raw_on, off, grace), &(tick, _, key, velocity)), on)| {
                let off_raw = off;
                let s = self.span_of(on);
                let g = match grids.get(&s) {
                    Some(&g) => g,
                    None if self.exported => self.grid_for(&[on], s),
                    None => 1,
                };
                // A played note's end, where no onset is near, on the
                // coarsest grid of steps up to 5/8 of its length (a half
                // played short ends on a beat, a dotted quarter on an eighth).
                let own = if self.exported {
                    g
                } else {
                    let len = (off - raw_on).max(1);
                    PLAIN
                        .iter()
                        .copied()
                        .filter(|&n| n <= self.finest)
                        .find(|&n| 8 * (self.quarter() / n) <= 5 * len)
                        .unwrap_or(self.finest)
                };
                let off = match self.snap_end(off, &grids, own) {
                    _ if grace.is_some() => on,
                    // Snapped away entirely: one step of its onset's grid.
                    off if off <= on => on + self.quarter() / g,
                    off => off,
                };
                QNote {
                    on: Frac::new(on, self.whole),
                    off: Frac::new(off, self.whole),
                    key,
                    velocity,
                    tick,
                    grid: g,
                    grace,
                    sounding: Frac::new((off_raw - raw_on).max(0), self.whole),
                }
            })
            .collect()
    }

    /// Snap an end: onto its span's onset grid — or, where nothing starts,
    /// its note's (`own`) — when it lies there; in exported music else onto
    /// the coarsest grid it lies on exactly (up to 32nds: finer grids catch
    /// played and grace-cut ends by chance); else to the nearest point of
    /// that grid, the later one on a tie.
    fn snap_end(&self, p: i64, grids: &std::collections::BTreeMap<i64, i64>, own: i64) -> i64 {
        let s = self.span_of(p);
        let n = grids.get(&s).copied().unwrap_or(own);
        // Distinct points of these grids are a 48th apart at least, so an
        // exact match is the only one.
        let (d, at) = self.snap(p, s, n, true);
        if d <= self.exact() {
            return at;
        }
        if let Some(m) = self
            .exported
            .then(|| self.fit(&[p], s, &NOT_GRACE, self.exact()))
            .flatten()
        {
            return self.snap(p, s, m, true).1;
        }
        at
    }
}

/// The tuplet ratio a note or rest of `len` is written with: none when it
/// spells without one (a whole beat in a triplet beat is a quarter), else its
/// onset grid's (`hint`), else the first ratio that spells it.
pub(super) fn written_ratio(len: Frac, hint: Option<(u8, u8)>) -> Option<(u8, u8)> {
    let dyadic = |r: Option<(u8, u8)>| {
        let (a, n) = r.unwrap_or((1, 1));
        let w = len * Frac::new(a as i64, n as i64);
        (*w.denom() as u64).is_power_of_two()
    };
    [None, hint, Some((3, 2)), Some((5, 4)), Some((7, 4))]
        .into_iter()
        .find(|r| dyadic(*r))
        .unwrap_or(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: i64, d: i64) -> Frac {
        Frac::new(n, d)
    }

    fn four_four() -> Bars {
        Bars {
            changes: vec![(zero(), Frac::from_integer(1))],
            ..Bars::default()
        }
    }

    #[test]
    fn bars_follow_pickups_and_meter_changes() {
        let b = Bars {
            changes: vec![(zero(), q(1, 4)), (q(1, 4), q(3, 4)), (q(7, 4), q(1, 2))],
            ..Bars::default()
        };
        assert_eq!(b.start_of(q(1, 8)), zero());
        assert_eq!(b.end_of(q(1, 8)), q(1, 4));
        assert_eq!(b.start_of(q(5, 4)), q(1, 1));
        assert_eq!(b.end_of(q(5, 4)), q(7, 4));
        assert_eq!(b.start_of(q(9, 4)), q(9, 4));
        let qz = Quantizer::new(&b, 1920, true);
        // 1200 ticks: in the bar from 480 (after the pickup), its third beat.
        assert_eq!(qz.span_of(1200 * SCALE), 960 * SCALE);
    }

    #[test]
    fn exported_tuplets_find_their_grid() {
        let qz = Quantizer::new(&four_four(), 1920, true);
        // Eighth triplets, then a quarter; 480 ticks a quarter.
        let raw: Vec<_> = (0..3u64)
            .map(|i| (i * 160, i * 160 + 160, 60, 90))
            .chain([(480, 960, 62, 90)])
            .collect();
        let n = qz.quantize(&raw);
        assert_eq!(n[1].on, q(1, 12));
        assert_eq!(n[1].grid, 3);
        assert_eq!(n[3].grid, 1);
        assert_eq!(
            written_ratio(n[1].off - n[1].on, ratio_of(n[1].grid)),
            Some((3, 2))
        );
        // A triplet half starting on the beat is still a triplet.
        assert_eq!(written_ratio(q(1, 3), None), Some((3, 2)));
        assert_eq!(written_ratio(q(3, 8), None), None);
    }

    #[test]
    fn played_notes_snap_to_the_beat() {
        let qz = Quantizer::new(&four_four(), 1920, false);
        let raw = [(0, 455, 60, 90), (492, 930, 62, 90), (955, 1400, 64, 90)];
        let n = qz.quantize(&raw);
        let got: Vec<_> = n.iter().map(|n| (n.on, n.off)).collect();
        assert_eq!(
            got,
            vec![(zero(), q(1, 4)), (q(1, 4), q(1, 2)), (q(1, 2), q(3, 4))]
        );
    }

    #[test]
    fn lilypond_grace_notes_are_found() {
        let qz = Quantizer::new(&four_four(), 1536, true);
        // A grace 16th before beat 2 at 384 a quarter: 9/40 of 96 ticks.
        let raw = [(0, 362, 60, 90), (362, 384, 67, 90), (384, 768, 65, 90)];
        let n = qz.quantize(&raw);
        assert_eq!(n[1].grace, Some(q(1, 16)));
        assert_eq!((n[1].on, n[1].off), (q(1, 4), q(1, 4)));
        // The note it cut short still ends on the beat.
        assert_eq!(n[0].off, q(1, 4));
    }

    #[test]
    fn grace_groups_lead_to_their_note_and_128ths_are_notes() {
        let qz = Quantizer::new(&four_four(), 1536, true);
        // Triplet 16ths (32 ticks), then two grace eighths (43 ticks each)
        // before beat 2 at 384 a quarter.
        let mut raw: Vec<(u64, u64, u8, u8)> =
            (0..9u64).map(|i| (i * 32, i * 32 + 32, 60, 90)).collect();
        raw.truncate(9);
        raw.push((297, 340, 71, 90));
        raw.push((340, 383, 72, 90));
        raw.push((384, 768, 74, 90));
        let n = qz.quantize(&raw);
        let graces: Vec<_> = n.iter().filter(|x| x.grace.is_some()).collect();
        assert_eq!(graces.len(), 2);
        assert!(graces.iter().all(|g| g.on == q(1, 4)), "{graces:?}");
        // A run of 128ths (12 ticks) stays notes.
        let run: Vec<(u64, u64, u8, u8)> = (0..8u64)
            .map(|i| (384 + i * 12, 396 + i * 12, 60 + i as u8, 90))
            .collect();
        let mut raw = vec![(0, 384, 59, 90)];
        raw.extend(run);
        raw.push((480, 768, 70, 90));
        assert!(qz.quantize(&raw).iter().all(|x| x.grace.is_none()));
        // At 256 a quarter too, where a 32nd grace is within a tick of them.
        let qz = Quantizer::new(&four_four(), 1024, true);
        let mut raw = vec![(0, 256, 59, 90)];
        raw.extend((0..8u64).map(|i| (256 + i * 8, 264 + i * 8, 60 + i as u8, 90)));
        raw.push((320, 512, 70, 90));
        assert!(qz.quantize(&raw).iter().all(|x| x.grace.is_none()));
        // A zero-length note is no grace (and no panic).
        let qz = Quantizer::new(&four_four(), 256, true);
        let raw = [(0, 64, 60, 90), (3, 3, 62, 90), (4, 64, 64, 90)];
        assert!(qz.quantize(&raw).iter().all(|x| x.grace.is_none()));
    }

    #[test]
    fn a_grace_inside_a_triplet_is_found() {
        // `\tuplet 3/2 { c'8 \grace d'16 e'8 f'8 } g'2.` at 384 a quarter:
        // LilyPond 2.22 plays the grace 9/40 of a triplet 16th, before e'.
        let qz = Quantizer::new(&four_four(), 1536, true);
        let raw = [
            (0, 113, 60, 90),
            (113, 127, 62, 90),
            (128, 256, 64, 90),
            (256, 384, 65, 90),
            (384, 1536, 67, 90),
        ];
        let n = qz.quantize(&raw);
        assert_eq!(n[1].grace, Some(q(1, 16)));
        assert_eq!(n[1].on, q(1, 12));
        assert!(
            n.iter()
                .filter(|x| x.grace.is_none())
                .take(3)
                .all(|x| x.grid == 3),
            "{n:?}"
        );
    }

    #[test]
    fn swung_pairs_are_straightened_when_asked() {
        let mut qz = Quantizer::new(&four_four(), 1920, false);
        let pair = [(0, 300, 60, 90), (322, 470, 62, 90)];
        // Unasked, a lone swung pair reads as a dotted eighth and a 16th.
        assert_eq!(qz.quantize(&pair)[1].on, q(3, 16));
        qz.swing = true;
        let n = qz.quantize(&pair);
        assert_eq!((n[1].on, n[1].grid), (q(1, 8), 2));
        assert!(n[0].off <= q(1, 8), "{n:?}");
        // A shuffle (3:1) too.
        let shuffle = [(0, 340, 60, 90), (362, 470, 62, 90)];
        assert_eq!(qz.quantize(&shuffle)[1].on, q(1, 8));
        // A beat of triplets has two onsets inside: left as it is.
        let trips = [(480, 630, 60, 90), (640, 790, 62, 90), (800, 950, 64, 90)];
        assert_eq!(qz.quantize(&trips)[1].grid, 3);
    }

    #[test]
    fn played_triplets_and_uneven_sixteenths() {
        let qz = Quantizer::new(&four_four(), 1920, false);
        // Eighth triplets played ±10 ticks.
        let trips = [(0, 150, 60, 90), (168, 310, 62, 90), (312, 470, 64, 90)];
        let n = qz.quantize(&trips);
        assert_eq!(n[1].on, q(1, 12));
        assert_eq!(n[1].grid, 3);
        // Sixteenths played unevenly stay sixteenths.
        let sixteenths = [
            (0, 110, 60, 90),
            (128, 230, 62, 90),
            (236, 350, 64, 90),
            (366, 470, 65, 90),
        ];
        let n = qz.quantize(&sixteenths);
        assert!(n.iter().all(|x| x.grid == 4 || x.grid == 1), "{n:?}");
        assert_eq!(n[1].on, q(1, 16));
        // A triplet beat whose first note comes 7 ticks early, one onset
        // only just nearer its triplet point than a 16th.
        let late = [(473, 622, 62, 90), (626, 777, 64, 90), (802, 945, 65, 90)];
        let n = qz.quantize(&late);
        let ons: Vec<Frac> = n.iter().map(|x| x.on).collect();
        assert_eq!(ons, [q(1, 4), q(1, 3), q(5, 12)], "{n:?}");
        // One onset inside the beat is no evidence: a dotted rhythm stays.
        let dotted = [(0, 330, 60, 90), (345, 470, 62, 90)];
        assert_eq!(qz.quantize(&dotted)[1].on, q(3, 16));
    }
}
