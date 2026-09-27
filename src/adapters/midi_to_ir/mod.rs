//! MIDI → IR adapter.
//!
//! Reads a Standard MIDI File with `midly` and rebuilds notation from it —
//! for MIDI exported by notation programs (LilyPond, MuseScore, lytk) first.
//! The pipeline is MuseScore's, simplified:
//!
//! 1. **Tracks**: a staff per track (per channel when a track mixes them);
//!    two consecutive keyboard tracks are one two-staff part (MuseScore's
//!    grand-staff rule, which also joins LilyPond's `upper:`/`lower:`
//!    tracks); channel 10 is percussion. Time and key signatures and tempos
//!    are read from every track. A first time signature shorter than the next
//!    is a pickup (how MuseScore and lytk write one).
//! 2. **Quantize** absolute positions, not durations, one grid per beat,
//!    tuplets included ([`quantize`]); LilyPond's grace notes are found by
//!    their timing.
//! 3. **Chords and voices** ([`voices`]); a note held short of the next is
//!    held to it (staccato when it was much shorter).
//! 4. **Notation**: every part becomes a positioned timeline, cut into bars
//!    by the crate's one bar-splitter (`crate::ir::timeline`), with notes
//!    tied across bar lines.
//!
//! Each note keeps its velocity (`Note.velocity`); a dynamic mark is added
//! where a part's level changes (LilyPond's table for files LilyPond or lytk
//! wrote). The sustain pedal (CC64) becomes pedal marks; lyric events become
//! lyrics.

mod beats;
mod quantize;
mod voices;

use std::collections::BTreeMap;
use std::path::Path;

use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use super::dynamics_velocity::{lilypond_dynamic, velocity_to_dynamic};
use super::{AdapterError, Result, ToIrAdapter};
use crate::ir::articulation::Placement;
use crate::ir::beams::post_process_beams_and_stems;
use crate::ir::direction::{Direction, PedalEvent, TempoDirection, TextDirection};
use crate::ir::duration::Frac;
use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, TimeSignature};
use crate::ir::music::MusicDocument;
use crate::ir::part::Part;
use crate::ir::pitch::{respell, Alter, Pitch, PitchStep};
use crate::ir::score::{Score, ScoreChild, ScoreMetadata};
use crate::ir::timeline::{split_tied, Event, Grid, Timeline, OFFSET_DIVISIONS};

use quantize::{Bars, Quantizer};
use voices::{separate, Writer};

/// The longest a file may run, in bars of its shortest meter and in whole
/// notes: a corrupt far event would otherwise have the grid (or one note's
/// tied values) exhaust memory.
const MAX_BARS: i64 = 100_000;
const MAX_WHOLES: i64 = 100_000;
/// The most bars all notes together may hold (each bar a note crosses is a
/// written note): a small file of long chords would otherwise expand into
/// millions of notes.
const MAX_HELD_BARS: i64 = 500_000;

// ---------------------------------------------------------------------------
// Public adapter
// ---------------------------------------------------------------------------

/// Adapter that reads MIDI files and produces an IR [`Score`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MidiToIrAdapter {
    /// The shortest plain value a played file is quantized to, as a note
    /// denominator (16: sixteenths); `None` lets each beat choose, to 32nds.
    shortest: Option<u32>,
    /// Straighten swung eighths: `None` decides from the playing (played
    /// files only).
    swing: Option<bool>,
}

/// (Notation files never swing unless asked: a DAW's swing-quantized
/// eighths at 60 % or 70 % lie on 5- or 10-tuplet points exactly and read as
/// such; `swing=True` straightens them.)
/// A played file swings when its staves swing 4 beats or more, three times
/// as many as they play straight, with the off-beat at under 72 % of the
/// beat in the middle one (3:2 to 2:1 swing; a file around 75 % plays dotted
/// rhythms, or a shuffle, which is written dotted too — unless asked).
const MIN_SWUNG_BEATS: usize = 4;
const SWING_MAX_MEDIAN: (i64, i64) = (72, 100);

impl MidiToIrAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Quantize played (not exported) music no finer than `1/denominator`
    /// notes (4, 8, 16 or 32; others count as the power of two below);
    /// triplets are still found.
    pub fn with_quantize(mut self, denominator: Option<u32>) -> Self {
        self.shortest = denominator;
        self
    }

    /// Read swung eighths as straight ones, marked "Swing" (`Some(true)`),
    /// never (`Some(false)`), or when a played file swings (`None`).
    pub fn with_swing(mut self, swing: Option<bool>) -> Self {
        self.swing = swing;
        self
    }

    /// Parse raw MIDI bytes into a [`Score`].
    pub fn convert_bytes(&self, bytes: &[u8]) -> Result<Score> {
        let smf = Smf::parse(bytes).map_err(|e| AdapterError::Parse(e.to_string()))?;
        let ppq = match smf.header.timing {
            Timing::Metrical(tpb) => u32::from(tpb.as_int()),
            // SMPTE: frames × subframes a second, taken as ticks a quarter.
            Timing::Timecode(fps, sub) => (fps.as_int() as u32) * (sub as u32),
        }
        // A corrupt header can declare 0 ticks a quarter.
        .max(1);
        if smf.header.format == Format::Sequential {
            return Err(AdapterError::Unsupported(
                "MIDI format 2 (independent sequences) is not supported".into(),
            ));
        }
        read(&smf.tracks, ppq, *self, true)
    }
}

impl ToIrAdapter for MidiToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score> {
        let bytes = std::fs::read(path)?;
        self.convert_bytes(&bytes)
    }

    fn convert_str(&self, _text: &str) -> Result<Score> {
        Err(AdapterError::Unsupported(
            "MIDI is binary; use convert_file() or convert_bytes()".into(),
        ))
    }
}

impl super::ToMusicAdapter for MidiToIrAdapter {
    fn convert_file_to_music(&self, path: &Path) -> Result<MusicDocument> {
        let score = self.convert_file(path)?;
        Ok(crate::ir::lift::lift_to_music(&score))
    }

    fn convert_str_to_music(&self, _text: &str) -> Result<MusicDocument> {
        Err(AdapterError::Unsupported(
            "MIDI is binary; use convert_file_to_music()".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Track events
// ---------------------------------------------------------------------------

/// A note-on/note-off pair with absolute tick timing.
#[derive(Debug)]
struct RawNote {
    start_tick: u64,
    end_tick: u64,
    midi_key: u8,
    velocity: u8,
    channel: u8,
}

/// Everything but notes, from one track.
#[derive(Debug, Default)]
struct TrackMeta {
    name: String,
    /// (tick, microseconds a quarter)
    tempo_changes: Vec<(u64, u32)>,
    /// (tick, numerator, denominator power)
    time_sig_changes: Vec<(u64, u8, u8)>,
    /// (tick, fifths, minor)
    key_sig_changes: Vec<(u64, i8, bool)>,
    /// (tick, channel, program)
    program_changes: Vec<(u64, u8, u8)>,
    /// (tick, text)
    lyrics: Vec<(u64, String)>,
    /// (tick, channel, pedal down): CC64.
    pedal: Vec<(u64, u8, bool)>,
    /// (tick, text) of text events: a karaoke file's lyrics.
    texts: Vec<(u64, String)>,
    /// Written by LilyPond or lytk (a text event says so): velocities follow
    /// LilyPond's dynamics table rather than the common one.
    lilypond: bool,
}

/// Collect note pairs and meta events from one track.
fn collect_track_events(events: &[midly::TrackEvent<'_>]) -> (Vec<RawNote>, TrackMeta) {
    let mut notes: Vec<RawNote> = Vec::new();
    let mut meta = TrackMeta::default();
    // Open notes per (key, channel), oldest first: overlapping unisons are
    // real (two voices on one key), and FIFO pairing closes the oldest.
    let mut pending: BTreeMap<(u8, u8), Vec<(u64, u8)>> = BTreeMap::new();
    let mut tick: u64 = 0;
    let mut close = |pending: &mut BTreeMap<(u8, u8), Vec<(u64, u8)>>, k, ch, tick| {
        if let Some(open) = pending.get_mut(&(k, ch)) {
            if !open.is_empty() {
                let (start, velocity) = open.remove(0);
                notes.push(RawNote {
                    start_tick: start,
                    end_tick: tick,
                    midi_key: k,
                    velocity,
                    channel: ch,
                });
            }
        }
    };
    for event in events {
        tick += u64::from(event.delta.as_int());
        match event.kind {
            TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();
                match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        pending
                            .entry((key.as_int(), ch))
                            .or_default()
                            .push((tick, vel.as_int()));
                    }
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        close(&mut pending, key.as_int(), ch, tick);
                    }
                    MidiMessage::ProgramChange { program } => {
                        meta.program_changes.push((tick, ch, program.as_int()));
                    }
                    MidiMessage::Controller { controller, value } if controller.as_int() == 64 => {
                        meta.pedal.push((tick, ch, value.as_int() >= 64));
                    }
                    _ => {}
                }
            }
            TrackEventKind::Meta(msg) => match msg {
                MetaMessage::TrackName(name) => {
                    // MuseScore ends its names with a NUL.
                    meta.name = String::from_utf8_lossy(name)
                        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
                        .to_string();
                }
                MetaMessage::Tempo(uspq) => meta.tempo_changes.push((tick, uspq.as_int())),
                MetaMessage::TimeSignature(num, den_pow, _, _) => {
                    meta.time_sig_changes.push((tick, num, den_pow));
                }
                MetaMessage::KeySignature(sf, minor) => {
                    meta.key_sig_changes.push((tick, sf, minor));
                }
                MetaMessage::Lyric(t) => {
                    let t = String::from_utf8_lossy(t).trim().to_string();
                    if !t.is_empty() {
                        meta.lyrics.push((tick, t));
                    }
                }
                MetaMessage::Text(t) | MetaMessage::Copyright(t) => {
                    let t = String::from_utf8_lossy(t);
                    if t.contains("LilyPond") || t.starts_with("creator: lytk") {
                        meta.lilypond = true;
                    }
                    if matches!(event.kind, TrackEventKind::Meta(MetaMessage::Text(_))) {
                        meta.texts.push((tick, t.into_owned()));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    (notes, meta)
}

// ---------------------------------------------------------------------------
// Staves and parts
// ---------------------------------------------------------------------------

/// The notes of one track and channel.
struct Staff {
    name: String,
    /// LilyPond names a staff's track `staff:`.
    lilypond_staff: bool,
    channel: u8,
    program: Option<u8>,
    notes: Vec<RawNote>,
    lyrics: Vec<(u64, String)>,
    /// (tick, pedal down)
    pedal: Vec<(u64, bool)>,
}

impl Staff {
    fn percussion(&self) -> bool {
        self.channel == 9
    }

    /// A piano, organ or other keyboard (no program: General MIDI's piano).
    fn keyboard(&self) -> bool {
        !self.percussion() && self.program.is_none_or(|p| p < 8)
    }

    fn mean_key(&self) -> Frac {
        let sum: i64 = self.notes.iter().map(|n| n.midi_key as i64).sum();
        Frac::new(sum, self.notes.len().max(1) as i64)
    }

    fn clef(&self) -> Clef {
        if self.percussion() {
            Clef {
                sign: ClefSign::Percussion,
                line: 3,
                octave_change: 0,
            }
        } else if self.mean_key() < Frac::from_integer(60) {
            Clef {
                sign: ClefSign::F,
                line: 4,
                octave_change: 0,
            }
        } else {
            Clef::default()
        }
    }
}

/// `upper:` / `staff:voice` (LilyPond's track names) → `upper`.
fn staff_name(track: &str) -> &str {
    track.split(':').next().unwrap_or(track).trim()
}

/// One staff per track and channel; LilyPond's `staff:voice` tracks of one
/// staff join. Returns the staves and the file's metadata (score-wide
/// signatures come back through `meta`).
fn staves(collected: Vec<(Vec<RawNote>, TrackMeta)>) -> Vec<Staff> {
    let mut out: Vec<Staff> = Vec::new();
    for (i, (notes, meta)) in collected.into_iter().enumerate() {
        let mut by_channel: BTreeMap<u8, Vec<RawNote>> = BTreeMap::new();
        for n in notes {
            by_channel.entry(n.channel).or_default().push(n);
        }
        let mut lyrics = Some(meta.lyrics);
        let lilypond_staff = meta.name.contains(':');
        for (channel, notes) in by_channel {
            let name = staff_name(&meta.name).to_string();
            let pedal: Vec<(u64, bool)> = meta
                .pedal
                .iter()
                .filter(|p| p.1 == channel)
                .map(|&(t, _, down)| (t, down))
                .collect();
            // Only LilyPond's `staff:voice` tracks of one staff join; other
            // writers (MuseScore) name both staves of a piano after the part.
            if let Some(prev) = out.last_mut() {
                let same = !name.is_empty() && prev.name == name && prev.channel == channel;
                if same && lilypond_staff && prev.lilypond_staff {
                    prev.notes.extend(notes);
                    prev.lyrics.extend(lyrics.take().unwrap_or_default());
                    prev.pedal.extend(pedal);
                    prev.pedal.sort_by_key(|p| p.0);
                    continue;
                }
            }
            let program = meta
                .program_changes
                .iter()
                .rev()
                .find_map(|&(_, c, p)| (c == channel).then_some(p));
            out.push(Staff {
                lilypond_staff,
                name: if name.is_empty() {
                    format!("Track {}", i + 1)
                } else {
                    name
                },
                channel,
                program,
                notes,
                lyrics: lyrics.take().unwrap_or_default(),
                pedal,
            });
        }
    }
    out
}

/// Whether two consecutive staves are one keyboard's two hands, and then the
/// part's name. Both must play a keyboard program, the first higher, and the
/// names must say so: lytk's `Piano 1`/`Piano 2`, LilyPond's `upper:`/
/// `lower:`, or a keyboard or a hand in them (a violin and a cello written
/// without instruments play the piano program too).
fn grand_staff(a: &Staff, b: &Staff) -> Option<String> {
    // MuseScore gives the program to the top staff only.
    let same = a.program == b.program || b.program.is_none();
    if !(a.keyboard() && b.keyboard() && same && a.mean_key() >= b.mean_key()) {
        return None;
    }
    let (x, y) = (a.name.as_str(), b.name.as_str());
    if let (Some(base), Some(other)) = (x.strip_suffix(" 1"), y.strip_suffix(" 2")) {
        if base == other {
            return Some(base.to_string());
        }
    }
    const WORDS: [&str; 16] = [
        "piano",
        "pno",
        "pf",
        "klavier",
        "keyboard",
        "organ",
        "orgel",
        "harpsichord",
        "cembalo",
        "celesta",
        "rh",
        "lh",
        "right",
        "left",
        "upper",
        "lower",
    ];
    let says = |s: &str| {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| WORDS.contains(&w))
    };
    ((a.lilypond_staff && b.lilypond_staff) || (says(x) && says(y))).then(|| x.to_string())
}

/// Staves into parts: a keyboard's two hands are one grand staff — two
/// tracks, or, in a played file (`split`), one track both hands played.
fn parts(staves: Vec<Staff>, split: Option<u64>) -> Vec<(String, Vec<Staff>)> {
    let mut out = Vec::new();
    let mut it = staves.into_iter().peekable();
    while let Some(s) = it.next() {
        match it.peek().and_then(|n| grand_staff(&s, n)) {
            Some(name) => {
                let lower = it.next().expect("peeked");
                out.push((name, vec![s, lower]));
            }
            None if split.is_some_and(|tol| both_hands(&s, tol)) => {
                let name = s.name.clone();
                out.push((name, split_hands(s, split.unwrap_or(0))));
            }
            None => out.push((s.name.clone(), vec![s])),
        }
    }
    out
}

/// A keyboard track both hands played, over two octaves or more: a fifth of
/// its notes or more on each side of middle C, or (MuseScore's rule) notes
/// struck together (within `tol`) more than an octave apart.
fn both_hands(s: &Staff, tol: u64) -> bool {
    let n = s.notes.len();
    let high = s.notes.iter().filter(|x| x.midi_key >= 60).count();
    let (lo, hi) = s.notes.iter().fold((u8::MAX, 0), |(a, b), x| {
        (a.min(x.midi_key), b.max(x.midi_key))
    });
    // Chords as `left_hand` makes them: notes within `tol` of the first.
    let spread = || {
        let mut on: Vec<(u64, u8)> = s.notes.iter().map(|x| (x.start_tick, x.midi_key)).collect();
        on.sort_unstable();
        let (mut first, mut lo, mut hi) = (0u64, u8::MAX, 0u8);
        on.iter().any(|&(t, k)| {
            if lo == u8::MAX || t > first + tol {
                (first, lo, hi) = (t, k, k);
            }
            (lo, hi) = (lo.min(k), hi.max(k));
            hi > lo + 12
        })
    };
    s.keyboard() && n >= 8 && hi >= lo + 24 && ((high * 5 >= n && (n - high) * 5 >= n) || spread())
}

/// The two hands, right (with the words) above left (with the pedal): one
/// staff when all the notes went to one hand.
fn split_hands(s: Staff, tol: u64) -> Vec<Staff> {
    let left = left_hand(&s.notes, tol);
    if left.iter().all(|&l| l) || !left.iter().any(|&l| l) {
        return vec![s];
    }
    let (mut rh, mut lh) = (Vec::new(), Vec::new());
    for (n, l) in s.notes.into_iter().zip(left) {
        if l {
            lh.push(n)
        } else {
            rh.push(n)
        }
    }
    vec![
        Staff {
            name: s.name.clone(),
            lilypond_staff: false,
            channel: s.channel,
            program: s.program,
            notes: rh,
            lyrics: s.lyrics,
            pedal: Vec::new(),
        },
        Staff {
            name: s.name,
            lilypond_staff: false,
            channel: s.channel,
            program: s.program,
            notes: lh,
            lyrics: Vec::new(),
            pedal: s.pedal,
        },
    ]
}

/// Which notes the left hand plays, by MuseScore's cost model (reimplemented
/// from its description): each chord (notes starting within `tol` of its
/// first) is cut in two, its lowest notes to the left hand, and a Viterbi
/// search over the cuts finds the cheapest sequence. A hand wider than an
/// octave costs 20, than a ninth 100; a hand whose notes end together saves
/// 10; a thinner left hand costs 5, a left hand alone with a chord 10; a
/// hand keeping its texture (octaves 12, a chord 5, one note 12) saves; a
/// note given to a hand still sounding costs 10. lytk adds what MuseScore's
/// model can't see: a hand moving costs a point a semitone its centre moves
/// from where it last played (octaves climbing past a held right-hand note
/// stay in the left hand; the hands start at C3 and C5, so hands that never
/// strike together keep their registers), and a hand's span counts the keys
/// it still holds (no C4 under a held E5).
// ponytail: a busy hand costs 10 once, where MuseScore counts every chord
// still sounding in it; and a state keeps only its cheapest path's hands
// (sounding, holding, where), so the search is close to, not exactly, the
// cheapest assignment (as MuseScore's).
fn left_hand(notes: &[RawNote], tol: u64) -> Vec<bool> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (notes[i].start_tick, notes[i].midi_key));
    let mut chords: Vec<std::ops::Range<usize>> = Vec::new();
    for k in 0..order.len() {
        match chords.last_mut() {
            Some(c) if notes[order[k]].start_tick <= notes[order[c.start]].start_tick + tol => {
                c.end = k + 1
            }
            _ => chords.push(k..k + 1),
        }
    }
    // A chord's notes low to high: a cut gives the left hand the lowest.
    for c in &chords {
        order[c.clone()].sort_by_key(|&i| notes[i].midi_key);
    }

    /// One cut of one chord, and along the cheapest path to it: until when
    /// each hand sounds, the range of keys it holds meanwhile, and where it
    /// last played (its centre key).
    #[derive(Clone)]
    struct State {
        cost: i64,
        back: usize,
        cut: usize,
        /// Keys each hand holds, and until when.
        lh_held: Vec<(u8, u64)>,
        rh_held: Vec<(u8, u64)>,
        lh_at: i64,
        rh_at: i64,
    }
    let span = |width: i64| match width {
        ..=12 => 0,
        13..=14 => 20,
        _ => 100,
    };
    let similar = |a: (usize, bool), b: (usize, bool)| -> i64 {
        match (a, b) {
            ((0, _), _) | (_, (0, _)) => 0,
            ((_, true), (_, true)) => -12,
            ((1, _), (1, _)) => -12,
            ((x, _), (y, _)) if x >= 2 && y >= 2 => -5,
            _ => 0,
        }
    };
    /// A hand's part of a chord: (notes, is an octave).
    fn shape(keys: &[u8]) -> (usize, bool) {
        (keys.len(), keys.len() == 2 && keys[1] == keys[0] + 12)
    }

    // Before the first chord: nothing sounds; the hands wait where hands
    // play (C3 and C5).
    let start = State {
        cost: 0,
        back: 0,
        cut: 0,
        lh_held: Vec::new(),
        rh_held: Vec::new(),
        lh_at: 48,
        rh_at: 72,
    };
    let mut table: Vec<Vec<State>> = Vec::with_capacity(chords.len());
    let mut prev_keys: Vec<u8> = Vec::new();
    for c in &chords {
        let keys: Vec<u8> = order[c.clone()]
            .iter()
            .map(|&i| notes[i].midi_key)
            .collect();
        let ends: Vec<u64> = order[c.clone()]
            .iter()
            .map(|&i| notes[i].end_tick)
            .collect();
        let on = notes[order[c.start]].start_tick;
        let n = keys.len();
        // Prefix sums and end ranges, so a cut costs O(1).
        let mut sum = vec![0i64; n + 1];
        let mut pre = vec![(u64::MAX, 0u64); n + 1];
        let mut suf = vec![(u64::MAX, 0u64); n + 1];
        for i in 0..n {
            sum[i + 1] = sum[i] + i64::from(keys[i]);
            pre[i + 1] = (pre[i].0.min(ends[i]), pre[i].1.max(ends[i]));
            let j = n - 1 - i;
            suf[j] = (suf[j + 1].0.min(ends[j]), suf[j + 1].1.max(ends[j]));
        }
        let together = |(lo, hi): (u64, u64)| lo != u64::MAX && hi - lo <= tol;
        // ponytail: more notes than two hands can hold are cut at their
        // widest gap, without a search (a cluster or a corrupt file).
        let cuts: Vec<usize> = if n > 24 {
            let widest = (1..n).max_by_key(|&i| keys[i] - keys[i - 1]).unwrap_or(0);
            vec![widest]
        } else {
            (0..=n).collect()
        };
        let prev = table
            .last()
            .map_or(std::slice::from_ref(&start), |p| p.as_slice());
        let states: Vec<State> = cuts
            .into_iter()
            .map(|cut| {
                let (lk, rk) = keys.split_at(cut);
                let lw = lk.last().map_or(0, |&t| i64::from(t - lk[0]));
                let rw = rk.last().map_or(0, |&t| i64::from(t - rk[0]));
                let mut local = span(lw) + span(rw);
                local -= 10 * (i64::from(together(pre[cut])) + i64::from(together(suf[cut])));
                if !lk.is_empty() && !rk.is_empty() && lk.len() < rk.len() {
                    local += 5;
                }
                if rk.is_empty() && lk.len() >= 2 {
                    local += 10;
                }
                let lh_now = (cut > 0).then(|| sum[cut] / cut as i64);
                let rh_now = (cut < n).then(|| (sum[n] - sum[cut]) / (n - cut) as i64);
                // A hand given notes: 10 if it still sounds, the reach from
                // what it holds to them, how far it moves.
                let (le, re) = ends.split_at(cut);
                // A hand given notes: 10 if it still sounds, the reach from
                // the keys it still holds to them, how far it moves; it then
                // holds what still sounds and the new notes.
                let hand =
                    |held: &[(u8, u64)], at: i64, part: &[u8], until: &[u64], now: Option<i64>| {
                        let mut holds: Vec<(u8, u64)> =
                            held.iter().copied().filter(|&(_, end)| end > on).collect();
                        let Some(now) = now else {
                            return (0, holds);
                        };
                        let (lo, hi) = (part[0], part[part.len() - 1]);
                        let mut cost = (at - now).abs();
                        if !holds.is_empty() {
                            let a = holds.iter().map(|h| h.0).min().unwrap_or(lo).min(lo);
                            let b = holds.iter().map(|h| h.0).max().unwrap_or(hi).max(hi);
                            cost += 10 + span(i64::from(b - a)) - span(i64::from(hi - lo));
                        }
                        holds.extend(part.iter().copied().zip(until.iter().copied()));
                        (cost, holds)
                    };
                let (p, best, lh_held, rh_held) = prev
                    .iter()
                    .enumerate()
                    .map(|(p, st)| {
                        let (pl, pr) = prev_keys.split_at(st.cut);
                        let (lc, lh_held) = hand(&st.lh_held, st.lh_at, lk, le, lh_now);
                        let (rc, rh_held) = hand(&st.rh_held, st.rh_at, rk, re, rh_now);
                        let cost = st.cost
                            + similar(shape(pl), shape(lk))
                            + similar(shape(pr), shape(rk))
                            + lc
                            + rc;
                        (p, cost, lh_held, rh_held)
                    })
                    .fold((0, i64::MAX, Vec::new(), Vec::new()), |acc, x| {
                        if x.1 < acc.1 {
                            x
                        } else {
                            acc
                        }
                    });
                State {
                    cost: best + local,
                    back: p,
                    cut,
                    lh_held,
                    rh_held,
                    lh_at: lh_now.unwrap_or(prev[p].lh_at),
                    rh_at: rh_now.unwrap_or(prev[p].rh_at),
                }
            })
            .collect();
        table.push(states);
        prev_keys = keys;
    }

    let mut left = vec![false; notes.len()];
    let Some(last) = table.last() else {
        return left;
    };
    let mut k = (0..last.len()).fold(0, |b, s| if last[s].cost < last[b].cost { s } else { b });
    for (c, states) in chords.iter().zip(&table).rev() {
        let st = &states[k];
        for &i in &order[c.start..c.start + st.cut] {
            left[i] = true;
        }
        k = st.back;
    }
    left
}

// ---------------------------------------------------------------------------
// The score
// ---------------------------------------------------------------------------

fn make_time_sig(num: u8, den_pow: u8) -> TimeSignature {
    TimeSignature {
        beats: num.to_string(),
        // A real denominator power is ≤ 7 (a 128th-note beat).
        beat_type: 1u8 << den_pow.min(7),
        symbol: None,
    }
}

fn make_key_sig(fifths: i8, minor: bool) -> KeySignature {
    KeySignature {
        fifths: fifths.clamp(-7, 7),
        mode: if minor {
            KeyMode::Minor
        } else {
            KeyMode::Major
        },
    }
}

/// Beats a minute from microseconds a quarter, rounded where a writer
/// truncated a whole number (LilyPond's 99 is 606,060 µs: 99.0001).
fn bpm(uspq: u32) -> f64 {
    let b = 60_000_000.0 / uspq.max(1) as f64;
    if (b - b.round()).abs() < 0.01 {
        b.round()
    } else {
        (b * 100.0).round() / 100.0
    }
}

/// Karaoke text events as lyrics: `@` lines are headers, `/` starts a line
/// and `\\` a paragraph; a syllable is continued by the next unless that one
/// starts a word (a leading space, or a new line).
fn karaoke_lyrics(texts: &[(u64, String)]) -> Vec<(u64, String)> {
    let sung: Vec<(u64, &str)> = texts
        .iter()
        .filter(|(_, t)| !t.starts_with('@'))
        .map(|(tick, t)| (*tick, t.as_str()))
        .collect();
    let starts_word = |t: &str| t.starts_with([' ', '/', '\\']);
    sung.iter()
        .enumerate()
        .filter_map(|(k, (tick, t))| {
            let word = t.trim_start_matches(['/', '\\']).trim();
            let continued =
                sung.get(k + 1).is_some_and(|(_, next)| !starts_word(next)) && !t.ends_with(' ');
            (!word.is_empty()).then(|| {
                (
                    *tick,
                    if continued {
                        format!("{word}-")
                    } else {
                        word.to_string()
                    },
                )
            })
        })
        .collect()
}

/// The key of some notes, as (fifths, minor): the major or minor key whose
/// Krumhansl–Kessler profile best matches how long each pitch class sounds.
/// `None` for too few notes to tell.
fn detect_key<'a>(staves: impl Iterator<Item = &'a Staff>) -> Option<(i8, bool)> {
    const MAJOR: [f64; 12] = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const MINOR: [f64; 12] = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let mut weight = [0.0f64; 12];
    let mut count = 0;
    for s in staves {
        for n in &s.notes {
            weight[(n.midi_key % 12) as usize] += (n.end_tick - n.start_tick).max(1) as f64;
            count += 1;
        }
    }
    if count < 8 {
        return None;
    }
    let corr = |profile: &[f64; 12], tonic: usize| {
        let x: Vec<f64> = (0..12).map(|i| weight[(tonic + i) % 12]).collect();
        let (mx, my) = (
            x.iter().sum::<f64>() / 12.0,
            profile.iter().sum::<f64>() / 12.0,
        );
        let cov: f64 = x
            .iter()
            .zip(profile)
            .map(|(a, b)| (a - mx) * (b - my))
            .sum();
        let vx: f64 = x.iter().map(|a| (a - mx).powi(2)).sum();
        let vy: f64 = profile.iter().map(|b| (b - my).powi(2)).sum();
        cov / (vx * vy).sqrt().max(f64::EPSILON)
    };
    let (tonic, minor, _) = (0..12)
        .flat_map(|t| [(t, false, corr(&MAJOR, t)), (t, true, corr(&MINOR, t))])
        .max_by(|a, b| a.2.total_cmp(&b.2))?;
    // The major key with that signature (a minor key's relative major is a
    // minor third up), on the circle of fifths from -5 (D♭) to 6 (F♯).
    let major = if minor { (tonic + 3) % 12 } else { tonic };
    let fifths = ((major * 7) % 12) as i8;
    Some((if fifths > 6 { fifths - 12 } else { fifths }, minor))
}

/// The last value at or before `p` in a position-ordered list.
fn at<T: Copy>(list: &[(Frac, T)], p: Frac) -> Option<T> {
    list.iter().take_while(|(q, _)| *q <= p).last().map(|x| x.1)
}

/// A played file whose beat drifts from its tempo (`follow`) is read again
/// with every event moved onto the tracked beats.
fn read(
    tracks: &[Vec<midly::TrackEvent<'_>>],
    ppq: u32,
    opts: MidiToIrAdapter,
    follow: bool,
) -> Result<Score> {
    let whole = 4 * ppq as i64;
    let pos = |t: u64| Frac::new(t as i64, whole);
    let zero = Frac::from_integer(0);
    let collected: Vec<(Vec<RawNote>, TrackMeta)> =
        tracks.iter().map(|t| collect_track_events(t)).collect();

    // Score-wide: signatures and tempos from every track (the last one at a
    // tick wins), the title from the first track, who wrote it.
    let mut times: BTreeMap<u64, (u8, u8)> = BTreeMap::new();
    let mut keys: BTreeMap<u64, (i8, bool)> = BTreeMap::new();
    let mut tempos: BTreeMap<u64, u32> = BTreeMap::new();
    for (_, m) in &collected {
        times.extend(
            m.time_sig_changes
                .iter()
                .filter(|t| t.1 > 0)
                .map(|&(t, n, d)| (t, (n, d))),
        );
        keys.extend(m.key_sig_changes.iter().map(|&(t, f, mi)| (t, (f, mi))));
        tempos.extend(m.tempo_changes.iter().copied());
    }
    let lilypond = collected.iter().any(|(_, m)| m.lilypond);
    // A karaoke file (`@` headers in its text events) sings its text events.
    let mut collected = collected;
    if collected
        .iter()
        .any(|(_, m)| m.texts.iter().any(|t| t.1.starts_with('@')))
    {
        for (_, m) in collected.iter_mut().filter(|(_, m)| m.lyrics.is_empty()) {
            m.lyrics = karaoke_lyrics(&m.texts);
        }
    }
    let mut score = Score::new();
    score.metadata = ScoreMetadata::default();
    if let Some((_, m)) = collected.first() {
        if !m.name.is_empty() && collected.len() > 1 {
            score.metadata.title = Some(m.name.clone());
        }
    }

    let staves = staves(collected);
    if staves.iter().all(|s| s.notes.is_empty()) {
        return Ok(score);
    }
    // Everything placed in the score, not only notes, ends within the limit
    // (checked before any position is computed).
    let last_tick = staves
        .iter()
        .flat_map(|s| {
            s.notes
                .iter()
                .map(|n| n.end_tick)
                .chain(s.pedal.iter().map(|p| p.0))
                .chain(s.lyrics.iter().map(|l| l.0))
        })
        .chain(times.keys().copied())
        .chain(keys.keys().copied())
        .chain(tempos.keys().copied())
        .max()
        .unwrap_or(0);
    let too_long = || {
        AdapterError::Parse(format!(
            "MIDI file too long: more than {MAX_BARS} bars, {MAX_WHOLES} whole notes \
             or {MAX_HELD_BARS} bars of held notes"
        ))
    };
    if last_tick as u128 > (MAX_WHOLES as u128) * (whole as u128) {
        return Err(too_long());
    }

    // Bars. A pickup is written as a short first time signature, the meter
    // following it one bar later.
    let len = |n: u8, d: u8| Frac::new(n as i64, 1i64 << d.min(7));
    let mut signs: Vec<(Frac, TimeSignature, Frac)> = times
        .iter()
        .map(|(&t, &(n, d))| (pos(t), make_time_sig(n, d), len(n, d)))
        .collect();
    if signs.is_empty() || signs[0].0 > zero {
        signs.insert(0, (zero, TimeSignature::default(), Frac::from_integer(1)));
    }
    let pickup = (signs.len() > 1 && signs[1].0 == signs[0].2 && signs[1].2 > signs[0].2)
        .then(|| signs[0].2);
    let mut changes: Vec<(Frac, Frac)> = Vec::new();
    let mut time_events: Vec<(Frac, TimeSignature)> = Vec::new();
    // Each change's meter (a pickup takes the one after it).
    let meters: Vec<(u8, u8)> = (0..signs.len())
        .map(|k| {
            let ts = &signs[if pickup.is_some() && k == 0 { 1 } else { k }].1;
            (ts.beats.parse().unwrap_or(4), ts.beat_type)
        })
        .collect();
    for (k, (p, ts, l)) in signs.iter().enumerate() {
        match pickup {
            Some(pk) if k == 0 => changes.push((zero, pk)),
            Some(_) if k == 1 => {
                changes.push((*p, *l));
                time_events.push((zero, ts.clone()));
            }
            _ => {
                changes.push((*p, *l));
                time_events.push((*p, ts.clone()));
            }
        }
    }
    let bars = Bars { changes, meters };
    let end = pos(last_tick);
    let shortest = bars
        .changes
        .iter()
        .map(|c| c.1)
        .min()
        .unwrap_or(Frac::from_integer(1));
    if shortest <= zero || end / shortest > Frac::from_integer(MAX_BARS) {
        return Err(too_long());
    }
    // In u128 ticks: at most 2^32 notes of at most MAX_WHOLES each.
    let held: u128 = staves
        .iter()
        .flat_map(|s| &s.notes)
        .map(|n| u128::from(n.end_tick.saturating_sub(n.start_tick)))
        .sum();
    if Frac::new((held / whole as u128).min(i64::MAX as u128) as i64, 1) / shortest
        > Frac::from_integer(MAX_HELD_BARS)
    {
        return Err(too_long());
    }
    let mut key_events: Vec<(Frac, KeySignature)> = keys
        .iter()
        .map(|(&t, &(f, m))| (pos(t), make_key_sig(f, m)))
        .collect();
    // No key signature at all: the key the notes are in (C and A minor, the
    // default, need none).
    if key_events.is_empty() {
        if let Some((f, minor)) = detect_key(staves.iter().filter(|s| !s.percussion())) {
            if f != 0 {
                key_events.push((zero, make_key_sig(f, minor)));
            }
        }
    }
    let fifths: Vec<(Frac, i32)> = key_events
        .iter()
        .map(|(p, k)| (*p, k.fifths as i32))
        .collect();
    let spell =
        |key: u8, p: Frac| respell(midi_key_to_pitch(key, true), at(&fifths, p).unwrap_or(0));

    // Exported notation or a performance: most onsets exactly on a grid.
    let probe = Quantizer::new(&bars, whole, true);
    let onsets: Vec<Frac> = staves
        .iter()
        .flat_map(|s| s.notes.iter().map(|n| pos(n.start_tick)))
        .collect();
    // ponytail: the first 256 onsets decide; a file doesn't change its nature.
    let onsets = &onsets[..onsets.len().min(256)];
    let on_grid = onsets.iter().filter(|&&o| probe.is_exact(o)).count();
    // (Read again on its tracked beats, a performance stays one: its onsets
    // are on the beats now.)
    let mut qz = Quantizer::new(&bars, whole, follow && on_grid * 5 >= onsets.len() * 4);
    if follow && !qz.exported {
        let mut ticks: Vec<u64> = staves
            .iter()
            .flat_map(|s| s.notes.iter().map(|n| n.start_tick))
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        if let Some(beats) = beats::drifting_beats(&ticks, u64::from(ppq)) {
            // What was played moves onto the tracked beats; the file's own
            // grid — time and key signatures, tempos — stays where it is (the
            // tracked beat k is the file's beat k).
            let grid = |e: &midly::TrackEvent<'_>| {
                matches!(
                    e.kind,
                    TrackEventKind::Meta(
                        MetaMessage::TimeSignature(..)
                            | MetaMessage::KeySignature(..)
                            | MetaMessage::Tempo(..)
                            | MetaMessage::EndOfTrack
                    )
                )
            };
            let moved: Vec<Vec<midly::TrackEvent<'_>>> = tracks
                .iter()
                .map(|t| {
                    let mut at = 0u64;
                    let mut timed: Vec<(u64, midly::TrackEvent<'_>)> = t
                        .iter()
                        .map(|e| {
                            at += u64::from(u32::from(e.delta));
                            let now = if grid(e) {
                                at
                            } else {
                                beats::onto_grid(at, &beats, u64::from(ppq))
                            };
                            (now, *e)
                        })
                        .collect();
                    // (The end of the track after everything.)
                    let end = timed.iter().map(|x| x.0).max().unwrap_or(0);
                    for x in timed.iter_mut() {
                        if matches!(x.1.kind, TrackEventKind::Meta(MetaMessage::EndOfTrack)) {
                            x.0 = end;
                        }
                    }
                    timed.sort_by_key(|x| x.0);
                    let mut was = 0u64;
                    timed
                        .into_iter()
                        .map(|(now, e)| {
                            let delta =
                                (now - was).min(u64::from(u32::from(midly::num::u28::max_value())));
                            was = now;
                            midly::TrackEvent {
                                delta: midly::num::u28::new(delta as u32),
                                kind: e.kind,
                            }
                        })
                        .collect()
                })
                .collect();
            return read(&moved, ppq, opts, false);
        }
    }
    // Steps a quarter of the finest plain grid: a 16th is 4.
    // (A denominator between powers of two counts as the one below.)
    qz.finest = opts.shortest.map_or(8, |d| {
        let d = d.clamp(4, 32);
        i64::from((1u32 << (31 - d.leading_zeros())) / 4)
    });
    // Swing: each staff's beats vote.
    qz.swing = opts.swing.unwrap_or_else(|| {
        let (mut swung, mut straight) = (Vec::new(), 0);
        for s in &staves {
            let ticks: Vec<u64> = s.notes.iter().map(|n| n.start_tick).collect();
            let (sw, st) = qz.swing_votes(&ticks);
            swung.extend(sw);
            straight += st;
        }
        swung.sort_unstable();
        let median = swung.get(swung.len() / 2).copied().unwrap_or(zero);
        !qz.exported
            && swung.len() >= MIN_SWUNG_BEATS
            && swung.len() >= 3 * straight
            && median < Frac::new(SWING_MAX_MEDIAN.0, SWING_MAX_MEDIAN.1)
    });
    // Directions land on the nearest 64th.
    let snap = |t: u64| {
        let p = pos(t) * Frac::from_integer(64);
        p.round() / Frac::from_integer(64)
    };

    // Half a second in wholes at a position, by the tempo there.
    let half_second = |p: Frac| {
        let tick = (p * Frac::from_integer(whole)).to_integer().max(0) as u64;
        let uspq = tempos
            .range(..=tick)
            .next_back()
            .map_or(500_000, |(_, &u)| u);
        Frac::new(125_000, i64::from(uspq.max(1)))
    };

    // A played track's chords: notes starting within a 64th.
    let groups = parts(staves, (!qz.exported).then_some(u64::from(ppq) / 16));
    let mut timelines: Vec<Timeline> = Vec::new();
    for (pi, (_, group)) in groups.iter().enumerate() {
        // The part's sustain pedal (the instrument's, whichever staff has
        // it): down from each press to the next release.
        let mut pedal: Vec<(Frac, bool)> = group
            .iter()
            .flat_map(|st| st.pedal.iter().map(|&(t, down)| (pos(t), down)))
            .collect();
        pedal.sort();
        let sustained = |p: Frac| {
            let k = pedal.partition_point(|x| x.0 <= p);
            k > 0 && pedal[k - 1].1
        };
        // Voices of every staff, then the part's dynamics in time order.
        let mut staff_voices: Vec<Vec<Vec<voices::Event>>> = group
            .iter()
            .map(|st| {
                let raw: Vec<(u64, u64, u8, u8)> = st
                    .notes
                    .iter()
                    .map(|n| (n.start_tick, n.end_tick, n.midi_key, n.velocity))
                    .collect();
                // A lyric goes on the top note starting at its tick.
                let mut top: BTreeMap<u64, u8> = BTreeMap::new();
                if !st.lyrics.is_empty() {
                    for n in &st.notes {
                        let k = top.entry(n.start_tick).or_insert(n.midi_key);
                        *k = (*k).max(n.midi_key);
                    }
                }
                let lyric = |tick: u64, key: u8| {
                    (top.get(&tick) == Some(&key))
                        .then(|| st.lyrics.iter().find(|l| l.0 == tick).map(|l| l.1.clone()))
                        .flatten()
                };
                separate(
                    qz.quantize(&raw),
                    lyric,
                    half_second,
                    st.percussion(),
                    qz.exported,
                    &bars,
                    &sustained,
                )
            })
            .collect();
        // Dynamics: each voice keeps its own level, marked where it changes
        // (from the loudest note of an event, less a staccato's 4) unless
        // another voice of the staff shows the same mark there.
        for voices in staff_voices.iter_mut() {
            let mut level: Vec<Option<&'static str>> = vec![None; voices.len()];
            let mut events: Vec<(usize, &mut voices::Event)> = voices
                .iter_mut()
                .enumerate()
                .flat_map(|(vi, v)| v.iter_mut().map(move |e| (vi, e)))
                .collect();
            events.sort_by_key(|(vi, e)| (e.on, *vi));
            let mut shown: (Frac, Vec<&'static str>) = (Frac::from_integer(-1), Vec::new());
            for (vi, e) in events {
                let v = e.notes.iter().map(|n| n.1).max().unwrap_or(0);
                // (Only notation plays a staccato 4 louder.)
                let v = if e.staccato && qz.exported {
                    v.saturating_sub(4)
                } else {
                    v
                };
                let band = if lilypond {
                    lilypond_dynamic(v)
                } else {
                    velocity_to_dynamic(v)
                };
                if level[vi] == Some(band) {
                    continue;
                }
                level[vi] = Some(band);
                if shown.0 != e.on {
                    shown = (e.on, Vec::new());
                }
                if !shown.1.contains(&band) {
                    shown.1.push(band);
                    e.dynamic = Some(band);
                }
            }
        }

        let mut part_tl = Timeline::default();
        let mut offset = 0u8;
        for (si, (st, voices)) in group.iter().zip(&staff_voices).enumerate() {
            let mut tl = Timeline::default();
            for (vi, v) in voices.iter().enumerate() {
                let lane = u8::try_from(vi + 1).unwrap_or(u8::MAX);
                let w = Writer {
                    lane,
                    staff: 1,
                    bars: &bars,
                    main: vi == 0,
                    spell: &spell,
                };
                tl.place_voice(lane, &w.elements(v));
            }
            tl.add(zero, Event::Clef(1, st.clef()));
            // The pedal's net change at each position (events a 64th apart
            // or less are one): down, up, or up and down again.
            let mut at_pos: BTreeMap<Frac, Vec<bool>> = BTreeMap::new();
            for &(t, d) in &st.pedal {
                at_pos.entry(snap(t)).or_default().push(d);
            }
            let mut down = false;
            for (p, moves) in at_pos {
                let after = *moves.last().expect("one move at least");
                let kind = match (down, after) {
                    (false, true) => "start",
                    (true, false) => "stop",
                    (true, true) if moves.contains(&false) => "change",
                    _ => continue,
                };
                down = after;
                let dir = Direction {
                    pedal: Some(PedalEvent {
                        pedal_type: kind.to_string(),
                        line: false,
                    }),
                    ..Direction::default()
                };
                tl.add(p, Event::Direction(Box::new(dir)));
            }
            let top = tl.lanes.keys().copied().max().unwrap_or(0);
            part_tl.absorb(tl.shift_staves(u8::try_from(si).unwrap_or(0), offset));
            offset = offset.saturating_add(top);
        }
        for (p, ts) in &time_events {
            part_tl.add(*p, Event::Time(ts.clone()));
        }
        for (p, k) in &key_events {
            part_tl.add(*p, Event::Key(*k));
        }
        if let Some(pk) = pickup {
            part_tl.add(zero, Event::Partial(pk));
        }
        if pi == 0 && qz.swing {
            let dir = Direction {
                text: Some(TextDirection {
                    text: "Swing".to_string(),
                    placement: Placement::Above,
                    font_style: None,
                    font_weight: None,
                }),
                ..Direction::default()
            };
            part_tl.add(zero, Event::Direction(Box::new(dir)));
        }
        if pi == 0 {
            for (&t, &uspq) in &tempos {
                let dir = Direction {
                    tempo: Some(TempoDirection {
                        text: None,
                        beat_unit: Some("quarter".to_string()),
                        per_minute: Some(bpm(uspq)),
                        dots: 0,
                        placement: Placement::Above,
                    }),
                    ..Direction::default()
                };
                part_tl.add(snap(t), Event::Direction(Box::new(dir)));
            }
        }
        timelines.push(part_tl);
    }

    let grid = Grid::build_tied(timelines.iter());
    for (i, (tl, (name, group))) in timelines.into_iter().zip(&groups).enumerate() {
        let first = &group[0];
        let mut part = Part::new(&format!("P{}", i + 1));
        part.name = name.clone();
        // `Part.midi_channel` is 1–16 as in MusicXML.
        part.midi_channel = first.channel + 1;
        if let Some(p) = first.program {
            part.midi_program = p;
            if let Some(gm) = super::gm::gm_name_from_program(p) {
                part.midi_instrument = gm.to_string();
            }
        }
        part.staves = u8::try_from(group.len()).unwrap_or(1);
        part.measures = split_tied(tl, &grid, OFFSET_DIVISIONS, OFFSET_DIVISIONS);
        if part.staves > 1 {
            if let Some(m) = part.measures.first_mut() {
                m.attributes.get_or_insert_with(Default::default).staves = Some(part.staves);
            }
        }
        score.children.push(ScoreChild::Part(part));
    }
    post_process_beams_and_stems(&mut score);
    Ok(score)
}

// ---------------------------------------------------------------------------
// MIDI pitch conversion
// ---------------------------------------------------------------------------

/// Convert a MIDI key number (0–127) to an IR [`Pitch`], with sharps or flats.
pub(super) fn midi_key_to_pitch(midi_key: u8, use_sharps: bool) -> Pitch {
    let octave = (midi_key as i32 / 12) - 1;
    let (step, alter) = match (midi_key % 12, use_sharps) {
        (0, _) => (PitchStep::C, 0),
        (1, true) => (PitchStep::C, 1),
        (1, false) => (PitchStep::D, -1),
        (2, _) => (PitchStep::D, 0),
        (3, true) => (PitchStep::D, 1),
        (3, false) => (PitchStep::E, -1),
        (4, _) => (PitchStep::E, 0),
        (5, _) => (PitchStep::F, 0),
        (6, true) => (PitchStep::F, 1),
        (6, false) => (PitchStep::G, -1),
        (7, _) => (PitchStep::G, 0),
        (8, true) => (PitchStep::G, 1),
        (8, false) => (PitchStep::A, -1),
        (9, _) => (PitchStep::A, 0),
        (10, true) => (PitchStep::A, 1),
        (10, false) => (PitchStep::B, -1),
        _ => (PitchStep::B, 0),
    };
    Pitch::with_alter(step, Alter::from_integer(alter), octave)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::articulation::StartStop;
    use crate::ir::note::{Note, VoiceElement};

    /// A format-1 file at 480 a quarter from each track's `(tick, event
    /// bytes)`; the first track is the conductor.
    fn smf_events(tracks: Vec<Vec<(u32, Vec<u8>)>>) -> Vec<u8> {
        fn vlq(mut v: u32, out: &mut Vec<u8>) {
            let mut bytes = vec![(v & 0x7f) as u8];
            v >>= 7;
            while v > 0 {
                bytes.push((v & 0x7f) as u8 | 0x80);
                v >>= 7;
            }
            out.extend(bytes.iter().rev());
        }
        let mut out = b"MThd".to_vec();
        out.extend(6u32.to_be_bytes());
        out.extend(1u16.to_be_bytes());
        out.extend((tracks.len() as u16).to_be_bytes());
        out.extend(480u16.to_be_bytes());
        for mut evs in tracks {
            evs.sort_by_key(|e| e.0);
            let (mut body, mut last) = (Vec::new(), 0);
            for (t, e) in evs {
                vlq(t - last, &mut body);
                body.extend(e);
                last = t;
            }
            body.extend([0, 0xff, 0x2f, 0]);
            out.extend(b"MTrk");
            out.extend((body.len() as u32).to_be_bytes());
            out.extend(body);
        }
        out
    }

    fn meter(t: u32, n: u8, d: u8) -> (u32, Vec<u8>) {
        (t, vec![0xff, 0x58, 4, n, d, 24, 8])
    }

    /// A track: its name, then `(on, off, key, velocity)` notes on `ch`.
    fn track(name: &str, ch: u8, notes: &[(u32, u32, u8, u8)]) -> Vec<(u32, Vec<u8>)> {
        let mut evs = vec![(
            0,
            [vec![0xff, 3, name.len() as u8], name.as_bytes().to_vec()].concat(),
        )];
        for &(a, b, k, v) in notes {
            evs.push((a, vec![0x90 | ch, k, v]));
            evs.push((b, vec![0x80 | ch, k, 0]));
        }
        evs
    }

    /// `(name, channel, notes as (on, off, key))`.
    type TrackSpec<'a> = (&'a str, u8, &'a [(u32, u32, u8)]);

    /// A conductor with a meter per `(tick, num, den_pow)`, then the tracks
    /// (velocity 90).
    fn smf(meters: &[(u32, u8, u8)], tracks: &[TrackSpec]) -> Vec<u8> {
        let mut all = vec![meters.iter().map(|&(t, n, d)| meter(t, n, d)).collect()];
        for (name, ch, notes) in tracks {
            let notes: Vec<_> = notes.iter().map(|&(a, b, k)| (a, b, k, 90)).collect();
            all.push(track(name, *ch, &notes));
        }
        smf_events(all)
    }

    /// Each note's (key, dynamic marks) in the first part, in order.
    fn dynamics(s: &Score) -> Vec<(i32, Vec<String>)> {
        s.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some((
                    n.pitch.midi_number(),
                    n.dynamics.iter().map(|d| d.sign.clone()).collect(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn musescore_piano_tracks_are_two_staves() {
        // MuseScore names both staves' tracks after the part, on one channel,
        // and sets the program on the top one.
        let mut rh = track("Piano\0", 0, &[(0, 1920, 72, 90)]);
        rh.insert(1, (0, vec![0xc0, 0]));
        let mut lh = track("Piano\0", 0, &[(0, 1920, 48, 90)]);
        lh.push((0, vec![0xb0, 64, 127]));
        lh.push((1900, vec![0xb0, 64, 0]));
        let s = read_bytes(&smf_events(vec![vec![meter(0, 4, 2)], rh, lh]));
        assert_eq!(s.parts().len(), 1);
        let p = s.parts()[0];
        assert_eq!((p.staves, p.name.as_str()), (2, "Piano"));
        let pedals = p.measures[0]
            .directions
            .iter()
            .filter(|d| d.pedal.is_some())
            .count();
        assert_eq!(pedals, 2, "the left hand's pedal is kept");
    }

    #[test]
    fn far_events_are_refused() {
        let mut conductor = vec![meter(0, 4, 2)];
        conductor.push((0x0FFF_FFFF, vec![0xff, 0x51, 3, 7, 0xa1, 0x20]));
        let bytes = smf_events(vec![conductor, track("p", 0, &[(0, 480, 60, 90)])]);
        assert!(MidiToIrAdapter::new().convert_bytes(&bytes).is_err());
        // Every key held for 99,999 wholes: short enough, too much music.
        let chord: Vec<(u32, u32, u8, u8)> = (0..128).map(|k| (0, 99_999 * 1920, k, 90)).collect();
        let bytes = smf_events(vec![vec![meter(0, 4, 2)], track("p", 0, &chord)]);
        assert!(MidiToIrAdapter::new().convert_bytes(&bytes).is_err());
    }

    #[test]
    fn staccato_and_other_hands_leave_the_dynamics_alone() {
        // LilyPond plays a staccato half length and 4 louder.
        let notes = [
            (0, 240, 60, 94),
            (480, 960, 62, 90),
            (960, 1200, 64, 94),
            (1440, 1920, 65, 90),
        ];
        let mut conductor = vec![meter(0, 4, 2)];
        conductor.push((
            0,
            [vec![0xff, 1, 17], b"creator: LilyPond".to_vec()].concat(),
        ));
        let s = read_bytes(&smf_events(vec![conductor.clone(), track("p", 0, &notes)]));
        let marks: Vec<usize> = dynamics(&s).iter().map(|(_, d)| d.len()).collect();
        assert_eq!(marks, [1, 0, 0, 0], "{:?}", dynamics(&s));
        // A right hand at f over a left hand at p: one mark each.
        let rh = track("upper:", 0, &[(0, 480, 72, 95), (480, 960, 74, 95)]);
        let lh = track("lower:", 1, &[(0, 480, 48, 69), (480, 960, 50, 69)]);
        let s = read_bytes(&smf_events(vec![conductor, rh, lh]));
        let marked = dynamics(&s).iter().filter(|(_, d)| !d.is_empty()).count();
        assert_eq!(marked, 2, "{:?}", dynamics(&s));
    }

    #[test]
    fn each_voice_keeps_its_dynamic() {
        // One staff: `<< { c''2\f d''2 } \\ { e'4\p g' f' a' } >>`.
        let mut conductor = vec![meter(0, 4, 2)];
        conductor.push((
            0,
            [vec![0xff, 1, 17], b"creator: LilyPond".to_vec()].concat(),
        ));
        let notes = [
            (0, 960, 72, 95),
            (960, 1920, 74, 95),
            (0, 480, 64, 69),
            (480, 960, 67, 69),
            (960, 1440, 65, 69),
            (1440, 1920, 69, 69),
        ];
        let s = read_bytes(&smf_events(vec![conductor, track("p", 0, &notes)]));
        let mut marked: Vec<(i32, Vec<String>)> = dynamics(&s)
            .into_iter()
            .filter(|(_, d)| !d.is_empty())
            .collect();
        marked.sort();
        assert_eq!(
            marked,
            [(64, vec!["p".to_string()]), (72, vec!["f".to_string()])]
        );
    }

    #[test]
    fn a_full_length_last_note_is_not_a_staccato() {
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("p", 0, &[(0, 480, 60, 90), (480, 960, 62, 94)]),
        ]));
        let d = s.parts()[0].measures[0].voices[0].elements[1].metric_duration();
        assert_eq!(d, Frac::new(1, 4));
    }

    fn read_bytes(b: &[u8]) -> Score {
        MidiToIrAdapter::new().convert_bytes(b).unwrap()
    }

    #[test]
    fn chords_ties_and_tuplets() {
        let notes: &[(u32, u32, u8)] = &[
            (0, 480, 60),
            (0, 480, 64),
            (0, 480, 67),
            (480, 640, 62),
            (640, 800, 64),
            (800, 960, 65),
            (960, 1440, 67),
            (1440, 2400, 69),
        ];
        let s = read_bytes(&smf(&[(0, 4, 2)], &[("p", 0, notes)]));
        let m = &s.parts()[0].measures;
        assert_eq!(m.len(), 2);
        let e = &m[0].voices[0].elements;
        assert!(matches!(&e[0], VoiceElement::Chord(c) if c.notes.len() == 3));
        match &e[1] {
            VoiceElement::Note(n) => {
                assert_eq!((n.duration.tuplet_actual, n.duration.tuplet_normal), (3, 2));
                assert_eq!(n.duration.base, Frac::new(1, 8));
                assert!(n
                    .tuplet
                    .as_ref()
                    .is_some_and(|t| t.tuplet_type == StartStop::Start));
            }
            other => panic!("{other:?}"),
        }
        // The half note from beat 4 is tied into bar 2.
        let tied = |e: &VoiceElement, t: StartStop| matches!(e, VoiceElement::Note(n) if n.pitch.midi_number() == 69 && n.ties.iter().any(|x| x.tie_type == t));
        assert!(tied(e.last().unwrap(), StartStop::Start));
        assert!(tied(&m[1].voices[0].elements[0], StartStop::Stop));
    }

    #[test]
    fn pickup_and_meter_change() {
        let notes: &[(u32, u32, u8)] = &[(0, 480, 67), (480, 2400, 60), (2400, 3360, 62)];
        let s = read_bytes(&smf(
            &[(0, 1, 2), (480, 4, 2), (2400, 2, 2)],
            &[("p", 0, notes)],
        ));
        let m = &s.parts()[0].measures;
        assert!(m[0].implicit);
        let meter = |i: usize| {
            m[i].attributes
                .as_ref()
                .and_then(|a| a.time.clone())
                .map(|t| t.beats)
        };
        assert_eq!(meter(0).as_deref(), Some("4"));
        assert_eq!(meter(2).as_deref(), Some("2"));
    }

    #[test]
    fn a_piano_in_two_tracks_is_one_part() {
        let rh: &[(u32, u32, u8)] = &[(0, 1920, 72)];
        let lh: &[(u32, u32, u8)] = &[(0, 1920, 48)];
        let s = read_bytes(&smf(&[(0, 4, 2)], &[("upper:", 0, rh), ("lower:", 1, lh)]));
        assert_eq!(s.parts().len(), 1);
        let p = s.parts()[0];
        assert_eq!(p.staves, 2);
        let clefs = &p.measures[0].attributes.as_ref().unwrap().clefs;
        assert_eq!(clefs.get(&2).map(|c| c.sign), Some(ClefSign::F));
    }

    #[test]
    fn tempos_come_back_whole() {
        assert_eq!(bpm(606_060), 99.0);
        assert_eq!(bpm(500_000), 120.0);
        assert_eq!(bpm(700_000), 85.71);
    }

    #[test]
    fn a_played_one_track_piano_splits_into_hands() {
        let mut notes = Vec::new();
        for i in 0..8u32 {
            let on = i * 480 + [0, 7, 3, 11, 5, 9, 2, 6][i as usize];
            notes.push((
                on,
                on + 430,
                [72, 74, 76, 77, 79, 77, 76, 74][i as usize],
                80,
            ));
        }
        for i in 0..4u32 {
            let on = i * 960 + [0, 5, 9, 4][i as usize];
            notes.push((on, on + 900, [48, 43, 45, 41][i as usize], 70));
        }
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("Piano", 0, &notes),
        ]));
        assert_eq!(s.parts().len(), 1);
        assert_eq!(s.parts()[0].staves, 2);
    }

    #[test]
    fn played_hands_keep_their_lines_across_middle_c() {
        // Bar 1: the right hand's quarters walk down to B3 and A3 over the
        // left hand's low halves. Bar 2: the left hand's quarters climb to C4
        // and D4 under the right hand's high halves.
        let mut notes = Vec::new();
        let jit = [0u32, 7, 3, 11, 5, 9, 2, 6];
        for (i, k) in [72u8, 69, 65, 62, 59, 57, 60, 64].into_iter().enumerate() {
            let on = i as u32 * 240 + jit[i];
            notes.push((on, on + 220, k, 80));
        }
        for (i, k) in [43u8, 41].into_iter().enumerate() {
            let on = i as u32 * 960 + jit[i + 2];
            notes.push((on, on + 900, k, 70));
        }
        for (i, k) in [48u8, 52, 55, 60, 62, 60, 55, 52].into_iter().enumerate() {
            let on = 1920 + i as u32 * 240 + jit[i];
            notes.push((on, on + 220, k, 70));
        }
        for (i, k) in [79u8, 76].into_iter().enumerate() {
            let on = 1920 + i as u32 * 960 + jit[i + 4];
            notes.push((on, on + 900, k, 80));
        }
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("Piano", 0, &notes),
        ]));
        let p = s.parts()[0];
        assert_eq!(p.staves, 2);
        let staff_of = |bar: usize, key: i32| -> Vec<u8> {
            p.measures[bar]
                .voices
                .iter()
                .flat_map(|v| &v.elements)
                .filter_map(|e| match e {
                    VoiceElement::Note(n) if n.pitch.midi_number() == key => Some(n.staff),
                    _ => None,
                })
                .collect()
        };
        for key in [59, 57, 60, 64] {
            assert_eq!(staff_of(0, key), [1], "bar 1, key {key}");
        }
        for key in [60, 62] {
            assert!(staff_of(1, key).iter().all(|&s| s == 2), "bar 2, key {key}");
        }
    }

    fn raw(on: u64, off: u64, key: u8) -> RawNote {
        RawNote {
            start_tick: on,
            end_tick: off,
            midi_key: key,
            velocity: 80,
            channel: 0,
        }
    }

    /// The keys `left_hand` gives each hand.
    fn hands(notes: &[RawNote]) -> (Vec<u8>, Vec<u8>) {
        let left = left_hand(notes, 30);
        let pick = |l: bool| {
            let mut k: Vec<u8> = notes
                .iter()
                .zip(&left)
                .filter(|(_, &x)| x == l)
                .map(|(n, _)| n.midi_key)
                .collect();
            k.sort_unstable();
            k.dedup();
            k
        };
        (pick(true), pick(false))
    }

    #[test]
    fn hands_follow_octaves_chords_and_their_span() {
        let jit = [0u64, 7, 3, 11, 5, 9, 2, 6];
        // Left-hand octaves climbing across middle C under right-hand halves.
        let mut notes = Vec::new();
        for (i, k) in [48u8, 50, 52, 53, 55, 57, 59, 60].into_iter().enumerate() {
            let on = i as u64 * 480 + jit[i];
            notes.push(raw(on, on + 430, k));
            notes.push(raw(on + 4, on + 430, k + 12));
        }
        for (i, k) in [76u8, 77, 79, 81].into_iter().enumerate() {
            let on = i as u64 * 960 + jit[i + 2];
            notes.push(raw(on, on + 900, k));
        }
        assert_eq!(hands(&notes).1, [76, 77, 79, 81]);
        // Given in any order.
        notes.reverse();
        assert_eq!(hands(&notes).1, [76, 77, 79, 81]);

        // An accompanying G major triad under the melody: D4 stays below.
        // (Under D5 it would tie with right-hand octaves: the right wins.)
        let mut notes = Vec::new();
        for i in 0..8u64 {
            let on = i * 480 + jit[i as usize];
            for k in [55u8, 59, 62, 76] {
                notes.push(raw(on, on + 440, k));
            }
        }
        assert_eq!(hands(&notes), (vec![55, 59, 62], vec![76]));

        // A chord no hand can span: the left takes an octave, and B3 goes
        // to the right.
        let notes = [
            raw(0, 900, 43),
            raw(2, 900, 50),
            raw(4, 900, 55),
            raw(6, 900, 59),
            raw(8, 900, 64),
        ];
        assert_eq!(hands(&notes), (vec![43, 50, 55], vec![59, 64]));
    }

    #[test]
    fn swing_is_straightened_and_marked() {
        let words = |s: &Score| -> Vec<String> {
            s.parts()[0]
                .measures
                .iter()
                .flat_map(|m| &m.directions)
                .filter_map(|d| d.text.as_ref().map(|t| t.text.clone()))
                .collect()
        };
        let eighths = |s: &Score| -> Vec<Frac> {
            s.parts()[0].measures[0].voices[0]
                .elements
                .iter()
                .map(|e| e.metric_duration())
                .collect()
        };
        // Played 2:1 pairs (a few ticks off).
        let jit = [0u32, 5, 3, 7, 2, 6, 4, 1];
        let mut played = Vec::new();
        for b in 0..8u32 {
            played.push((b * 480 + jit[b as usize], b * 480 + 300, 60, 85));
            played.push((b * 480 + 322 - jit[b as usize], b * 480 + 470, 64, 80));
        }
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("p", 0, &played),
        ]));
        assert_eq!(words(&s), ["Swing"]);
        assert!(
            eighths(&s).iter().all(|&d| d == Frac::new(1, 8)),
            "{:?}",
            eighths(&s)
        );
        // Written at 2:1 exactly, as `\tuplet 3/2 { c'4 e'8 }` is: triplets,
        // unless asked.
        let exact: Vec<_> = (0..8u32)
            .flat_map(|b| {
                [
                    (b * 480, b * 480 + 320, 60, 85),
                    (b * 480 + 320, b * 480 + 480, 64, 80),
                ]
            })
            .collect();
        let bytes = smf_events(vec![vec![meter(0, 4, 2)], track("p", 0, &exact)]);
        let s = read_bytes(&bytes);
        assert!(words(&s).is_empty());
        assert_eq!(eighths(&s)[0], Frac::new(1, 6));
        let s = MidiToIrAdapter::new()
            .with_swing(Some(true))
            .convert_bytes(&bytes)
            .unwrap();
        assert_eq!(words(&s), ["Swing"]);
        assert_eq!(eighths(&s)[0], Frac::new(1, 8));
        // Asked not to: the played pairs stay as played.
        let bytes = smf_events(vec![vec![meter(0, 4, 2)], track("p", 0, &played)]);
        let s = MidiToIrAdapter::new()
            .with_swing(Some(false))
            .convert_bytes(&bytes)
            .unwrap();
        assert!(words(&s).is_empty());
        // One swung pair among quarters is no swing.
        let mut once: Vec<_> = (0..8u32)
            .filter(|&b| b != 6)
            .map(|b| (b * 480 + jit[b as usize], b * 480 + 440, 60, 85))
            .collect();
        once.push((2880 + 3, 3180, 62, 85));
        once.push((3200 - 4, 3350, 64, 80));
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("p", 0, &once),
        ]));
        assert!(words(&s).is_empty());
    }

    #[test]
    fn a_rubato_piano_still_splits() {
        // Quarters slowing (gaps 480 + 17i) over a bass on beats 1 and 3:
        // after beat tracking the file is still a performance.
        let mut notes = Vec::new();
        let mut on = 0u32;
        let mut ons = Vec::new();
        for (i, k) in [72u8, 74, 76, 77, 79, 77, 76, 74].into_iter().enumerate() {
            let gap = 480 + 17 * i as u32;
            notes.push((on, on + gap * 9 / 10, k, 85));
            ons.push(on);
            on += gap;
        }
        for (j, k) in [48u8, 43, 45, 41].into_iter().enumerate() {
            let end = ons.get(2 * j + 2).map_or(4276, |&n| n - 40);
            notes.push((ons[2 * j], end, k, 70));
        }
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("Piano", 0, &notes),
        ]));
        let p = s.parts()[0];
        assert_eq!((s.parts().len(), p.staves, p.measures.len()), (1, 2, 2));
    }

    #[test]
    fn played_staccatos_are_found_by_their_length() {
        // (written length, staccato) of every note.
        let read = |notes: &[(u32, u32, u8, u8)]| -> Vec<(Frac, bool)> {
            let s = read_bytes(&smf_events(vec![
                vec![meter(0, 4, 2)],
                track("p", 0, notes),
            ]));
            s.parts()[0]
                .measures
                .iter()
                .flat_map(|m| &m.voices[0].elements)
                .filter_map(|e| match e {
                    VoiceElement::Note(n) => Some((
                        n.duration.actual_duration(),
                        n.articulations.iter().any(|a| a.name == "staccato"),
                    )),
                    _ => None,
                })
                .collect()
        };
        let jit = [0u32, 7, 3, 9, 1, 4, 2, 6];
        // Quarters held 200 ticks of 480.
        let short: Vec<_> = (0..8u32)
            .map(|i| {
                (
                    i * 480 + jit[i as usize],
                    i * 480 + jit[i as usize] + 200,
                    60 + i as u8,
                    80,
                )
            })
            .collect();
        assert_eq!(read(&short), vec![(Frac::new(1, 4), true); 8]);
        // Halves released at 65 %.
        let halves = [
            (0, 624, 60, 80),
            (967, 1591, 62, 80),
            (1915, 2539, 64, 80),
            (2889, 3513, 65, 80),
        ];
        assert_eq!(read(&halves), vec![(Frac::new(1, 2), true); 4]);
        // Quarters held at 85 %: plain.
        let held: Vec<_> = (0..8u32)
            .map(|i| {
                (
                    i * 480 + jit[i as usize],
                    i * 480 + jit[i as usize] + 408,
                    60 + i as u8,
                    80,
                )
            })
            .collect();
        assert_eq!(read(&held), vec![(Frac::new(1, 4), false); 8]);
        // Released 5 ticks past the beat: a quarter (and a rest), not a
        // staccato half.
        let q = |n| (Frac::new(n, 4), false);
        assert_eq!(read(&[(0, 485, 60, 80), (967, 1440, 62, 80)]), [q(1), q(1)]);
        // A detached half and an eighth rest: the staccato stays.
        assert_eq!(
            read(&[(0, 576, 60, 80), (1207, 1440, 62, 80)]),
            [(Frac::new(1, 2), true), (Frac::new(1, 8), false)]
        );
    }

    fn staccatos(s: &Score) -> Vec<(i32, bool, usize)> {
        s.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .flat_map(|e| match e {
                VoiceElement::Note(n) => vec![n.as_ref()],
                VoiceElement::Chord(c) => c.notes.iter().collect(),
                _ => vec![],
            })
            .map(|n| {
                (
                    n.pitch.midi_number(),
                    n.articulations.iter().any(|a| a.name == "staccato"),
                    n.dynamics.len(),
                )
            })
            .collect()
    }

    #[test]
    fn notes_held_by_the_pedal_are_no_staccatos() {
        let mut t = track(
            "Piano",
            0,
            &[
                (0, 200, 60, 80),
                (487, 687, 62, 80),
                (953, 1153, 64, 80),
                (1446, 1646, 65, 80),
            ],
        );
        t.push((0, vec![0xb0, 64, 127]));
        t.push((1900, vec![0xb0, 64, 0]));
        let s = read_bytes(&smf_events(vec![vec![meter(0, 4, 2)], t]));
        assert!(staccatos(&s).iter().all(|n| !n.1), "{:?}", staccatos(&s));
    }

    #[test]
    fn a_run_of_lilypond_staccatos_is_found() {
        // `{ c'4 d'4-. e'4-. f'4-. g'4 }` as LilyPond plays it (at 384 a
        // quarter, 120 bpm): staccatos half length and 4 louder.
        let mut conductor = vec![meter(0, 4, 2)];
        conductor.push((
            0,
            [vec![0xff, 1, 17], b"creator: LilyPond".to_vec()].concat(),
        ));
        let notes = [
            (0, 384, 60, 90),
            (384, 576, 62, 94),
            (768, 960, 64, 94),
            (1152, 1344, 65, 94),
            (1536, 1920, 67, 90),
        ];
        let mut b = smf_events(vec![conductor, track("p", 0, &notes)]);
        (b[12], b[13]) = (0x01, 0x80);
        let got = staccatos(&read_bytes(&b));
        let marks: Vec<(bool, usize)> = got.iter().map(|n| (n.1, n.2)).collect();
        assert_eq!(
            marks,
            [(false, 1), (true, 0), (true, 0), (true, 0), (false, 0)],
            "{got:?}"
        );
    }

    #[test]
    fn a_lilypond_run_ends_where_notes_join() {
        // `{ c'8 r8 d'4 e'2 | f'1\mf }` at 60 bpm: no dynamic is 90, mf 86.
        // c' is no staccato: d' and e' are joined, no run of staccatos.
        let mut conductor = vec![meter(0, 4, 2)];
        conductor.push((
            0,
            [vec![0xff, 1, 17], b"creator: LilyPond".to_vec()].concat(),
        ));
        conductor.push((0, vec![0xff, 0x51, 3, 0x0f, 0x42, 0x40]));
        let notes = [
            (0, 240, 60, 90),
            (480, 960, 62, 90),
            (960, 1920, 64, 90),
            (1920, 3840, 65, 86),
        ];
        let got = staccatos(&read_bytes(&smf_events(vec![
            conductor,
            track("p", 0, &notes),
        ])));
        assert!(got.iter().all(|n| !n.1), "{got:?}");
    }

    #[test]
    fn a_tap_on_the_beat_is_a_staccato() {
        let got = staccatos(&read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("p", 0, &[(0, 25, 60, 80), (967, 1440, 62, 80)]),
        ])));
        assert_eq!(got.iter().map(|n| n.1).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn a_merged_fifth_voice_keeps_its_length() {
        // Five notes struck together, four voices busy: G3 (sounding 950
        // ticks) joins the shortest one's chord; it is no staccato.
        let notes = [
            (0, 2390, 72, 80),
            (0, 1900, 67, 80),
            (0, 1430, 64, 80),
            (0, 150, 60, 80),
            (0, 950, 55, 80),
            (3847, 4300, 60, 80),
            (4327, 4780, 62, 80),
            (4807, 5260, 64, 80),
            (5287, 5740, 65, 80),
        ];
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("p", 0, &notes),
        ]));
        assert!(
            !staccatos(&s).iter().any(|n| n.0 == 55 && n.1),
            "{:?}",
            staccatos(&s)
        );
    }

    /// Each measure's time signature, where one is set.
    fn meters_of(s: &Score) -> Vec<(usize, String)> {
        s.parts()[0]
            .measures
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let t = m.attributes.as_ref()?.time.as_ref()?;
                Some((i, format!("{}/{}", t.beats, t.beat_type)))
            })
            .collect()
    }

    #[test]
    fn signatures_stay_on_the_file_grid_when_beats_are_tracked() {
        // A ritardando of 20 quarters (gaps 480 + 8i); the file changes to
        // 3/4 at tick 7680, its bar 5: the performer's 17th note.
        let mut on = 0u32;
        let notes: Vec<_> = (0..20u32)
            .map(|i| {
                let n = (on, on + (480 + 8 * i) * 9 / 10, 60 + (i % 12) as u8, 85);
                on += 480 + 8 * i;
                n
            })
            .collect();
        let bytes = smf_events(vec![
            vec![meter(0, 4, 2), meter(7680, 3, 2)],
            track("p", 0, &notes),
        ]);
        let s = read_bytes(&bytes);
        assert_eq!(
            meters_of(&s),
            [(0, "4/4".to_string()), (4, "3/4".to_string())]
        );
        assert!(s.parts()[0].measures[..4].iter().all(|m| m
            .voices
            .iter()
            .map(|v| v.elements.len())
            .sum::<usize>()
            >= 4));

        // A pickup (1/4 then 4/4), the downbeat played 60 ticks late and the
        // tempo slowing on: still a pickup.
        let mut notes = vec![(0, 430, 67, 85)];
        let mut on = 540u32;
        for i in 0..12u32 {
            notes.push((on, on + 450, 60 + i as u8, 85));
            on += 500 + 6 * i;
        }
        let bytes = smf_events(vec![
            vec![meter(0, 1, 2), meter(480, 4, 2)],
            track("p", 0, &notes),
        ]);
        let s = read_bytes(&bytes);
        assert!(s.parts()[0].measures[0].implicit, "{:?}", meters_of(&s));
    }

    #[test]
    fn a_tempo_after_a_ritardando_is_followed() {
        // 12 quarters slowing (480 → 700), then 16 quarters back at 480.
        let mut on = 0u32;
        let mut ons = Vec::new();
        for i in 0..12u32 {
            ons.push(on);
            on += 480 + 20 * i;
        }
        for _ in 0..16 {
            ons.push(on);
            on += 480;
        }
        let beats = super::beats::drifting_beats(
            &ons.iter().map(|&t| u64::from(t)).collect::<Vec<_>>(),
            480,
        )
        .expect("it drifts");
        let mapped: Vec<u64> = ons
            .iter()
            .map(|&t| super::beats::onto_grid(u64::from(t), &beats, 480))
            .collect();
        assert_eq!(mapped, (0..28).map(|k| k * 480).collect::<Vec<u64>>());
    }

    /// Keys per staff (1 = right hand) of a one-track played piano.
    fn staff_keys(notes: &[(u32, u32, u8, u8)], meter_n: u8) -> (usize, Vec<i32>, Vec<i32>) {
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, meter_n, 2)],
            track("Piano", 0, notes),
        ]));
        let p = s.parts()[0];
        let mut by = [Vec::new(), Vec::new()];
        for e in p
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
        {
            let ns: Vec<&Note> = match e {
                VoiceElement::Note(n) => vec![n.as_ref()],
                VoiceElement::Chord(c) => c.notes.iter().collect(),
                _ => vec![],
            };
            for n in ns {
                by[usize::from(n.staff.max(1) - 1).min(1)].push(n.pitch.midi_number());
            }
        }
        for b in by.iter_mut() {
            b.sort_unstable();
            b.dedup();
        }
        let [rh, lh] = by;
        (p.staves.into(), rh, lh)
    }

    #[test]
    fn hands_that_never_strike_together_keep_their_registers() {
        // A syncopated melody over held triads.
        let mut notes = vec![];
        for (k, at) in [
            (48u8, 0u32),
            (52, 0),
            (55, 0),
            (45, 1927),
            (48, 1927),
            (52, 1927),
        ] {
            notes.push((at, at + 1880, k, 70));
        }
        let ons = [247u32, 733, 1213, 1691, 2165, 2649, 3137, 3606];
        for (i, k) in [76u8, 77, 79, 77, 76, 74, 72, 74].into_iter().enumerate() {
            notes.push((ons[i], ons[i] + 440, k, 85));
        }
        let (staves, rh, lh) = staff_keys(&notes, 4);
        assert_eq!(
            (staves, rh, lh),
            (2, vec![72, 74, 76, 77, 79], vec![45, 48, 52, 55])
        );
        // Chords alternating between the hands.
        let jit = [0u32, 7, 3, 9, 1, 4, 2, 6];
        let mut alt = vec![];
        for b in 0..8u32 {
            for k in [48u8, 52, 55] {
                alt.push((b * 480 + jit[b as usize], b * 480 + 200, k, 70));
            }
            for k in [72u8, 76, 79] {
                alt.push((b * 480 + 245 + jit[b as usize], b * 480 + 440, k, 80));
            }
        }
        let (staves, rh, lh) = staff_keys(&alt, 4);
        assert_eq!((staves, rh, lh), (2, vec![72, 76, 79], vec![48, 52, 55]));
    }

    #[test]
    fn a_busy_hand_over_a_sparse_one_is_two_staves() {
        // Right-hand 16ths over left-hand halves: 4 of 36 notes below C4.
        let rh = [
            72u8, 74, 76, 77, 79, 81, 83, 84, 83, 81, 79, 77, 76, 74, 72, 71,
        ];
        let jit = [0u32, 5, 9, 3, 7, 11, 2, 6];
        let mut notes = vec![];
        for i in 0..32u32 {
            let on = i * 120 + jit[(i % 8) as usize];
            notes.push((on, on + 100, rh[(i % 16) as usize], 80));
        }
        for (i, k) in [48u8, 43, 45, 41].into_iter().enumerate() {
            let on = i as u32 * 960 + jit[i];
            notes.push((on, on + 900, k, 70));
        }
        let (staves, _, lh) = staff_keys(&notes, 4);
        assert_eq!((staves, lh), (2, vec![41, 43, 45, 48]));
    }

    #[test]
    fn a_hand_holding_a_note_reaches_no_further() {
        // A waltz: the right hand holds E5, D5 for the bar; the left plays
        // the bass and chords with C4 or B3 on top.
        let notes = [
            (0, 440, 36, 70),
            (4, 1400, 76, 85),
            (487, 927, 52, 70),
            (487, 927, 55, 70),
            (487, 927, 60, 70),
            (963, 1403, 52, 70),
            (963, 1403, 55, 70),
            (963, 1403, 60, 70),
            (1447, 1887, 43, 70),
            (1444, 2840, 74, 85),
            (1923, 2363, 53, 70),
            (1923, 2363, 55, 70),
            (1923, 2363, 59, 70),
            (2411, 2851, 53, 70),
            (2411, 2851, 55, 70),
            (2411, 2851, 59, 70),
        ];
        let (staves, rh, _) = staff_keys(&notes, 3);
        assert_eq!((staves, rh), (2, vec![74, 76]));
    }

    #[test]
    fn a_legato_line_keeps_to_its_hand() {
        // Left-hand eighths over two octaves, each held into the next.
        let jit = [0u32, 7, 3, 11, 5, 9, 2, 6];
        let mut notes = vec![];
        for b in 0..2u32 {
            for (i, k) in [36u8, 43, 52, 55, 60, 55, 52, 43].into_iter().enumerate() {
                let on = b * 1920 + i as u32 * 240 + jit[i];
                notes.push((on, on + 255, k, 70));
            }
            for (i, k) in [[76u8, 77, 79, 76], [74, 72, 71, 72]][b as usize]
                .into_iter()
                .enumerate()
            {
                let on = b * 1920 + i as u32 * 480 + jit[(i + 3) % 8];
                notes.push((on, on + 440, k, 85));
            }
        }
        let (staves, rh, lh) = staff_keys(&notes, 4);
        assert_eq!((staves, lh), (2, vec![36, 43, 52, 55, 60]));
        assert_eq!(rh, [71, 72, 74, 76, 77, 79]);
    }

    #[test]
    fn a_huge_stacked_chord_is_split_in_linear_time() {
        // 20,000 notes struck together (unisons stack): one chord.
        let notes: Vec<RawNote> = (0..20_000u64)
            .map(|i| raw(7, 8, 36 + (i % 49) as u8))
            .collect();
        let left = left_hand(&notes, 30);
        assert_eq!(left.len(), notes.len());
    }

    #[test]
    fn a_key_is_found_when_the_file_has_none() {
        // A D major scale and cadence, no key signature: F♯ and C♯, not G♭/D♭.
        let keys = [62, 64, 66, 67, 69, 71, 73, 74, 69, 66, 62, 73, 74];
        let notes: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, &k)| (i as u32 * 480, i as u32 * 480 + 480, k, 80))
            .collect();
        let s = read_bytes(&smf_events(vec![
            vec![meter(0, 4, 2)],
            track("p", 0, &notes),
        ]));
        let key = s.parts()[0].measures[0]
            .attributes
            .as_ref()
            .and_then(|a| a.key);
        assert_eq!(key.map(|k| k.fifths), Some(2));
        let fsharp = s.parts()[0].measures[0].voices[0]
            .elements
            .iter()
            .find_map(|e| match e {
                VoiceElement::Note(n) if n.pitch.midi_number() == 66 => Some(n.pitch),
                _ => None,
            })
            .unwrap();
        assert_eq!(fsharp.step, PitchStep::F);
    }

    #[test]
    fn karaoke_text_is_sung() {
        let text = |t: u32, s: &str| {
            (
                t,
                [vec![0xff, 1, s.len() as u8], s.as_bytes().to_vec()].concat(),
            )
        };
        let mut words = track(
            "Melody",
            0,
            &[(0, 480, 60, 80), (480, 960, 60, 80), (960, 1440, 67, 80)],
        );
        words.extend([text(0, "\\Twin"), text(480, "kle"), text(960, " twin")]);
        let conductor = vec![meter(0, 4, 2), text(0, "@KMIDI KARAOKE FILE")];
        let s = read_bytes(&smf_events(vec![conductor, words]));
        let sung: Vec<String> = s.parts()[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => n.lyrics.first().map(|l| l.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(sung, ["Twin", "kle", "twin"]);
    }

    #[test]
    fn played_music_quantizes_no_finer_than_asked() {
        // A played 16th pair: to eighths when asked for eighths.
        let notes = [(0, 110, 60, 80), (125, 460, 62, 80), (492, 950, 64, 80)];
        let bytes = smf_events(vec![vec![meter(0, 4, 2)], track("p", 0, &notes)]);
        let s = MidiToIrAdapter::new()
            .with_quantize(Some(8))
            .convert_bytes(&bytes)
            .unwrap();
        let d: Vec<Frac> = s.parts()[0].measures[0].voices[0]
            .elements
            .iter()
            .map(|e| e.metric_duration())
            .collect();
        assert!(d.iter().all(|x| *x >= Frac::new(1, 8)), "{d:?}");
        // 12 is no power of two: it counts as 8 (triplets aren't forced).
        let jit = [0u32, 7, 3, 5, 1, 6, 2, 4];
        let eighths: Vec<_> = (0..8u32)
            .map(|i| (i * 240 + jit[i as usize], i * 240 + 200, 60 + i as u8, 80))
            .collect();
        let bytes = smf_events(vec![vec![meter(0, 4, 2)], track("p", 0, &eighths)]);
        let s = MidiToIrAdapter::new()
            .with_quantize(Some(12))
            .convert_bytes(&bytes)
            .unwrap();
        let d: Vec<Frac> = s.parts()[0].measures[0].voices[0]
            .elements
            .iter()
            .map(|e| e.metric_duration())
            .collect();
        assert_eq!(d, vec![Frac::new(1, 8); 8]);
    }

    #[test]
    fn test_zero_tpb_header_is_clamped() {
        let bytes: &[u8] = &[
            b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 0, b'M', b'T', b'r', b'k', 0, 0, 0,
            4, 0, 0xFF, 0x2F, 0x00,
        ];
        let _ = MidiToIrAdapter::new().convert_bytes(bytes);
    }

    #[test]
    fn test_midi_key_roundtrip() {
        for key in 0..128u8 {
            assert_eq!(midi_key_to_pitch(key, true).midi_number(), key as i32);
            assert_eq!(midi_key_to_pitch(key, false).midi_number(), key as i32);
        }
        let p = midi_key_to_pitch(61, false);
        assert_eq!((p.step, p.alter), (PitchStep::D, Alter::from_integer(-1)));
    }
}
