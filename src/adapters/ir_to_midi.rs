//! IR → MIDI adapter.
//!
//! Plays an IR [`Score`] into a Standard MIDI File (format 1, 384 ticks per
//! quarter by default): track 0 is the conductor (tempo, meter, key), then one
//! track per staff. LilyPond's MIDI performers are the reference, so that
//! `ly → MIDI` sounds like LilyPond's own MIDI:
//!
//! - a bar lasts as long as its music (a pickup is short); repeats are played
//!   out with their endings (`with_unfold_repeats(false)` keeps the written
//!   order), as MuseScore does;
//! - positions are exact fractions until the last step, so tuplets don't drift;
//! - velocity follows LilyPond's dynamics table and instrument equalizer
//!   (`ly/midi-init.ly`) and its hairpin rule (`lily/dynamic-performer.cc`);
//!   a note's own velocity (from a MIDI file) wins;
//! - staccato, staccatissimo and portato shorten a note, accents add velocity
//!   (`ly/script-init.ly`);
//! - grace notes sound just before the beat, for 9/40 of their written length,
//!   and cut the note before them short (`lily/note-performer.cc`,
//!   `lily/audio-item.cc`);
//! - a tied chain sounds once; the sustain pedal is CC64; lyrics are lyric
//!   events; `<transpose>` gives the sounding pitch.
//!
//! A bar whose length differs from its meter (a pickup, a cadenza) gets a time
//! signature of its real length, then the meter again: LilyPond itself has no
//! way to say `\partial` in MIDI, MuseScore writes it this way, and readers
//! (lytk's included) take a short first bar as a pickup.

use std::collections::HashMap;
use std::path::Path;

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use super::dynamics_velocity::{lilypond_equalizer, lilypond_volume, LILYPOND_DEFAULT_VOLUME};
use super::{AdapterError, FromIrAdapter, Result};
use crate::ir::articulation::{Articulation, LyricSyllable, StartStop, SyllabicType};
use crate::ir::direction::{BarlineType, RepeatDirection};
use crate::ir::duration::Frac;
use crate::ir::measure::{ClefSign, KeyMode, KeySignature, Measure, TimeSignature};
use crate::ir::note::{Note, VoiceElement};
use crate::ir::part::Part;
use crate::ir::voice::Voice;
use crate::ir::Score;

/// LilyPond's MIDI resolution (`lily/audio-item.cc`): 384 ticks a quarter.
const DEFAULT_PPQ: u16 = 384;

/// Grace notes sound for 9/40 of their written length (`moment_to_real`).
fn grace_factor() -> Frac {
    Frac::new(9, 40)
}

fn zero() -> Frac {
    Frac::from_integer(0)
}

// ---------------------------------------------------------------------------
// Public adapter
// ---------------------------------------------------------------------------

/// Adapter that plays an IR [`Score`] into a Standard MIDI File.
pub struct IrToMidiAdapter {
    /// Velocity where no dynamic is in force; `None` follows LilyPond (90,
    /// or its equalized value for an orchestral instrument).
    velocity: Option<u8>,
    /// Ticks per quarter note.
    divisions: u16,
    /// Play repeats out (default) or keep the written order.
    unfold_repeats: bool,
}

impl IrToMidiAdapter {
    pub fn new() -> Self {
        Self {
            velocity: None,
            divisions: DEFAULT_PPQ,
            unfold_repeats: true,
        }
    }

    /// Set the velocity used where no dynamic is in force.
    pub fn with_velocity(mut self, vel: u8) -> Self {
        self.velocity = Some(vel.clamp(1, 127));
        self
    }

    /// Set the output divisions (ticks per quarter note).
    pub fn with_divisions(mut self, divisions: u16) -> Self {
        self.divisions = divisions.max(1);
        self
    }

    /// Play repeats out with their endings (`true`, the default) or keep the
    /// bars in written order.
    pub fn with_unfold_repeats(mut self, unfold: bool) -> Self {
        self.unfold_repeats = unfold;
        self
    }

    /// Render a score to raw MIDI bytes.
    pub fn convert_bytes(&self, score: &Score) -> Result<Vec<u8>> {
        let parts = score.parts();
        let bars = Bars::new(&parts, self.unfold_repeats);
        let tempo = TempoMap::new(&parts, &bars);
        let clock = Clock {
            ppq: self.divisions as i64,
        };

        let mut tracks: Vec<Vec<Timed>> = vec![conductor(&bars, &tempo, &clock)];
        let channels = assign_channels(&parts);
        for (pi, part) in parts.iter().enumerate() {
            let channel = channels[pi];
            let program = program_of(part);
            let staves = part.staves.max(1);
            for staff in 1..=staves {
                let mut track = vec![Timed::meta(
                    0,
                    Meta::TrackName(track_name(part, staff, staves)),
                )];
                if !part.midi_instrument.is_empty() {
                    track.push(Timed::meta(
                        0,
                        Meta::Instrument(part.midi_instrument.clone()),
                    ));
                }
                track.push(Timed {
                    tick: 0,
                    rank: 2,
                    ev: Ev::Program(channel, program),
                });
                let player = Player {
                    part,
                    staff,
                    staves,
                    bars: &bars,
                    tempo: &tempo,
                    default_velocity: self.velocity,
                    end: bars.end,
                };
                track.extend(player.play(channel, &clock));
                tracks.push(track);
            }
        }
        encode(&tracks, self.divisions)
    }
}

impl Default for IrToMidiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIrAdapter for IrToMidiAdapter {
    fn convert(&self, _score: &Score) -> Result<String> {
        Err(AdapterError::Unsupported(
            "MIDI is binary; use write() or convert_bytes()".into(),
        ))
    }

    fn write(&self, score: &Score, path: &Path) -> Result<()> {
        let bytes = self.convert_bytes(score)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

impl super::FromMusicAdapter for IrToMidiAdapter {
    fn convert_music(&self, _doc: &crate::ir::music::MusicDocument) -> Result<String> {
        Err(AdapterError::Unsupported(
            "MIDI is binary; use write_music()".into(),
        ))
    }

    fn write_music(&self, doc: &crate::ir::music::MusicDocument, path: &Path) -> Result<()> {
        let score = crate::ir::lower::lower_to_score(doc);
        self.write(&score, path)
    }
}

// ---------------------------------------------------------------------------
// Positions → ticks
// ---------------------------------------------------------------------------

struct Clock {
    ppq: i64,
}

impl Clock {
    /// Ticks in a span of whole notes, truncated like LilyPond's
    /// `moment_to_ticks` (may be negative for a grace note before the start).
    fn ticks(&self, len: Frac) -> i64 {
        (len * Frac::from_integer(4 * self.ppq))
            .floor()
            .to_integer()
    }

    /// Tick at a position; never before the start.
    fn tick(&self, pos: Frac) -> u64 {
        self.ticks(pos).max(0) as u64
    }

    /// Note-on and note-off ticks: LilyPond puts the note-off its truncated
    /// length after the truncated note-on, and never at or before it.
    fn span(&self, start: Frac, end: Frac) -> (u64, u64) {
        let on = self.ticks(start);
        let off = on + self.ticks(end - start);
        let on = on.max(0) as u64;
        (on, (off.max(0) as u64).max(on + 1))
    }
}

// ---------------------------------------------------------------------------
// Bars and the order they are played in
// ---------------------------------------------------------------------------

/// Written length of a voice element; grace notes take no time.
fn metric_len(e: &VoiceElement) -> Frac {
    if is_grace(e) {
        return zero();
    }
    match e {
        VoiceElement::Note(n) => n.duration.actual_duration(),
        VoiceElement::Rest(r) => r.duration.actual_duration(),
        VoiceElement::Chord(c) => c.duration.actual_duration(),
    }
}

fn is_grace(e: &VoiceElement) -> bool {
    match e {
        VoiceElement::Note(n) => n.is_grace,
        VoiceElement::Chord(c) => c.notes.iter().any(|n| n.is_grace),
        VoiceElement::Rest(_) => false,
    }
}

/// How long a measure's music lasts: its longest voice.
fn content_len(m: &Measure) -> Frac {
    m.voices
        .iter()
        .map(|v| v.elements.iter().map(metric_len).sum::<Frac>())
        .max()
        .unwrap_or_else(zero)
}

/// Repeat and ending marks of one measure index, gathered over all parts.
#[derive(Clone, Default)]
struct Marks {
    forward: bool,
    backward: bool,
    times: Option<u8>,
    ending: Option<u8>,
    ending_end: bool,
}

/// The score's bars: each measure index's length, meter and key, and the
/// order the bars are played in with their start positions.
struct Bars {
    len: Vec<Frac>,
    meter: Vec<Option<TimeSignature>>,
    key: Vec<Option<KeySignature>>,
    /// Played measure indices.
    order: Vec<usize>,
    /// Start (whole notes) of each played bar.
    start: Vec<Frac>,
    /// Where the music ends.
    end: Frac,
}

impl Bars {
    fn new(parts: &[&Part], unfold: bool) -> Bars {
        let n = parts.iter().map(|p| p.measures.len()).max().unwrap_or(0);
        let (mut meter, mut key) = (None, None);
        let mut bars = Bars {
            len: Vec::with_capacity(n),
            meter: Vec::with_capacity(n),
            key: Vec::with_capacity(n),
            order: Vec::new(),
            start: Vec::new(),
            end: zero(),
        };
        let mut marks = vec![Marks::default(); n + 1];
        for i in 0..n {
            let at: Vec<&Measure> = parts.iter().filter_map(|p| p.measures.get(i)).collect();
            let attrs = || at.iter().filter_map(|m| m.attributes.as_ref());
            if let Some(t) = attrs().find_map(|a| a.time.clone()) {
                meter = Some(t);
            }
            if let Some(k) = attrs().find_map(|a| a.key) {
                key = Some(k);
            }
            let content = at.iter().map(|m| content_len(m)).max().unwrap_or_else(zero);
            let meter_len = meter
                .as_ref()
                .map(TimeSignature::beats_fraction)
                .filter(|l| *l > zero())
                .unwrap_or_else(|| Frac::from_integer(1));
            bars.len
                .push(if content > zero() { content } else { meter_len });
            bars.meter.push(meter.clone());
            bars.key.push(key);
            for m in &at {
                read_marks(m, i, &mut marks);
            }
        }
        bars.order = if unfold {
            played_order(&marks[..n])
        } else {
            (0..n).collect()
        };
        let mut pos = zero();
        for &i in &bars.order {
            bars.start.push(pos);
            pos += bars.len[i];
        }
        bars.end = pos;
        bars
    }
}

fn read_marks(m: &Measure, i: usize, marks: &mut [Marks]) {
    for (b, left) in [(&m.left_barline, true), (&m.right_barline, false)] {
        let Some(b) = b else { continue };
        let forward = b.repeat_direction == Some(RepeatDirection::Forward)
            || matches!(b.style, BarlineType::RepeatForward);
        let backward = b.repeat_direction == Some(RepeatDirection::Backward)
            || matches!(
                b.style,
                BarlineType::RepeatBackward | BarlineType::RepeatBoth
            );
        if forward {
            // A forward repeat on a right barline opens the next bar.
            marks[if left { i } else { i + 1 }].forward = true;
        }
        if backward && !left {
            marks[i].backward = true;
            if b.style == BarlineType::RepeatBoth {
                marks[i + 1].forward = true;
            }
        }
        if b.repeat_times.is_some() {
            marks[i].times = marks[i].times.or(b.repeat_times);
        }
        match b.ending_type.as_deref() {
            Some("start") if b.ending_number.is_some() => marks[i].ending = b.ending_number,
            Some("stop" | "discontinue") => marks[i].ending_end = true,
            _ => {}
        }
    }
}

/// The order bars are played in: volta repeats with their endings, MuseScore's
/// rule. An ending plays on its pass (the last one also on any later pass).
fn played_order(marks: &[Marks]) -> Vec<usize> {
    let n = marks.len();
    // Where each ending (starting at i) stops.
    let ending_end = |i: usize| -> usize {
        (i..n)
            .find(|&j| {
                marks[j].ending_end
                    || marks[j].backward
                    || (j + 1 < n && (marks[j + 1].ending.is_some() || marks[j + 1].forward))
            })
            .unwrap_or(n.saturating_sub(1))
    };
    // How many times the repeat opening at `start` plays.
    let times_from = |start: usize| -> u32 {
        let mut explicit = None;
        let mut last_ending = 0u32;
        for (j, m) in marks.iter().enumerate().skip(start) {
            if j > start && m.forward {
                break;
            }
            explicit = explicit.or(m.times);
            if let Some(e) = m.ending {
                last_ending = last_ending.max(e as u32);
            }
            if m.backward && m.times.is_some() {
                explicit = m.times;
            }
            if m.backward && last_ending == 0 && marks.get(j + 1).is_none_or(|x| x.ending.is_none())
            {
                break;
            }
        }
        explicit.map(u32::from).unwrap_or(last_ending.max(2))
    };
    let mut order = Vec::new();
    let (mut i, mut pass, mut start) = (0usize, 1u32, 0usize);
    let mut jumped = false;
    let limit = n * 16 + 16;
    while i < n && order.len() < limit {
        if marks[i].forward && !jumped {
            start = i;
            pass = 1;
        }
        jumped = false;
        if let Some(num) = marks[i].ending {
            let end = ending_end(i);
            let times = times_from(start);
            let last = (end + 1..n)
                .take_while(|&j| !marks[j].forward)
                .all(|j| marks[j].ending.is_none());
            // (Not one that closes with a repeat sign: `|: A [1 B :| C` plays
            // A B A C.)
            let plays = num as u32 == pass || (last && pass > num as u32 && !marks[end].backward);
            if !plays {
                i = end + 1;
                continue;
            }
            order.extend(i..=end);
            if marks[end].backward && pass < times {
                pass += 1;
                i = start;
                jumped = true;
                continue;
            }
            if num as u32 >= times || last {
                pass = 1;
                start = end + 1;
            }
            i = end + 1;
            continue;
        }
        order.push(i);
        if marks[i].backward {
            let times = times_from(start);
            if pass < times {
                pass += 1;
                i = start;
                jumped = true;
                continue;
            }
            pass = 1;
            start = i + 1;
        }
        i += 1;
    }
    order
}

// ---------------------------------------------------------------------------
// Tempo
// ---------------------------------------------------------------------------

/// Tempo changes in played order: (position, microseconds per quarter).
struct TempoMap {
    changes: Vec<(Frac, u32)>,
}

impl TempoMap {
    fn new(parts: &[&Part], bars: &Bars) -> TempoMap {
        let mut changes: Vec<(Frac, u32)> = Vec::new();
        for (k, &i) in bars.order.iter().enumerate() {
            for part in parts {
                let Some(m) = part.measures.get(i) else {
                    continue;
                };
                for d in &m.directions {
                    let Some(t) = &d.tempo else { continue };
                    let bpm = t.per_minute.unwrap_or(0.0);
                    if bpm <= 0.0 {
                        continue;
                    }
                    let quarters = beat_unit_to_quarters(t.beat_unit.as_deref(), t.dots);
                    let uspq = (60_000_000.0 / (bpm * quarters)).round() as u32;
                    changes.push((bars.start[k] + d.offset_frac, uspq.clamp(1, 0xFF_FFFF)));
                }
            }
        }
        changes.sort_by_key(|a| a.0);
        changes.dedup_by(|b, a| a.0 == b.0);
        if changes.first().is_none_or(|c| c.0 > zero()) {
            changes.insert(0, (zero(), 500_000));
        }
        TempoMap { changes }
    }

    /// How many whole notes `seconds` last at `pos`.
    fn wholes(&self, pos: Frac, seconds: f64) -> Frac {
        let uspq = self
            .changes
            .iter()
            .take_while(|c| c.0 <= pos)
            .last()
            .map_or(500_000, |c| c.1);
        let wholes = seconds * 1_000_000.0 / uspq as f64 / 4.0;
        Frac::new((wholes * 1_000_000.0).round() as i64, 1_000_000)
    }
}

/// A beat unit name + dots in quarter notes ("half" → 2, dotted quarter → 1.5).
fn beat_unit_to_quarters(beat_unit: Option<&str>, dots: u8) -> f64 {
    let base = match beat_unit.unwrap_or("quarter") {
        "breve" => 8.0,
        "whole" => 4.0,
        "half" => 2.0,
        "eighth" => 0.5,
        "16th" => 0.25,
        "32nd" => 0.125,
        "64th" => 0.0625,
        _ => 1.0,
    };
    (0..dots)
        .fold((base, base / 2.0), |(t, add), _| (t + add, add / 2.0))
        .0
}

// ---------------------------------------------------------------------------
// Conductor track
// ---------------------------------------------------------------------------

fn conductor(bars: &Bars, tempo: &TempoMap, clock: &Clock) -> Vec<Timed> {
    let mut out = vec![Timed::meta(
        0,
        Meta::Text(format!("creator: lytk {}", env!("CARGO_PKG_VERSION"))),
    )];
    for &(pos, uspq) in &tempo.changes {
        out.push(Timed::meta(clock.tick(pos), Meta::Tempo(uspq)));
    }
    let (mut last_time, mut last_key) = (None, None);
    for (k, &i) in bars.order.iter().enumerate() {
        let tick = clock.tick(bars.start[k]);
        let meter = bars.meter[i].as_ref();
        let meter_len = meter.map(TimeSignature::beats_fraction);
        // A bar shorter or longer than its meter says so with its own length.
        let time = if meter_len == Some(bars.len[i]) {
            meter.and_then(written_meter)
        } else {
            length_meter(bars.len[i], meter.map_or(4, |t| t.beat_type))
        };
        if let Some((n, d)) = time.filter(|_| time != last_time) {
            out.push(Timed::meta(tick, Meta::Time(n, d)));
            last_time = time;
        }
        if let Some(key) = bars.key[i] {
            let key = (key.fifths, key.mode == KeyMode::Minor);
            if Some(key) != last_key {
                out.push(Timed::meta(tick, Meta::Key(key.0, key.1)));
                last_key = Some(key);
            }
        }
    }
    out
}

/// (numerator, log2 denominator) of a time signature, if MIDI can hold it.
fn written_meter(ts: &TimeSignature) -> Option<(u8, u8)> {
    let num: u32 = ts
        .beats
        .split('+')
        .filter_map(|b| b.trim().parse::<u32>().ok())
        .sum();
    (ts.beat_type.is_power_of_two() && (1..=255).contains(&num))
        .then(|| (num as u8, ts.beat_type.trailing_zeros() as u8))
}

/// A time signature `len` long, counted in the meter's beat (or a finer one).
fn length_meter(len: Frac, beat_type: u8) -> Option<(u8, u8)> {
    let mut den = beat_type.max(1) as i64;
    while den <= 256 {
        let num = len * Frac::from_integer(den);
        if num.is_integer() && (1..=255).contains(num.numer()) && (den as u64).is_power_of_two() {
            return Some((*num.numer() as u8, (den as u64).trailing_zeros() as u8));
        }
        den *= 2;
    }
    None
}

// ---------------------------------------------------------------------------
// Channels and programs
// ---------------------------------------------------------------------------

fn is_percussion(part: &Part) -> bool {
    part.midi_channel == 10
        || part.measures.iter().any(|m| {
            m.attributes
                .as_ref()
                .is_some_and(|a| a.clefs.values().any(|c| c.sign == ClefSign::Percussion))
        })
}

fn program_of(part: &Part) -> u8 {
    if part.midi_program != 0 {
        part.midi_program.min(127)
    } else {
        super::gm::gm_program_from_name(&part.midi_instrument).unwrap_or(0)
    }
}

/// One MIDI channel (0-based) per part. `Part.midi_channel` is 1–16 (0 = not
/// set), as in MusicXML. Percussion plays on channel 10; the others take free
/// channels around it, and past 15 parts share one with a part of the same
/// program (LilyPond's `midiChannelMapping = #'instrument`).
fn assign_channels(parts: &[&Part]) -> Vec<u8> {
    let mut used = [false; 16];
    used[9] = true;
    for p in parts {
        if (1..=16).contains(&p.midi_channel) && !is_percussion(p) {
            used[p.midi_channel as usize - 1] = true;
        }
    }
    let mut by_program: HashMap<u8, u8> = HashMap::new();
    let mut round = 0usize;
    parts
        .iter()
        .map(|p| {
            let program = program_of(p);
            let ch = if is_percussion(p) {
                9
            } else if (1..=16).contains(&p.midi_channel) {
                p.midi_channel - 1
            } else if let Some(c) = (0..16u8).find(|&c| !used[c as usize]) {
                used[c as usize] = true;
                c
            } else if let Some(&c) = by_program.get(&program) {
                c
            } else {
                let melodic: Vec<u8> = (0..16u8).filter(|&c| c != 9).collect();
                round += 1;
                melodic[(round - 1) % melodic.len()]
            };
            if ch != 9 {
                by_program.entry(program).or_insert(ch);
            }
            ch
        })
        .collect()
}

fn track_name(part: &Part, staff: u8, staves: u8) -> String {
    let name = if part.name.is_empty() {
        &part.part_id
    } else {
        &part.name
    };
    if staves > 1 {
        format!("{name} {staff}")
    } else {
        name.clone()
    }
}

// ---------------------------------------------------------------------------
// Playing one staff of a part
// ---------------------------------------------------------------------------

/// A sounding note while a staff is played: positions in whole notes.
#[derive(Clone, Debug)]
struct Sound {
    start: Frac,
    end: Frac,
    /// Where the written note ends (ties join at written ends).
    written_end: Frac,
    key: u8,
    lane: u8,
    own_velocity: Option<u8>,
    extra_velocity: i32,
    tie_start: bool,
    tie_stop: bool,
    grace: bool,
    /// A staccato or portato keeps this share of the note (a staccato no
    /// more than until the position given): a tied note keeps it over the
    /// whole chain.
    shorten: Option<(Frac, Option<Frac>)>,
}

/// A dynamic event a voice's volume follows.
#[derive(Clone, Copy, Debug)]
enum Dyn {
    Level(f64),
    Hairpin(i8),
    HairpinStop,
}

/// Per-voice state while playing.
#[derive(Default)]
struct Lane {
    /// Indices (in `sounds`) of the voice's last main note or chord.
    last_main: Vec<usize>,
    /// Grace notes waiting for their main note.
    graces: Vec<Vec<(u8, Frac, Option<u8>)>>,
    /// After-graces waiting to be placed at the end of the last main note.
    after: Vec<Vec<(u8, Frac, Option<u8>)>>,
}

struct Player<'a> {
    part: &'a Part,
    staff: u8,
    staves: u8,
    bars: &'a Bars,
    tempo: &'a TempoMap,
    default_velocity: Option<u8>,
    end: Frac,
}

impl Player<'_> {
    fn voice_staff(&self, v: &Voice) -> u8 {
        if self.staves <= 1 {
            return self.staff;
        }
        v.elements
            .iter()
            .find(|e| !matches!(e, VoiceElement::Rest(r) if r.is_spacer))
            .or(v.elements.first())
            .map(|e| match e {
                VoiceElement::Note(n) => n.staff,
                VoiceElement::Rest(r) => r.staff,
                VoiceElement::Chord(c) => c.staff,
            })
            .filter(|s| *s > 0)
            .unwrap_or(1)
    }

    fn on_staff(&self, staff: u8) -> bool {
        self.staves <= 1 || staff == 0 || staff == self.staff
    }

    fn play(&self, channel: u8, clock: &Clock) -> Vec<Timed> {
        let mut sounds: Vec<Sound> = Vec::new();
        // (position, lane or all, event), in play order.
        let mut dyns: Vec<(Frac, Option<u8>, Dyn)> = Vec::new();
        let mut out: Vec<Timed> = Vec::new();
        let mut lanes: HashMap<u8, Lane> = HashMap::new();
        let mut transpose = 0i32;

        for (k, &mi) in self.bars.order.iter().enumerate() {
            let Some(m) = self.part.measures.get(mi) else {
                continue;
            };
            let bar = self.bars.start[k];
            if let Some(t) = m.attributes.as_ref().and_then(|a| a.transpose.as_ref()) {
                transpose = t.chromatic as i32 + 12 * t.octave_change as i32;
            }
            for d in m.directions.iter().filter(|d| self.on_staff(d.staff)) {
                let at = bar + d.offset_frac;
                if let Some(dm) = &d.dynamic {
                    dyns.push((at, None, Dyn::Level(lilypond_volume(&dm.sign))));
                }
                if let Some(w) = &d.wedge {
                    dyns.push((at, None, wedge(&w.wedge_type)));
                }
                if let Some(p) = &d.pedal {
                    let tick = clock.tick(at);
                    let cc = |v: u8| Timed {
                        tick,
                        rank: 2,
                        ev: Ev::Cc(channel, 64, v),
                    };
                    match p.pedal_type.as_str() {
                        "start" => out.push(cc(127)),
                        "stop" => out.push(cc(0)),
                        "change" => {
                            out.push(cc(0));
                            out.push(cc(127));
                        }
                        _ => {}
                    }
                }
            }
            for v in m
                .voices
                .iter()
                .filter(|v| self.voice_staff(v) == self.staff)
            {
                let lane_no = v.number;
                let lane = lanes.entry(lane_no).or_default();
                let mut pos = bar;
                for e in &v.elements {
                    if is_grace(e) {
                        let group = grace_group(e, transpose);
                        let after = matches!(e, VoiceElement::Note(n) if n.after_grace)
                            || matches!(e, VoiceElement::Chord(c) if c.notes.iter().any(|n| n.after_grace));
                        if after {
                            lane.after.push(group);
                        } else {
                            lane.graces.push(group);
                        }
                        continue;
                    }
                    place_after_graces(lane, lane_no, &mut sounds);
                    place_graces(lane, lane_no, pos, &mut sounds);
                    let len = metric_len(e);
                    match e {
                        VoiceElement::Rest(r) => {
                            for dm in &r.dynamics {
                                dyns.push((
                                    pos,
                                    Some(lane_no),
                                    Dyn::Level(lilypond_volume(&dm.sign)),
                                ));
                            }
                            for w in &r.wedges {
                                dyns.push((pos, Some(lane_no), wedge(&w.wedge_type)));
                            }
                        }
                        VoiceElement::Note(n) => {
                            push_lyric(&mut out, clock.tick(pos), &n.lyrics);
                            let arts: Vec<&Articulation> = n.articulations.iter().collect();
                            let i = self.sound(
                                n,
                                &arts,
                                pos,
                                len,
                                lane_no,
                                transpose,
                                &mut sounds,
                                &mut dyns,
                            );
                            lane.last_main = vec![i];
                        }
                        VoiceElement::Chord(c) => {
                            if let Some(n) = c.notes.first() {
                                push_lyric(&mut out, clock.tick(pos), &n.lyrics);
                            }
                            // A chord's articulations (stored on any of its notes)
                            // apply to all of them, as in LilyPond.
                            let mut arts: Vec<&Articulation> = Vec::new();
                            for a in c.notes.iter().flat_map(|n| &n.articulations) {
                                if !arts.iter().any(|b| b.name == a.name) {
                                    arts.push(a);
                                }
                            }
                            let mut idx = Vec::new();
                            for n in &c.notes {
                                idx.push(self.sound(
                                    n,
                                    &arts,
                                    pos,
                                    len,
                                    lane_no,
                                    transpose,
                                    &mut sounds,
                                    &mut dyns,
                                ));
                            }
                            lane.last_main = idx;
                        }
                    }
                    pos += len;
                }
                place_after_graces(lane, lane_no, &mut sounds);
            }
        }

        let sounds = join_ties(sounds);
        let eq = lilypond_equalizer(&self.part.midi_instrument);
        let default = match self.default_velocity {
            Some(v) => v as f64 / 127.0,
            None => equalize(eq, LILYPOND_DEFAULT_VOLUME),
        };
        let mut curves: HashMap<u8, Curve> = HashMap::new();
        let mut notes: Vec<(u64, u64, u8, u8)> = Vec::with_capacity(sounds.len());
        for s in &sounds {
            let curve = curves.entry(s.lane).or_insert_with(|| {
                let mut evs: Vec<(Frac, Dyn)> = dyns
                    .iter()
                    .filter(|(_, l, _)| l.is_none_or(|l| l == s.lane))
                    .map(|(p, _, d)| (*p, *d))
                    .collect();
                evs.sort_by_key(|a| a.0);
                Curve::new(&evs, default, eq, self.end)
            });
            let vel = match s.own_velocity {
                Some(v) => v as i32,
                None => (curve.at(s.start) * 127.0) as i32 + s.extra_velocity,
            };
            let (on, off) = clock.span(s.start, s.end);
            notes.push((on, off, s.key, vel.clamp(1, 127) as u8));
        }
        for (on, off, key, vel) in merge_unisons(notes) {
            out.push(Timed {
                tick: on,
                rank: 3,
                ev: Ev::On(channel, key, vel),
            });
            out.push(Timed {
                tick: off,
                rank: 1,
                ev: Ev::Off(channel, key),
            });
        }
        out
    }

    /// Record a main note starting at `pos`: its articulated length and
    /// velocity offset, its dynamics and hairpins, and its lyric.
    #[allow(clippy::too_many_arguments)]
    fn sound(
        &self,
        n: &Note,
        articulations: &[&Articulation],
        pos: Frac,
        len: Frac,
        lane: u8,
        transpose: i32,
        sounds: &mut Vec<Sound>,
        dyns: &mut Vec<(Frac, Option<u8>, Dyn)>,
    ) -> usize {
        for dm in &n.dynamics {
            dyns.push((pos, Some(lane), Dyn::Level(lilypond_volume(&dm.sign))));
        }
        for w in &n.wedges {
            dyns.push((pos, Some(lane), wedge(&w.wedge_type)));
        }
        // LilyPond's `midi-length` procedures, applied in order, and
        // `midi-extra-velocity` summed (ly/script-init.ly).
        let mut sounding = len;
        let mut extra = 0;
        let mut shorten = None;
        for a in articulations {
            match a.name.as_str() {
                "staccato" => {
                    let cap = self.tempo.wholes(pos, 0.5);
                    sounding = (sounding / Frac::from_integer(2)).min(cap);
                    shorten = Some((Frac::new(1, 2), Some(pos + cap)));
                    extra += 4;
                }
                "staccatissimo" => {
                    sounding = self.tempo.wholes(pos, 0.125);
                    extra += 6;
                }
                "detached-legato" | "portato" => {
                    sounding *= Frac::new(3, 4);
                    shorten = Some((Frac::new(3, 4), None));
                }
                "accent" => extra += 20,
                "strong-accent" | "marcato" => extra += 40,
                _ => {}
            }
        }
        sounds.push(Sound {
            start: pos,
            end: pos + sounding,
            written_end: pos + len,
            key: key_of(n, transpose),
            lane,
            own_velocity: n.velocity,
            extra_velocity: extra,
            tie_start: n.ties.iter().any(|t| t.tie_type == StartStop::Start),
            tie_stop: n.ties.iter().any(|t| t.tie_type == StartStop::Stop),
            grace: false,
            shorten,
        });
        sounds.len() - 1
    }
}

/// A note's first-verse syllable as a lyric event (as LilyPond's lyric
/// performer writes it); a hyphenated syllable keeps its hyphen.
fn push_lyric(out: &mut Vec<Timed>, tick: u64, lyrics: &[LyricSyllable]) {
    let Some(l) = lyrics.iter().min_by_key(|l| l.number) else {
        return;
    };
    if l.text.is_empty() {
        return;
    }
    let hyphen = matches!(l.syllabic, SyllabicType::Begin | SyllabicType::Middle);
    let text = if hyphen {
        format!("{}-", l.text)
    } else {
        l.text.clone()
    };
    out.push(Timed::meta(tick, Meta::Lyric(text)));
}

fn key_of(n: &Note, transpose: i32) -> u8 {
    (n.pitch.midi_number() + transpose).clamp(0, 127) as u8
}

fn wedge(kind: &str) -> Dyn {
    match kind {
        "crescendo" => Dyn::Hairpin(1),
        "diminuendo" | "decrescendo" => Dyn::Hairpin(-1),
        _ => Dyn::HairpinStop,
    }
}

/// The notes of a grace element: (key, written length, own velocity).
fn grace_group(e: &VoiceElement, transpose: i32) -> Vec<(u8, Frac, Option<u8>)> {
    match e {
        VoiceElement::Note(n) => vec![(
            key_of(n, transpose),
            n.duration.actual_duration(),
            n.velocity,
        )],
        VoiceElement::Chord(c) => c
            .notes
            .iter()
            .map(|n| {
                (
                    key_of(n, transpose),
                    c.duration.actual_duration(),
                    n.velocity,
                )
            })
            .collect(),
        VoiceElement::Rest(_) => Vec::new(),
    }
}

/// Place a lane's pending grace notes before the beat at `beat`: each sounds
/// for 9/40 of its written length, the group ending on the beat, and the note
/// before them is cut where they begin (LilyPond's note performer).
fn place_graces(lane: &mut Lane, lane_no: u8, beat: Frac, sounds: &mut Vec<Sound>) {
    if lane.graces.is_empty() {
        return;
    }
    let groups = std::mem::take(&mut lane.graces);
    let total: Frac = groups
        .iter()
        .map(|g| g.first().map_or(zero(), |n| n.1))
        .sum();
    let begin = beat - total * grace_factor();
    for &i in &lane.last_main {
        if sounds[i].start < begin && sounds[i].end > begin {
            sounds[i].end = begin;
        }
    }
    let mut t = begin;
    for group in groups {
        let len = group.first().map_or(zero(), |n| n.1) * grace_factor();
        for (key, _, own) in group {
            sounds.push(Sound {
                start: t,
                end: t + len,
                written_end: t + len,
                key,
                lane: lane_no,
                own_velocity: own,
                extra_velocity: 0,
                tie_start: false,
                tie_stop: false,
                grace: true,
                shorten: None,
            });
        }
        t += len;
    }
}

/// Place after-graces (`\afterGrace`) like grace notes leading to the point
/// three quarters through the note they follow (LilyPond's
/// `afterGraceFraction`).
fn place_after_graces(lane: &mut Lane, lane_no: u8, sounds: &mut Vec<Sound>) {
    if lane.after.is_empty() {
        return;
    }
    let Some(&main) = lane.last_main.first() else {
        lane.after.clear();
        return;
    };
    let s = &sounds[main];
    let at = s.start + (s.written_end - s.start) * Frac::new(3, 4);
    let pending = std::mem::replace(&mut lane.graces, std::mem::take(&mut lane.after));
    place_graces(lane, lane_no, at, sounds);
    lane.graces = pending;
}

/// One channel can't sound a key twice. As LilyPond's MIDI walker does: two
/// notes of a key starting together become one, lasting to the later end; a
/// note starting while its key sounds cuts that note off and lasts to the later
/// of the two ends. Notes are `(on, off, key, velocity)` ticks.
fn merge_unisons(mut notes: Vec<(u64, u64, u8, u8)>) -> Vec<(u64, u64, u8, u8)> {
    notes.sort_by_key(|n| (n.0, n.2, std::cmp::Reverse(n.1)));
    let mut out: Vec<(u64, u64, u8, u8)> = Vec::with_capacity(notes.len());
    let mut sounding: HashMap<u8, usize> = HashMap::new();
    for (on, off, key, vel) in notes {
        if let Some(&i) = sounding.get(&key) {
            if out[i].1 > on {
                if out[i].0 == on {
                    out[i].1 = out[i].1.max(off);
                    continue;
                }
                let end = out[i].1.max(off);
                out[i].1 = on;
                out.push((on, end, key, vel));
                sounding.insert(key, out.len() - 1);
                continue;
            }
        }
        out.push((on, off, key, vel));
        sounding.insert(key, out.len() - 1);
    }
    out
}

/// Join tied notes into one sound per chain: a note tied onward is extended by
/// the next note of the same key in its voice that starts where it (as
/// written) ends — or later, when that note says the tie stops there
/// (LilyPond's `tieWaitForNote`, MusicXML's explicit tie stops). A tie that
/// goes nowhere just ends.
fn join_ties(mut sounds: Vec<Sound>) -> Vec<Sound> {
    let mut order: Vec<usize> = (0..sounds.len()).collect();
    order.sort_by(|&a, &b| {
        (sounds[a].lane, sounds[a].start, sounds[a].grace)
            .cmp(&(sounds[b].lane, sounds[b].start, sounds[b].grace))
            .then(a.cmp(&b))
    });
    let mut open: HashMap<(u8, u8), usize> = HashMap::new();
    let mut gone = vec![false; sounds.len()];
    for &i in &order {
        if sounds[i].grace {
            continue;
        }
        let k = (sounds[i].lane, sounds[i].key);
        if let Some(&head) = open.get(&k) {
            let at_end = sounds[head].written_end == sounds[i].start;
            let waited = sounds[i].tie_stop && sounds[i].start >= sounds[head].written_end;
            if at_end || waited {
                let written = sounds[i].written_end;
                sounds[head].end = match sounds[head].shorten {
                    Some((share, cap)) => {
                        let end = sounds[head].start + (written - sounds[head].start) * share;
                        cap.map_or(end, |c| end.min(c))
                    }
                    None => sounds[i].end,
                };
                sounds[head].written_end = written;
                gone[i] = true;
                if !sounds[i].tie_start {
                    open.remove(&k);
                }
                continue;
            }
            open.remove(&k);
        }
        if sounds[i].tie_start {
            open.insert(k, i);
        }
    }
    sounds
        .into_iter()
        .zip(gone)
        .filter(|(_, g)| !g)
        .map(|(s, _)| s)
        .collect()
}

// ---------------------------------------------------------------------------
// Dynamics: LilyPond's volume curve
// ---------------------------------------------------------------------------

fn equalize(eq: Option<(f64, f64)>, volume: f64) -> f64 {
    match eq {
        Some((lo, hi)) => (lo + (hi - lo) * volume).clamp(0.0, 1.0),
        None => volume,
    }
}

/// A voice's volume over time: points where it is set, joined by holds or by
/// the straight lines of hairpins.
struct Curve {
    /// (position, volume there, the line to the next point is a ramp).
    points: Vec<(Frac, f64, bool)>,
}

impl Curve {
    /// Build the curve as LilyPond's dynamic performer does: a dynamic sets the
    /// volume; the hairpins between two dynamics share the change between them
    /// in proportion to their lengths; a hairpin with no dynamic after it (or
    /// one going the other way) moves a quarter of the dynamic range; a
    /// crescendo turning into a diminuendo peaks between the two.
    fn new(events: &[(Frac, Dyn)], default: f64, eq: Option<(f64, f64)>, end: Frac) -> Curve {
        let lo = equalize(eq, 0.1);
        let hi = equalize(eq, 1.0);
        let mut points = vec![(zero(), default, false)];
        let mut cur = default;
        // Hairpins since the last dynamic: (start, end, direction).
        let mut group: Vec<(Frac, Frac, i8)> = Vec::new();
        let mut open: Option<(Frac, i8)> = None;
        for &(pos, ev) in events {
            match ev {
                Dyn::Hairpin(dir) => {
                    if let Some((s, d)) = open.take() {
                        group.push((s, pos, d));
                    }
                    open = Some((pos, dir));
                }
                Dyn::HairpinStop => {
                    if let Some((s, d)) = open.take() {
                        group.push((s, pos, d));
                    }
                }
                Dyn::Level(v) => {
                    let v = equalize(eq, v);
                    if let Some((s, d)) = open.take() {
                        group.push((s, pos, d));
                    }
                    if !group.is_empty() {
                        resolve(&mut points, &group, cur, Some(v), lo, hi);
                        group.clear();
                    }
                    points.push((pos, v, false));
                    cur = v;
                }
            }
        }
        if let Some((s, d)) = open.take() {
            group.push((s, end.max(s), d));
        }
        if !group.is_empty() {
            resolve(&mut points, &group, cur, None, lo, hi);
        }
        points.sort_by_key(|a| a.0);
        Curve { points }
    }

    fn at(&self, pos: Frac) -> f64 {
        let i = self.points.partition_point(|p| p.0 <= pos);
        if i == 0 {
            return self.points[0].1;
        }
        let (p0, v0, ramp) = self.points[i - 1];
        match self.points.get(i) {
            Some(&(p1, v1, _)) if ramp && p1 > p0 => {
                let f = (pos - p0) / (p1 - p0);
                v0 + (v1 - v0) * (*f.numer() as f64 / *f.denom() as f64)
            }
            _ => v0,
        }
    }
}

/// Give a run of hairpins between two dynamics their volumes (LilyPond's
/// `finish_queued_spans`): from `start` to the next dynamic `next`, or to a
/// departure volume when there is none or it goes the other way.
fn resolve(
    points: &mut Vec<(Frac, f64, bool)>,
    group: &[(Frac, Frac, i8)],
    start: f64,
    next: Option<f64>,
    lo: f64,
    hi: f64,
) {
    let dir = group[0].2;
    let turn = group.iter().position(|h| h.2 != dir).unwrap_or(group.len());
    let (depart, back) = group.split_at(turn);
    if back.is_empty() {
        let target = match next {
            Some(v) if (v - start).signum() as i8 == dir && v != start => v,
            _ => departure(dir, start, start, lo, hi),
        };
        ramp(points, depart, start, target);
    } else {
        let end = next.unwrap_or(start);
        let peak = departure(dir, start, end, lo, hi);
        ramp(points, depart, start, peak);
        ramp(points, back, peak, end);
    }
}

/// Spread the change `from → to` over hairpins in proportion to their lengths.
fn ramp(points: &mut Vec<(Frac, f64, bool)>, hairpins: &[(Frac, Frac, i8)], from: f64, to: f64) {
    let total: Frac = hairpins.iter().map(|h| h.1 - h.0).sum();
    let mut done = zero();
    let mut vol = from;
    for &(s, e, _) in hairpins {
        done += e - s;
        let share = if total > zero() {
            *done.numer() as f64
                / *done.denom() as f64
                / (*total.numer() as f64 / *total.denom() as f64)
        } else {
            1.0
        };
        let next = from + (to - from) * share;
        points.push((s, vol, true));
        points.push((e, next, false));
        vol = next;
    }
}

/// LilyPond's `calc_departure_volume`: a volume reasonably far from `start`
/// and `end` in direction `dir`, within the dynamic range.
fn departure(dir: i8, start: f64, end: f64, lo: f64, hi: f64) -> f64 {
    let lo = lo.min(start).min(end);
    let hi = hi.max(start).max(end);
    let range = hi - lo;
    let d = dir as f64;
    let pick = |dir: f64, a: f64, b: f64| if dir > 0.0 { a.max(b) } else { a.min(b) };
    let near = pick(d, start, end) + d * 0.07 * range;
    let far = pick(-d, start, end) + d * 0.25 * range;
    pick(d, near, far).clamp(lo, hi)
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Meta {
    Text(String),
    TrackName(String),
    Instrument(String),
    Lyric(String),
    Tempo(u32),
    Time(u8, u8),
    Key(i8, bool),
}

#[derive(Clone, Debug)]
enum Ev {
    Meta(Meta),
    Program(u8, u8),
    Cc(u8, u8, u8),
    On(u8, u8, u8),
    Off(u8, u8),
}

/// An event at a tick. At one tick: metas (0), note-offs (1), controllers
/// and programs (2), note-ons (3).
#[derive(Clone, Debug)]
struct Timed {
    tick: u64,
    rank: u8,
    ev: Ev,
}

impl Timed {
    fn meta(tick: u64, m: Meta) -> Timed {
        Timed {
            tick,
            rank: 0,
            ev: Ev::Meta(m),
        }
    }
}

fn encode(tracks: &[Vec<Timed>], ppq: u16) -> Result<Vec<u8>> {
    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(ppq)),
    ));
    for track in tracks {
        let mut evs: Vec<&Timed> = track.iter().collect();
        evs.sort_by_key(|t| (t.tick, t.rank));
        let mut out: Vec<TrackEvent> = Vec::with_capacity(evs.len() + 1);
        let mut last = 0u64;
        for t in evs {
            let kind = match &t.ev {
                Ev::Meta(m) => TrackEventKind::Meta(match m {
                    Meta::Text(s) => MetaMessage::Text(s.as_bytes()),
                    Meta::TrackName(s) => MetaMessage::TrackName(s.as_bytes()),
                    Meta::Instrument(s) => MetaMessage::InstrumentName(s.as_bytes()),
                    Meta::Lyric(s) => MetaMessage::Lyric(s.as_bytes()),
                    Meta::Tempo(us) => MetaMessage::Tempo(u24::new(*us)),
                    Meta::Time(n, d) => MetaMessage::TimeSignature(*n, *d, 24, 8),
                    Meta::Key(sf, minor) => MetaMessage::KeySignature(*sf, *minor),
                }),
                Ev::Program(ch, p) => TrackEventKind::Midi {
                    channel: u4::new(*ch),
                    message: MidiMessage::ProgramChange {
                        program: u7::new(*p),
                    },
                },
                Ev::Cc(ch, c, v) => TrackEventKind::Midi {
                    channel: u4::new(*ch),
                    message: MidiMessage::Controller {
                        controller: u7::new(*c),
                        value: u7::new(*v),
                    },
                },
                Ev::On(ch, k, v) => TrackEventKind::Midi {
                    channel: u4::new(*ch),
                    message: MidiMessage::NoteOn {
                        key: u7::new(*k),
                        vel: u7::new(*v),
                    },
                },
                Ev::Off(ch, k) => TrackEventKind::Midi {
                    channel: u4::new(*ch),
                    message: MidiMessage::NoteOff {
                        key: u7::new(*k),
                        vel: u7::new(64),
                    },
                },
            };
            let delta = u32::try_from(t.tick - last)
                .ok()
                .filter(|d| *d < (1 << 28))
                .ok_or_else(|| AdapterError::Parse("MIDI delta time out of range".into()))?;
            out.push(TrackEvent {
                delta: u28::new(delta),
                kind,
            });
            last = t.tick;
        }
        out.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(out);
    }
    let mut buf = Vec::new();
    smf.write(&mut buf)
        .map_err(|e| AdapterError::Parse(format!("MIDI write error: {e}")))?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::measure::{KeySignature, Measure, MeasureAttributes, TimeSignature};
    use crate::ir::note::{Note, Rest};
    use crate::ir::part::Part;
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{ScoreChild, ScoreMetadata};
    use crate::ir::voice::Voice;

    /// Build a minimal 1-measure score with one note.
    fn one_note_score(pitch: Pitch, dur: Duration) -> Score {
        let note = Note::new(pitch, dur.clone());
        let rest = Rest::new(Duration {
            base: Frac::new(3, 4),
            dots: 0,
            tuplet_normal: 1,
            tuplet_actual: 1,
        });
        let mut voice = Voice::new(1);
        voice.elements.push(VoiceElement::Note(Box::new(note)));
        voice.elements.push(VoiceElement::Rest(rest));
        let mut measure = Measure::new(1);
        measure.attributes = Some(MeasureAttributes {
            divisions: 480,
            time: Some(TimeSignature::default()),
            key: Some(KeySignature::default()),
            ..MeasureAttributes::default()
        });
        measure.voices.push(voice);
        let mut part = Part::new("P1");
        part.name = "Test".to_string();
        part.measures.push(measure);
        Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        }
    }

    #[test]
    fn test_conductor_aggregates_time_sig_from_any_part() {
        // The time/tempo can live on a part other than the first (e.g.
        // example.ly, where only the Corno staff declares \time 4/4). The
        // conductor track must still pick it up.
        // Part 0: a half + quarter (no time/key/tempo declared).
        let mut v0 = Voice::new(1);
        v0.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::half(),
        ))));
        v0.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::D, 4),
            Duration::quarter(),
        ))));
        let mut m0 = Measure::new(1);
        m0.voices.push(v0);
        let mut p0 = Part::new("P1");
        p0.measures.push(m0);

        // Part 1: declares 3/4 + a tempo, three quarters.
        let mut v1 = Voice::new(1);
        for step in [PitchStep::E, PitchStep::F, PitchStep::G] {
            v1.elements.push(VoiceElement::Note(Box::new(Note::new(
                Pitch::new(step, 4),
                Duration::quarter(),
            ))));
        }
        let mut m1 = Measure::new(1);
        m1.attributes = Some(MeasureAttributes {
            divisions: 384,
            time: Some(TimeSignature {
                beats: "3".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            ..MeasureAttributes::default()
        });
        m1.directions.push(crate::ir::direction::Direction {
            tempo: Some(crate::ir::direction::TempoDirection {
                text: None,
                beat_unit: Some("quarter".to_string()),
                per_minute: Some(90.0),
                dots: 0,
                placement: crate::ir::articulation::Placement::Above,
            }),
            ..crate::ir::direction::Direction::default()
        });
        m1.voices.push(v1);
        let mut p1 = Part::new("P2");
        p1.measures.push(m1);

        let score = Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children: vec![ScoreChild::Part(p0), ScoreChild::Part(p1)],
        };

        let bytes = IrToMidiAdapter::new().convert_bytes(&score).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let mut saw_time = false;
        let mut saw_tempo = false;
        for ev in &smf.tracks[0] {
            if let TrackEventKind::Meta(MetaMessage::TimeSignature(num, den_pow, ..)) = ev.kind {
                if num == 3 && den_pow == 2 {
                    saw_time = true;
                }
            }
            if let TrackEventKind::Meta(MetaMessage::Tempo(uspq)) = ev.kind {
                // 90 BPM quarter = 666_667 µs/quarter (not the 120 default).
                if (uspq.as_int() as i64 - 666_667).abs() < 50 {
                    saw_tempo = true;
                }
            }
        }
        assert!(saw_time, "conductor track missing 3/4 time sig from part 2");
        assert!(saw_tempo, "conductor track missing tempo from part 2");
    }

    #[test]
    fn test_convert_bytes_produces_valid_midi() {
        let score = one_note_score(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let adapter = IrToMidiAdapter::new();
        let bytes = adapter.convert_bytes(&score).unwrap();

        // Must start with MIDI header "MThd"
        assert_eq!(&bytes[0..4], b"MThd");

        // Parse back with midly to verify it's valid.
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.header.format, Format::Parallel);
        // 2 tracks: conductor + 1 part
        assert_eq!(smf.tracks.len(), 2);
    }

    #[test]
    fn test_roundtrip_midi() {
        // Build score → MIDI → parse back → verify note count
        let score = one_note_score(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let adapter = IrToMidiAdapter::new();
        let bytes = adapter.convert_bytes(&score).unwrap();

        // Parse back
        let reader = crate::adapters::midi_to_ir::MidiToIrAdapter::new();
        let score2 = reader.convert_bytes(&bytes).unwrap();

        // Should have at least 1 part with 1 measure
        assert!(!score2.parts().is_empty());
        let parts = score2.parts();
        let part = parts[0];
        assert!(!part.measures.is_empty());

        // First measure should contain a note with E4
        let m = &part.measures[0];
        assert!(!m.voices.is_empty());
        let has_e4 = m.voices[0].elements.iter().any(|e| {
            if let VoiceElement::Note(n) = e {
                n.pitch.step == PitchStep::E && n.pitch.octave == 4
            } else {
                false
            }
        });
        assert!(has_e4, "Expected E4 note in roundtrip");
    }

    #[test]
    fn test_grace_note_does_not_steal_metrical_time() {
        // One 4/4 bar: grace eighth (C5) then D4 E4 F4 G4 quarters. The grace
        // must not push the metrical notes later — D4 stays at tick 0 and G4 at
        // three quarters. (Before the fix the grace consumed a quarter, shifting
        // every later note and overflowing the bar.)
        let mut grace = Note::new(Pitch::new(PitchStep::C, 5), Duration::eighth());
        grace.is_grace = true;
        let q =
            |s, o| VoiceElement::Note(Box::new(Note::new(Pitch::new(s, o), Duration::quarter())));
        let mut voice = Voice::new(1);
        voice.elements.push(VoiceElement::Note(Box::new(grace)));
        voice.elements.push(q(PitchStep::D, 4));
        voice.elements.push(q(PitchStep::E, 4));
        voice.elements.push(q(PitchStep::F, 4));
        voice.elements.push(q(PitchStep::G, 4));
        let mut measure = Measure::new(1);
        measure.attributes = Some(MeasureAttributes {
            time: Some(TimeSignature::default()),
            ..MeasureAttributes::default()
        });
        measure.voices.push(voice);
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let score = Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        };

        let adapter = IrToMidiAdapter::new(); // 384 ticks/quarter
        let quarter = 384u32;
        let bytes = adapter.convert_bytes(&score).unwrap();
        let smf = Smf::parse(&bytes).unwrap();

        // Collect (pitch, absolute tick) for every real note-on.
        let mut onsets: Vec<(u8, u32)> = Vec::new();
        for track in &smf.tracks {
            let mut t = 0u32;
            for ev in track {
                t += ev.delta.as_int();
                if let TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { key, vel },
                    ..
                } = ev.kind
                {
                    if vel.as_int() > 0 {
                        onsets.push((key.as_int(), t));
                    }
                }
            }
        }
        let onset_of = |k: u8| onsets.iter().find(|(p, _)| *p == k).map(|(_, t)| *t);
        // D4=62 (right after the grace) must start at tick 0.
        assert_eq!(onset_of(62), Some(0), "grace stole time: D4 not at tick 0");
        // G4=67 must start at 3 quarters (one bar holds all four quarters).
        assert_eq!(
            onset_of(67),
            Some(3 * quarter),
            "bar overflowed past the grace"
        );
    }

    #[test]
    fn test_running_time_signature_sizes_later_bars() {
        // Three 3/8 bars, time signature declared only on bar 1 (the common case
        // — `\time 3/8` once, then bars inherit it). Each later bar must be 3/8
        // long (576 ticks at 384/quarter), NOT padded to the 4/4 default; before
        // the carried-meter fix, bar 1 padded to 1536 and every later onset
        // shifted, breaking the MIDI round-trip for non-4/4 pieces (pedal).
        let dotted_q = || Duration::dotted(Frac::new(1, 4), 1); // 3/8 of a whole
        let note = |s, o| VoiceElement::Note(Box::new(Note::new(Pitch::new(s, o), dotted_q())));

        let mut bars = Vec::new();
        for (i, (s, o)) in [(PitchStep::C, 4), (PitchStep::D, 4), (PitchStep::E, 4)]
            .into_iter()
            .enumerate()
        {
            let mut v = Voice::new(1);
            v.elements.push(note(s, o));
            let mut m = Measure::new(i as u32 + 1);
            if i == 0 {
                m.attributes = Some(MeasureAttributes {
                    time: Some(TimeSignature {
                        beats: "3".to_string(),
                        beat_type: 8,
                        symbol: None,
                    }),
                    ..MeasureAttributes::default()
                });
            }
            m.voices.push(v);
            bars.push(m);
        }
        let mut part = Part::new("P1");
        part.measures = bars;
        let score = Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        };

        let bytes = IrToMidiAdapter::new().convert_bytes(&score).unwrap(); // 384/quarter
        let smf = Smf::parse(&bytes).unwrap();
        let mut onsets: Vec<(u8, u32)> = Vec::new();
        for track in &smf.tracks {
            let mut t = 0u32;
            for ev in track {
                t += ev.delta.as_int();
                if let TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { key, vel },
                    ..
                } = ev.kind
                {
                    if vel.as_int() > 0 {
                        onsets.push((key.as_int(), t));
                    }
                }
            }
        }
        let onset_of = |k: u8| onsets.iter().find(|(p, _)| *p == k).map(|(_, t)| *t);
        let bar = 576u32; // 3/8 at 384 ticks/quarter
        assert_eq!(onset_of(60), Some(0), "C4 (bar 1)");
        assert_eq!(
            onset_of(62),
            Some(bar),
            "D4 (bar 2) — bar 1 was padded to 4/4"
        );
        assert_eq!(
            onset_of(64),
            Some(2 * bar),
            "E4 (bar 3) — drift accumulated"
        );
    }

    #[test]
    fn test_convert_returns_unsupported() {
        let score = Score::new();
        let adapter = IrToMidiAdapter::new();
        assert!(adapter.convert(&score).is_err());
    }

    /// Two voices in the same measure must not panic (regression for
    /// subtract-with-overflow when voice_tick < last_emit_tick).
    #[test]
    fn test_multi_voice_no_overflow() {
        let mut voice1 = Voice::new(1);
        voice1.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::quarter(),
        ))));
        voice1.elements.push(VoiceElement::Rest(Rest::new(Duration {
            base: Frac::new(3, 4),
            dots: 0,
            tuplet_normal: 1,
            tuplet_actual: 1,
        })));

        let mut voice2 = Voice::new(2);
        voice2.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::E, 3),
            Duration::half(),
        ))));
        voice2
            .elements
            .push(VoiceElement::Rest(Rest::new(Duration::half())));

        let mut measure = Measure::new(1);
        measure.attributes = Some(MeasureAttributes {
            divisions: 480,
            time: Some(TimeSignature::default()),
            key: Some(KeySignature::default()),
            ..MeasureAttributes::default()
        });
        measure.voices.push(voice1);
        measure.voices.push(voice2);

        let mut part = Part::new("P1");
        part.name = "Multi".to_string();
        part.measures.push(measure);
        let score = Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        };

        let adapter = IrToMidiAdapter::new();
        let bytes = adapter.convert_bytes(&score).unwrap();

        // Must be valid MIDI.
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.tracks.len(), 2);

        // Verify both pitches round-trip.
        let reader = crate::adapters::midi_to_ir::MidiToIrAdapter::new();
        let score2 = reader.convert_bytes(&bytes).unwrap();
        let midi_nums: Vec<u8> = score2.parts()[0].measures[0]
            .voices
            .iter()
            .flat_map(|v| v.elements.iter())
            .filter_map(|e| {
                if let VoiceElement::Note(n) = e {
                    Some(n.pitch.midi_number() as u8)
                } else {
                    None
                }
            })
            .collect();
        assert!(midi_nums.contains(&60), "Expected C4 (60)");
        assert!(midi_nums.contains(&52), "Expected E3 (52)");
    }

    /// Chords interleaved with a second voice must produce valid MIDI.
    #[test]
    fn test_multi_voice_with_chords() {
        use crate::ir::note::Chord;

        let mut voice1 = Voice::new(1);
        let chord = Chord::new(
            Duration::half(),
            vec![
                Note::new(Pitch::new(PitchStep::C, 4), Duration::half()),
                Note::new(Pitch::new(PitchStep::E, 4), Duration::half()),
            ],
        );
        voice1.elements.push(VoiceElement::Chord(chord));
        voice1
            .elements
            .push(VoiceElement::Rest(Rest::new(Duration::half())));

        let mut voice2 = Voice::new(2);
        voice2.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::G, 3),
            Duration::whole(),
        ))));

        let mut measure = Measure::new(1);
        measure.attributes = Some(MeasureAttributes {
            divisions: 480,
            time: Some(TimeSignature::default()),
            key: Some(KeySignature::default()),
            ..MeasureAttributes::default()
        });
        measure.voices.push(voice1);
        measure.voices.push(voice2);

        let mut part = Part::new("P1");
        part.measures.push(measure);
        let score = Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        };

        let adapter = IrToMidiAdapter::new();
        let bytes = adapter.convert_bytes(&score).unwrap();

        // Parse back — valid MIDI with notes from both voices + chord.
        let reader = crate::adapters::midi_to_ir::MidiToIrAdapter::new();
        let score2 = reader.convert_bytes(&bytes).unwrap();
        let midi_nums: Vec<u8> = score2.parts()[0].measures[0]
            .voices
            .iter()
            .flat_map(|v| v.elements.iter())
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.midi_number() as u8),
                VoiceElement::Chord(c) => Some(c.notes[0].pitch.midi_number() as u8),
                _ => None,
            })
            .collect();
        // C4=60, E4=64, G3=55
        assert!(
            midi_nums.contains(&60) || midi_nums.contains(&64),
            "Expected chord notes"
        );
        assert!(midi_nums.contains(&55), "Expected G3 (55)");
    }

    // ---- Phase B: playing as LilyPond does ----

    use crate::adapters::ly_to_ir::LyToIrAdapter;
    use crate::adapters::ToIrAdapter;

    /// Every note as (on, off, key, velocity), and the conductor's metas.
    type Played = (Vec<(u32, u32, u8, u8)>, Vec<String>);

    fn play_ly(src: &str) -> Played {
        play(
            &LyToIrAdapter::new().convert_str(src).unwrap(),
            IrToMidiAdapter::new(),
        )
    }

    fn play(score: &Score, adapter: IrToMidiAdapter) -> Played {
        let bytes = adapter.convert_bytes(score).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let mut notes = Vec::new();
        let mut metas = Vec::new();
        for (ti, track) in smf.tracks.iter().enumerate() {
            let mut open: HashMap<u8, Vec<(u32, u8)>> = HashMap::new();
            let mut t = 0u32;
            for ev in track {
                t += ev.delta.as_int();
                match ev.kind {
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOn { key, vel },
                        ..
                    } if vel.as_int() > 0 => {
                        open.entry(key.as_int())
                            .or_default()
                            .push((t, vel.as_int()));
                    }
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. },
                        ..
                    } => {
                        if let Some(q) = open.get_mut(&key.as_int()).filter(|q| !q.is_empty()) {
                            let (on, vel) = q.remove(0);
                            notes.push((on, t, key.as_int(), vel));
                        }
                    }
                    TrackEventKind::Midi {
                        message: MidiMessage::Controller { controller, value },
                        ..
                    } => {
                        metas.push(format!("{t} cc{} {}", controller.as_int(), value.as_int()));
                    }
                    TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, ..)) if ti == 0 => {
                        metas.push(format!("{t} time {n}/{}", 1u32 << d));
                    }
                    TrackEventKind::Meta(MetaMessage::Lyric(l)) => {
                        metas.push(format!("{t} lyric {}", String::from_utf8_lossy(l)));
                    }
                    _ => {}
                }
            }
        }
        notes.sort();
        (notes, metas)
    }

    #[test]
    fn pickup_bar_is_short_and_says_so() {
        // After `\partial 4` the next bar starts one beat in, not a bar in.
        let (notes, metas) = play_ly(r"{ \time 4/4 \partial 4 g4 | c'1 }");
        assert_eq!(notes[0].0, 0);
        assert_eq!(notes[1].0, 384, "{notes:?}");
        assert!(metas.contains(&"0 time 1/4".to_string()), "{metas:?}");
        assert!(metas.contains(&"384 time 4/4".to_string()), "{metas:?}");
    }

    #[test]
    fn repeats_are_played_out_unless_asked_not_to() {
        let src = r"{ \repeat volta 2 { c'1 } \alternative { { d'1 } { e'1 } } f'1 }";
        let score = LyToIrAdapter::new().convert_str(src).unwrap();
        let keys = |n: &[(u32, u32, u8, u8)]| n.iter().map(|x| x.2).collect::<Vec<_>>();
        let (unfolded, _) = play(&score, IrToMidiAdapter::new());
        assert_eq!(keys(&unfolded), vec![60, 62, 60, 64, 65]);
        let (written, _) = play(&score, IrToMidiAdapter::new().with_unfold_repeats(false));
        assert_eq!(keys(&written), vec![60, 62, 64, 65]);
        // A first ending alone, closed by the repeat sign.
        let src = r"{ \repeat volta 2 { c'1 } \alternative { { d'1 } { } } f'1 }";
        let score = LyToIrAdapter::new().convert_str(src).unwrap();
        let (unfolded, _) = play(&score, IrToMidiAdapter::new());
        assert_eq!(keys(&unfolded), vec![60, 62, 60, 65]);
    }

    #[test]
    fn dynamics_follow_lilypond() {
        // Velocities LilyPond 2.22 writes for these lines (volume × 127,
        // equalizer, hairpins interpolated or departing 25% of the range).
        let vel = |src: &str| play_ly(src).0.iter().map(|n| n.3).collect::<Vec<_>>();
        assert_eq!(vel(r"{ c'4 d' }"), vec![90, 90]);
        assert_eq!(
            vel(r"{ c'4\p d' e'\< f' g' a' b'\f c'' }"),
            vec![69, 69, 69, 76, 82, 88, 95, 95]
        );
        assert_eq!(
            vel(r"{ c'4\mf d'\< e' f'\! g' }"),
            vec![86, 86, 100, 114, 114]
        );
        assert_eq!(
            vel(r#"{ \set Staff.midiInstrument = "violin" c'4 d'\p }"#),
            vec![97, 81]
        );
    }

    #[test]
    fn articulations_follow_lilypond() {
        // At 60 BPM: staccato halves (+4), staccatissimo lasts 1/8 s (+6),
        // accent +20, marcato +40 (clamped), tenuto as written.
        let (notes, _) = play_ly(r"{ \tempo 4 = 60 g'4-. a'-> b'-^ c''-- d''-! }");
        let got: Vec<(u32, u8)> = notes.iter().map(|n| (n.1 - n.0, n.3)).collect();
        assert_eq!(
            got,
            vec![(192, 94), (384, 110), (384, 127), (384, 90), (48, 96)]
        );
    }

    #[test]
    fn grace_notes_sound_before_the_beat() {
        // LilyPond: c' is cut where the graces start; each grace lasts 9/40 of
        // its written length; the main note is on the beat.
        let (notes, _) = play_ly(r"{ c'4 \grace { d'16 e' } f'4 }");
        assert_eq!(
            notes.iter().map(|n| (n.0, n.1, n.2)).collect::<Vec<_>>(),
            vec![(0, 340, 60), (340, 361, 62), (362, 383, 64), (384, 768, 65)]
        );
    }

    #[test]
    fn tied_chord_sounds_once() {
        let (notes, _) = play_ly(r"{ <c' e'>2~ <c' e'>2 }");
        assert_eq!(notes, vec![(0, 1536, 60, 90), (0, 1536, 64, 90)]);
    }

    #[test]
    fn septuplets_do_not_drift() {
        let (notes, _) = play_ly(r"{ \tuplet 7/4 { c'16 d' e' f' g' a' b' } c''4 }");
        assert_eq!(notes.last().unwrap().0, 384, "{notes:?}");
    }

    #[test]
    fn pedal_and_lyrics_are_events() {
        let (_, metas) = play_ly(r"{ c'4\sustainOn d'4\sustainOff } \addlyrics { la -- la }");
        assert!(metas.iter().any(|m| m == "0 cc64 127"), "{metas:?}");
        assert!(metas.iter().any(|m| m == "384 cc64 0"), "{metas:?}");
        assert!(metas.iter().any(|m| m == "0 lyric la-"), "{metas:?}");
    }

    #[test]
    fn channels_follow_musicxml_and_skip_drums() {
        // `Part.midi_channel` is 1–16 (0 = not set): channel 10 is drums.
        let part = |id: &str, ch: u8, program: u8| {
            let mut p = Part::new(id);
            p.midi_channel = ch;
            p.midi_program = program;
            let mut m = Measure::new(1);
            let mut v = Voice::new(1);
            v.elements.push(VoiceElement::Note(Box::new(Note::new(
                Pitch::new(PitchStep::C, 4),
                Duration::quarter(),
            ))));
            m.voices.push(v);
            p.measures.push(m);
            ScoreChild::Part(p)
        };
        let mut children = vec![part("D", 10, 0)];
        children.extend((0..17).map(|i| part(&format!("P{i}"), 0, i as u8 % 3)));
        let score = Score {
            metadata: ScoreMetadata::default(),
            page_layout: None,
            children,
        };
        let bytes = IrToMidiAdapter::new().convert_bytes(&score).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let channel_of = |track: usize| {
            smf.tracks[track].iter().find_map(|e| match e.kind {
                TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOn { .. },
                } => Some(channel.as_int()),
                _ => None,
            })
        };
        assert_eq!(channel_of(1), Some(9), "drums on channel 10");
        let melodic: Vec<u8> = (2..smf.tracks.len()).filter_map(channel_of).collect();
        assert!(melodic.iter().all(|&c| c != 9), "{melodic:?}");
        // Past 15 melodic parts, a part shares a channel with its program.
        assert_eq!(melodic[15], melodic[0]);
    }

    #[test]
    fn transposing_instrument_sounds_at_concert_pitch() {
        use crate::ir::measure::Transpose;
        let mut score = one_note_score(Pitch::new(PitchStep::D, 4), Duration::quarter());
        if let ScoreChild::Part(p) = &mut score.children[0] {
            p.measures[0].attributes.as_mut().unwrap().transpose = Some(Transpose {
                diatonic: -1,
                chromatic: -2,
                octave_change: 0,
            });
        }
        let (notes, _) = play(&score, IrToMidiAdapter::new());
        assert_eq!(notes[0].2, 60, "a B♭ clarinet's written D sounds C");
    }
}
