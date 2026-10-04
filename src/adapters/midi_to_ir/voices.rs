//! Chords and voices from quantized notes, then each voice as positioned
//! notation (notes, chords, rests, grace notes, tuplet brackets).
//!
//! Notes that start and end together are a chord. Voices follow MuseScore's
//! idea, simplified: an event goes to a free voice (one whose last note has
//! ended), the one nearest in pitch; at most four voices, the busiest cut
//! short when a fifth would be needed; voices are then numbered from the top.
//! Within a voice, a note that stops short of the next by no more than its
//! own length is held to it: a staccato (marked when it needed a third or
//! more) or an articulation-shortened note, not a written rest.

use crate::ir::articulation::{
    Articulation, ArticulationType, DynamicMark, DynamicType, Placement, ShowNumber, StartStop,
    TieEvent, TupletDisplay,
};
use crate::ir::duration::{Duration, Frac};
use crate::ir::notate::notate;
use crate::ir::note::{Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::Pitch;

use super::quantize::{ratio_of, written_ratio, Bars, QNote};

const MAX_VOICES: usize = 4;

/// A chord (or single note) in a voice.
#[derive(Clone, Debug)]
pub(super) struct Event {
    pub(super) on: Frac,
    pub(super) off: Frac,
    /// (key, velocity, lyric), lowest key first.
    pub(super) notes: Vec<(u8, u8, Option<String>)>,
    pub(super) grid: i64,
    /// Grace notes before it: (key, velocity, written value).
    pub(super) graces: Vec<(u8, u8, Frac)>,
    pub(super) staccato: bool,
    /// A dynamic mark where the part's level changes.
    pub(super) dynamic: Option<DynamicType>,
    /// How long its longest note sounds, as played.
    pub(super) sounding: Frac,
}

impl Event {
    fn top(&self) -> u8 {
        self.notes.last().map_or(60, |n| n.0)
    }
}

/// Split one staff's notes into voices, top voice first. `lyric` gives the
/// lyric sung on a note, by its raw onset tick and key; `half_second` is
/// half a second in wholes at a position (LilyPond's longest staccato), and
/// `written` whether the file was made from notation (only such a file
/// marks a staccato by playing it 4 louder; a played staccato is found by
/// its length, in `bars`).
pub(super) fn separate(
    notes: Vec<QNote>,
    lyric: impl Fn(u64, u8) -> Option<String>,
    half_second: impl Fn(Frac) -> Frac,
    percussion: bool,
    written: bool,
    bars: &Bars,
    sustained: &dyn Fn(Frac) -> bool,
) -> Vec<Vec<Event>> {
    let (mut graces, mut plain): (Vec<QNote>, Vec<QNote>) =
        notes.into_iter().partition(|n| n.grace.is_some());
    // A group's graces in the order they were played.
    graces.sort_by_key(|g| g.tick);
    plain.sort_by_key(|n| (n.on, n.off, n.key));

    // Chords: same start, same end.
    let mut events: Vec<Event> = Vec::new();
    for n in plain {
        let l = lyric(n.tick, n.key);
        match events.last_mut() {
            Some(e) if e.on == n.on && e.off == n.off => {
                e.notes.push((n.key, n.velocity, l));
                e.sounding = e.sounding.max(n.sounding);
            }
            _ => events.push(Event {
                on: n.on,
                off: n.off,
                notes: vec![(n.key, n.velocity, l)],
                grid: n.grid,
                graces: Vec::new(),
                staccato: false,
                dynamic: None,
                sounding: n.sounding,
            }),
        }
    }
    // Higher events first at one onset, so the top line keeps voice 1.
    events.sort_by(|a, b| a.on.cmp(&b.on).then(b.top().cmp(&a.top())));

    let mut voices: Vec<Vec<Event>> = Vec::new();
    for e in events {
        let free = voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.last().is_none_or(|l| l.off <= e.on))
            .min_by_key(|(_, v)| {
                v.last()
                    .map_or(0, |l| (l.top() as i32 - e.top() as i32).abs())
            })
            .map(|(i, _)| i);
        let i = match free {
            Some(i) => i,
            None if voices.len() < MAX_VOICES => {
                voices.push(Vec::new());
                voices.len() - 1
            }
            None => {
                // Every voice busy: cut the one that frees soonest.
                let i = (0..voices.len())
                    .min_by_key(|&i| voices[i].last().map(|l| l.off))
                    .unwrap_or(0);
                let last = voices[i].last_mut().expect("a busy voice has notes");
                if last.on < e.on {
                    last.off = e.on;
                } else {
                    // Merged into a chord: both cut to the shorter (it sounds
                    // as long as its longest).
                    last.off = last.off.min(e.off);
                    last.sounding = last.sounding.max(e.sounding);
                    last.notes.extend(e.notes);
                    last.notes.sort_by_key(|n| n.0);
                    continue;
                }
                i
            }
        };
        voices[i].push(e);
    }

    for v in voices.iter_mut() {
        hold_to_next(v, &half_second, percussion, written, bars, sustained);
    }
    // Top voice first.
    let mean = |v: &Vec<Event>| {
        let keys: Vec<i64> = v
            .iter()
            .flat_map(|e| e.notes.iter().map(|n| n.0 as i64))
            .collect();
        Frac::new(keys.iter().sum::<i64>(), keys.len().max(1) as i64)
    };
    voices.sort_by_key(|v| std::cmp::Reverse(mean(v)));

    // Grace notes go before the event they lead to (nearest in pitch).
    for g in graces {
        let target = voices
            .iter_mut()
            .flat_map(|v| v.iter_mut())
            .filter(|e| e.on == g.on)
            .min_by_key(|e| (e.top() as i32 - g.key as i32).abs());
        if let (Some(e), Some(w)) = (target, g.grace) {
            e.graces.push((g.key, g.velocity, w));
        }
    }
    voices
}

/// Hold a note to the next when the gap after it is what playing leaves:
/// a third of the note or less (legato), or — marked staccato — no longer
/// than the note when it is 4 louder than a neighbour and sounds no longer
/// than half a second (how LilyPond and lytk play a staccato: half the
/// note, at most half a second). Without that a note and a rest of its
/// length stay a note and a rest: timing alone can't tell them from a
/// staccato. A drum hit lasts until the next (`percussion`), the last as
/// long as the one before it.
fn hold_to_next(
    voice: &mut [Event],
    half_second: &dyn Fn(Frac) -> Frac,
    percussion: bool,
    written: bool,
    bars: &Bars,
    sustained: &dyn Fn(Frac) -> bool,
) {
    let n = voice.len();
    let raw: Vec<Frac> = voice.iter().map(|e| e.off - e.on).collect();
    // Played: a note well short of where it could end — the next onset, or
    // the next point of its own grid after its sound, within its bar — is a
    // staccato of that
    // length, when it sounds two thirds of it or less (MuseScore marks from
    // 70 %, but on its quantized lengths; lytk has the sounding one, and a
    // dotted quarter at a 90 % gate is 67.5 % of a half), it is then one
    // written value, and the silence is a 32nd or more. (A release within a
    // 64th past a beat is on it; a note let go under the pedal still
    // sounds.)
    if !written && !percussion {
        let one_value = |d: Frac| {
            d > Frac::from_integer(0) && d.denom().count_ones() == 1 && matches!(d.numer(), 1 | 3)
        };
        for k in 0..n {
            let e = &voice[k];
            if sustained(e.on + e.sounding) {
                continue;
            }
            let end = (e.on + e.sounding - Frac::new(1, 64)).max(e.on);
            // The note's own grid: the coarsest of the beat and its halves
            // down to an eighth that its start lies on (a quarter starting
            // off the beat goes to the next half beat, not the next beat).
            let bar = bars.start_of(e.on);
            let mut step = bars.beat_at(e.on);
            for _ in 0..3 {
                if ((e.on - bar) / step).is_integer() {
                    break;
                }
                step /= 2;
            }
            // Its next point after the sound (after its start, at least),
            // never past the bar it starts in.
            let mut after = bar + ((end - bar) / step).ceil() * step;
            if after <= e.on {
                after += step;
            }
            let after = after.min(bars.end_of(e.on));
            let target = voice.get(k + 1).map_or(after, |next| after.min(next.on));
            let len = target - e.on;
            if target >= e.off
                && one_value(len)
                && e.sounding * Frac::from_integer(3) <= len * Frac::from_integer(2)
                && len - e.sounding >= Frac::new(1, 32)
            {
                voice[k].off = target;
                voice[k].staccato = true;
            }
        }
    }
    let loud = |e: &Event| e.notes.iter().map(|x| x.1 as i32).max().unwrap_or(0);
    // 4 louder than the nearest note either side that isn't as loud, over a
    // run of as loud notes each followed by a rest (a run of staccatos) —
    // found in one pass each way.
    let detached = |j: usize| voice.get(j + 1).is_none_or(|next| next.on > voice[j].off);
    let mut before: Vec<Option<i32>> = vec![None; n];
    for k in 1..n {
        let (me, j) = (loud(&voice[k]), k - 1);
        before[k] = if loud(&voice[j]) != me {
            Some(loud(&voice[j]))
        } else if detached(j) {
            before[j]
        } else {
            None
        };
    }
    let mut after: Vec<Option<i32>> = vec![None; n];
    for k in (0..n.saturating_sub(1)).rev() {
        let (me, j) = (loud(&voice[k]), k + 1);
        after[k] = if loud(&voice[j]) != me {
            Some(loud(&voice[j]))
        } else if detached(j) {
            after[j]
        } else {
            None
        };
    }
    let played_staccato = |voice: &[Event], k: usize| {
        let me = loud(&voice[k]);
        written
            && raw[k] <= half_second(voice[k].on)
            && [before[k], after[k]]
                .into_iter()
                .flatten()
                .any(|o| me == o + 4)
    };
    for k in 0..n.saturating_sub(1) {
        let (len, gap) = (raw[k], voice[k + 1].on - voice[k].off);
        // (A played staccato keeps its length and mark.)
        if gap <= Frac::from_integer(0) || voice[k].staccato {
            continue;
        }
        // (Under a third: a dotted quarter and an eighth rest stay; and in
        // played music a 16th at most: a whole note and an eighth rest stay.
        // LilyPond plays a portato half short by an eighth.)
        let legato = gap * Frac::from_integer(3) < len && (written || gap <= Frac::new(1, 16));
        let staccato = gap <= len && played_staccato(voice, k);
        if percussion || legato || staccato {
            let next = voice[k + 1].on;
            let e = &mut voice[k];
            e.staccato = !percussion && !legato;
            e.off = next;
        }
    }
    if n >= 2 {
        let last = n - 1;
        if percussion && raw[last] == raw[last - 1] {
            let held = voice[last - 1].off - voice[last - 1].on;
            voice[last].off = voice[last].on + held.max(raw[last]);
        } else if !percussion
            && played_staccato(voice, last)
            && raw[last] < half_second(voice[last].on)
        {
            // Under half a second, so played at half its length (a note
            // cut at half a second could be any length: it stays as is).
            let e = &mut voice[last];
            e.staccato = true;
            e.off = e.on + raw[last] * Frac::from_integer(2);
        }
    }
}

/// How one voice is written: its lane, staff and bars, and how keys are
/// spelled at a position.
pub(super) struct Writer<'a> {
    pub(super) lane: u8,
    pub(super) staff: u8,
    pub(super) bars: &'a Bars,
    /// The first voice of a staff shows every rest; the others only rests in
    /// the bars where they sing.
    pub(super) main: bool,
    pub(super) spell: &'a dyn Fn(u8, Frac) -> Pitch,
}

impl Writer<'_> {
    /// The voice as positioned elements.
    pub(super) fn elements(&self, voice: &[Event]) -> Vec<(Frac, VoiceElement)> {
        let mut out: Vec<(Frac, VoiceElement)> = Vec::new();
        let zero = Frac::from_integer(0);
        let mut cursor = match voice.first() {
            Some(_) if self.main => zero,
            Some(e) => self.bars.start_of(e.on),
            None => return out,
        };
        // The grid of the note before a rest (its beat's, roughly the rest's).
        let mut last_grid = 1;
        for e in voice {
            self.rests(&mut out, cursor, e.on, (last_grid, e.grid));
            last_grid = e.grid;
            for &(key, velocity, value) in &e.graces {
                let mut n = Note::new((self.spell)(key, e.on), Duration::new(value));
                n.is_grace = true;
                n.grace_slash = true;
                n.velocity = Some(velocity);
                out.push((e.on, VoiceElement::Note(Box::new(self.place_note(n)))));
            }
            let len = e.off - e.on;
            let pieces = notate(len, written_ratio(len, ratio_of(e.grid)));
            let last = pieces.len() - 1;
            let mut at = e.on;
            for (k, d) in pieces.into_iter().enumerate() {
                let step = d.actual_duration();
                let ties = |n: &mut Note| {
                    if k > 0 {
                        n.ties.push(TieEvent {
                            tie_type: StartStop::Stop,
                        });
                    }
                    if k < last {
                        n.ties.push(TieEvent {
                            tie_type: StartStop::Start,
                        });
                    }
                };
                let mut notes: Vec<Note> = e
                    .notes
                    .iter()
                    .map(|(key, velocity, lyric)| {
                        let mut n = Note::new((self.spell)(*key, e.on), d.clone());
                        n.velocity = Some(*velocity);
                        ties(&mut n);
                        if k == 0 {
                            if let Some(text) = lyric {
                                n.lyrics.push(syllable(text));
                            }
                        }
                        self.place_note(n)
                    })
                    .collect();
                if k == 0 && e.staccato {
                    notes[0].articulations.push(Articulation {
                        name: ArticulationType::Staccato,
                        placement: Placement::default(),
                    });
                }
                if let (0, Some(sign)) = (k, e.dynamic.clone()) {
                    notes[0].dynamics.push(DynamicMark {
                        sign,
                        placement: Placement::default(),
                    });
                }
                let elem = if notes.len() == 1 {
                    VoiceElement::Note(Box::new(notes.remove(0)))
                } else {
                    let mut c = Chord::new(d, notes);
                    (c.voice, c.staff) = (self.lane, self.staff);
                    VoiceElement::Chord(c)
                };
                out.push((at, elem));
                at += step;
            }
            cursor = e.off;
        }
        if let Some(e) = voice.last() {
            let end = self.bars.end_of(e.off - Frac::new(1, 1_000_000));
            self.rests(&mut out, cursor, end.max(cursor), (e.grid, 1));
        }
        brackets(&mut out);
        out
    }

    fn place_note(&self, mut n: Note) -> Note {
        (n.voice, n.staff) = (self.lane, self.staff);
        n
    }

    /// Rests from `a` to `b`: all of it in a staff's main voice; in another
    /// voice only up to the end of the bar it stops in and from the start of
    /// the bar it resumes in (the bars between stay empty).
    /// `grids`: the grid of the beat the rest starts in and of the one it
    /// ends in; a rest keeps a tuplet only inside a tuplet beat (the gap in
    /// `(3 c z e`), and whole beats between are plain.
    fn rests(&self, out: &mut Vec<(Frac, VoiceElement)>, a: Frac, b: Frac, grids: (i64, i64)) {
        if b <= a {
            return;
        }
        // Where the bar the voice stopped in ends (nothing to fill when it
        // stopped on a bar line).
        let head_end = if self.bars.start_of(a) == a {
            a
        } else {
            self.bars.end_of(a)
        };
        let spans = if self.main || head_end >= b {
            vec![(a, b)]
        } else {
            vec![(a, head_end), (self.bars.start_of(b), b)]
        };
        let quarter = Frac::new(1, 4);
        let beat = |p: Frac| {
            let bar = self.bars.start_of(p);
            bar + quarter * ((p - bar) / quarter).floor()
        };
        for (from, to) in spans.into_iter().filter(|(f, t)| t > f) {
            // Only a tuplet beat is cut off: plain rests spell as one.
            let head_end = match ratio_of(grids.0) {
                Some(_) => (beat(from) + quarter).min(to),
                None => from,
            };
            let tail_start = match ratio_of(grids.1) {
                Some(_) => beat(to).max(head_end),
                None => to,
            };
            let pieces = [
                (from, head_end, grids.0),
                (head_end, tail_start, 1),
                (tail_start, to, grids.1),
            ];
            // (A rest wholly in one tuplet beat is its head.)
            for (x, y, grid) in pieces.into_iter().filter(|(x, y, _)| y > x) {
                let mut at = x;
                let len = y - x;
                for d in notate(len, written_ratio(len, ratio_of(grid))) {
                    let step = d.actual_duration();
                    let mut r = Rest::new(d);
                    (r.voice, r.staff) = (self.lane, self.staff);
                    out.push((at, VoiceElement::Rest(r)));
                    at += step;
                }
            }
        }
    }
}

/// Marks a lyric whose word the syllable before began (`Hal-` `le`).
pub(super) const CONTINUES: char = '\u{1}';

/// `la-` is a syllable the next one continues; one after such a syllable
/// (marked [`CONTINUES`]) continues a word.
fn syllable(text: &str) -> crate::ir::articulation::LyricSyllable {
    use crate::ir::articulation::{LyricSyllable, SyllabicType};
    let (continues, text) = match text.strip_prefix(CONTINUES) {
        Some(t) => (true, t),
        None => (false, text),
    };
    let (text, goes_on) = match text.strip_suffix('-') {
        Some(t) => (t, true),
        None => (text, false),
    };
    let syllabic = match (continues, goes_on) {
        (false, false) => SyllabicType::Single,
        (false, true) => SyllabicType::Begin,
        (true, true) => SyllabicType::Middle,
        (true, false) => SyllabicType::End,
    };
    LyricSyllable {
        text: text.to_string(),
        syllabic,
        number: 1,
        extend: false,
        elision: false,
        name: None,
    }
}

/// Bracket each run of tuplet notes and rests that fills a whole number of
/// quarter-note beats (a triplet of eighths, three triplet quarters, …).
fn brackets(out: &mut [(Frac, VoiceElement)]) {
    let ratio = |e: &VoiceElement| {
        let d = match e {
            VoiceElement::Note(n) if !n.is_grace => &n.duration,
            VoiceElement::Chord(c) => &c.duration,
            VoiceElement::Rest(r) => &r.duration,
            _ => return None,
        };
        (d.tuplet_actual > 1).then_some((d.tuplet_actual, d.tuplet_normal))
    };
    let mut start: Option<(usize, (u8, u8), Frac)> = None;
    for k in 0..out.len() {
        // Grace notes sit inside a group without breaking it.
        if matches!(&out[k].1, VoiceElement::Note(n) if n.is_grace) {
            continue;
        }
        let r = ratio(&out[k].1);
        if let Some((s, sr, from)) = start {
            if r != Some(sr) {
                start = None; // an unfinished group gets no bracket
            } else {
                let end = out[k].0 + out[k].1.metric_duration();
                if ((end - from) / Frac::new(1, 4)).is_integer() {
                    mark(&mut out[s].1, StartStop::Start);
                    mark(&mut out[k].1, StartStop::Stop);
                    start = None;
                }
                continue;
            }
        }
        if let Some(r) = r {
            let end = out[k].0 + out[k].1.metric_duration();
            if ((end - out[k].0) / Frac::new(1, 4)).is_integer() {
                continue; // a lone tuplet value filling a beat needs none
            }
            start = Some((k, r, out[k].0));
        }
    }
}

fn mark(e: &mut VoiceElement, t: StartStop) {
    let d = TupletDisplay {
        tuplet_type: t,
        bracket: true,
        show_number: (t == StartStop::Start).then_some(ShowNumber::Actual),
    };
    match e {
        VoiceElement::Note(n) => n.tuplet = Some(d),
        VoiceElement::Rest(r) => r.tuplet = Some(d),
        VoiceElement::Chord(c) => {
            if let Some(n) = c.notes.first_mut() {
                n.tuplet = Some(d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qn(on: (i64, i64), off: (i64, i64), key: u8) -> QNote {
        QNote {
            on: Frac::new(on.0, on.1),
            off: Frac::new(off.0, off.1),
            key,
            velocity: 90,
            tick: 0,
            grid: 1,
            grace: None,
            sounding: Frac::new(off.0, off.1) - Frac::new(on.0, on.1),
        }
    }

    /// Half a second at 120 bpm.
    fn quarter(_: Frac) -> Frac {
        Frac::new(1, 4)
    }

    fn whole_bars() -> Bars {
        Bars {
            changes: vec![(Frac::from_integer(0), Frac::from_integer(1))],
            ..Bars::default()
        }
    }

    #[test]
    fn a_held_note_under_moving_ones_is_its_own_voice() {
        let notes = vec![
            qn((0, 1), (1, 1), 48),
            qn((0, 1), (1, 4), 64),
            qn((1, 4), (1, 2), 67),
            qn((1, 2), (3, 4), 64),
        ];
        let v = separate(
            notes,
            |_, _| None,
            quarter,
            false,
            true,
            &whole_bars(),
            &|_| false,
        );
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 3, "the moving line on top");
        assert_eq!(v[1][0].notes[0].0, 48);
    }

    #[test]
    fn chords_and_detached_notes() {
        let mut notes = vec![
            qn((0, 1), (1, 8), 60),
            qn((0, 1), (1, 8), 64),
            qn((1, 4), (1, 2), 62),
        ];
        // Played 4 louder, as LilyPond plays a staccato.
        notes[0].velocity = 94;
        notes[1].velocity = 94;
        let v = separate(
            notes,
            |_, _| None,
            quarter,
            false,
            true,
            &whole_bars(),
            &|_| false,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0][0].notes.len(), 2);
        assert_eq!(v[0][0].off, Frac::new(1, 4));
        assert!(v[0][0].staccato);
        // A lone note and a rest of its length stay a note and a rest.
        let lone = separate(
            vec![qn((0, 1), (1, 4), 60), qn((1, 2), (1, 1), 62)],
            |_, _| None,
            quarter,
            false,
            true,
            &whole_bars(),
            &|_| false,
        );
        assert_eq!(lone[0][0].off, Frac::new(1, 4));
    }

    #[test]
    fn a_last_staccato_is_under_half_a_second() {
        let last = |notes: Vec<QNote>| {
            let v = separate(
                notes,
                |_, _| None,
                quarter,
                false,
                true,
                &whole_bars(),
                &|_| false,
            );
            let e = v[0].last().unwrap();
            (e.staccato, e.off)
        };
        // `c'1 d'1` with d' 4 louder: a second long, not a staccato.
        let mut held = vec![qn((0, 1), (1, 1), 60), qn((1, 1), (2, 1), 62)];
        held[1].velocity = 94;
        assert_eq!(last(held), (false, Frac::from_integer(2)));
        // `c'8 d'8 g'4-.`: the staccato after shorter notes.
        let mut short = vec![
            qn((0, 1), (1, 8), 60),
            qn((1, 8), (1, 4), 62),
            qn((1, 4), (3, 8), 67),
        ];
        short[2].velocity = 94;
        assert_eq!(last(short), (true, Frac::new(1, 2)));
    }

    #[test]
    fn a_fifth_voice_is_cut_not_lengthened() {
        let notes = vec![
            qn((0, 1), (1, 1), 72),
            qn((0, 1), (3, 4), 67),
            qn((0, 1), (1, 2), 64),
            qn((0, 1), (3, 8), 60),
            qn((0, 1), (1, 4), 48),
        ];
        let v = separate(
            notes,
            |_, _| None,
            quarter,
            false,
            true,
            &whole_bars(),
            &|_| false,
        );
        let with_48 = v
            .iter()
            .flatten()
            .find(|e| e.notes.iter().any(|n| n.0 == 48))
            .unwrap();
        assert_eq!(with_48.off, Frac::new(1, 4));
    }

    #[test]
    fn a_grace_inside_a_triplet_keeps_the_bracket() {
        let bars = Bars {
            changes: vec![(Frac::from_integer(0), Frac::from_integer(1))],
            ..Bars::default()
        };
        let spell = |k: u8, _| crate::adapters::midi_to_ir::midi_key_to_pitch(k, true);
        let w = Writer {
            lane: 1,
            staff: 1,
            bars: &bars,
            main: true,
            spell: &spell,
        };
        let ev = |on: i64, key: u8, graces: Vec<(u8, u8, Frac)>| Event {
            on: Frac::new(on, 12),
            off: Frac::new(on + 1, 12),
            notes: vec![(key, 90, None)],
            grid: 3,
            graces,
            staccato: false,
            dynamic: None,
            sounding: Frac::new(1, 12),
        };
        let voice = vec![
            ev(0, 60, vec![]),
            ev(1, 64, vec![(62, 90, Frac::new(1, 16))]),
            ev(2, 65, vec![]),
        ];
        let out = w.elements(&voice);
        let marks: Vec<Option<StartStop>> = out
            .iter()
            .filter_map(|(_, e)| match e {
                VoiceElement::Note(n) if !n.is_grace => {
                    Some(n.tuplet.as_ref().map(|t| t.tuplet_type))
                }
                _ => None,
            })
            .collect();
        assert_eq!(marks, [Some(StartStop::Start), None, Some(StartStop::Stop)]);
    }

    #[test]
    fn a_second_voice_rests_only_in_bars_it_sings_in() {
        let bars = Bars {
            changes: vec![(Frac::from_integer(0), Frac::from_integer(1))],
            ..Bars::default()
        };
        let spell = |k: u8, _| crate::adapters::midi_to_ir::midi_key_to_pitch(k, true);
        let w = Writer {
            lane: 2,
            staff: 1,
            bars: &bars,
            main: false,
            spell: &spell,
        };
        let ev = |on: i64, off: i64| Event {
            on: Frac::from_integer(on),
            off: Frac::from_integer(off),
            notes: vec![(48, 90, None)],
            grid: 1,
            graces: vec![],
            staccato: false,
            dynamic: None,
            sounding: Frac::from_integer(1),
        };
        let out = w.elements(&[ev(0, 1), ev(3, 4)]);
        let rests = out
            .iter()
            .filter(|(_, e)| matches!(e, VoiceElement::Rest(_)))
            .count();
        assert_eq!(rests, 0, "{out:?}");
    }
}
