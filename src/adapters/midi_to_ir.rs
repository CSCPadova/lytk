//! MIDI → IR adapter.
//!
//! Parses a Standard MIDI File (SMF) using the `midly` crate and produces
//! an IR [`Score`]. MIDI is lossy — articulations, lyrics, and detailed
//! notation are not preserved. The adapter quantises tick-based durations to
//! the nearest standard musical duration and assigns tracks/channels to parts.

use std::collections::HashMap;
use std::path::Path;

use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use super::dynamics_velocity::velocity_to_dynamic;
use super::{AdapterError, Result, ToIrAdapter};
use crate::ir::articulation::{DynamicMark, Placement, StartStop, TieEvent};
use crate::ir::direction::{Direction, TempoDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::{Clef, KeyMode, KeySignature, Measure, MeasureAttributes, TimeSignature};
use crate::ir::music::MusicDocument;
use crate::ir::note::{Chord, Note, Rest, VoiceElement};
use crate::ir::part::Part;
use crate::ir::pitch::{Alter, Pitch, PitchStep};
use crate::ir::score::{Score, ScoreChild, ScoreMetadata};
use crate::ir::voice::Voice;

// ---------------------------------------------------------------------------
// Public adapter
// ---------------------------------------------------------------------------

/// Adapter that reads MIDI files and produces an IR [`Score`].
pub struct MidiToIrAdapter;

impl MidiToIrAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Parse raw MIDI bytes into a [`Score`].
    pub fn convert_bytes(&self, bytes: &[u8]) -> Result<Score> {
        let smf = Smf::parse(bytes).map_err(|e| AdapterError::Parse(e.to_string()))?;

        let divisions = match smf.header.timing {
            Timing::Metrical(tpb) => u32::from(tpb.as_int()),
            Timing::Timecode(fps, sub) => {
                // SMPTE: use fps×sub as approximate ticks-per-quarter
                (fps.as_int() as u32) * (sub as u32)
            }
        }
        // A crafted/corrupt SMF header can declare 0 ticks-per-quarter (or a
        // 0-subframe SMPTE timing). Clamp to 1 so the measure-boundary math
        // below can't divide-by-zero / loop forever.
        .max(1);

        match smf.header.format {
            Format::SingleTrack => self.parse_format0(&smf.tracks, divisions),
            Format::Parallel => self.parse_format1(&smf.tracks, divisions),
            Format::Sequential => self.parse_format1(&smf.tracks, divisions),
        }
    }
}

impl Default for MidiToIrAdapter {
    fn default() -> Self {
        Self::new()
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
// Internal: raw event collection
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

/// Working copy of a note during measure assembly. Notes that cross a
/// barline are split here: the in-measure part is emitted with a tie start
/// and the remainder is re-queued with `tied_from_prev` set.
#[derive(Clone, Copy)]
struct WorkNote {
    start_tick: u64,
    end_tick: u64,
    midi_key: u8,
    velocity: u8,
    tied_from_prev: bool,
}

/// Meta events extracted from a track.
#[derive(Debug, Default)]
struct TrackMeta {
    name: String,
    /// (abs_tick, microseconds_per_quarter)
    tempo_changes: Vec<(u64, u32)>,
    /// (abs_tick, numerator, denominator_power)
    time_sig_changes: Vec<(u64, u8, u8)>,
    /// (abs_tick, fifths, is_minor)
    key_sig_changes: Vec<(u64, i8, bool)>,
    /// (abs_tick, channel, program)
    program_changes: Vec<(u64, u8, u8)>,
}

/// Collect raw note pairs and meta events from a single MIDI track.
fn collect_track_events(events: &[midly::TrackEvent<'_>]) -> (Vec<RawNote>, TrackMeta) {
    let mut notes: Vec<RawNote> = Vec::new();
    let mut meta = TrackMeta::default();
    // Pending note-ons: (key, channel) → (start_tick, velocity)
    let mut pending: HashMap<(u8, u8), (u64, u8)> = HashMap::new();
    let mut abs_tick: u64 = 0;

    for event in events {
        abs_tick += u64::from(event.delta.as_int());

        match event.kind {
            TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();
                match message {
                    MidiMessage::NoteOn { key, vel } => {
                        let k = key.as_int();
                        let v = vel.as_int();
                        if v == 0 {
                            // NoteOn with vel 0 = NoteOff
                            if let Some((start, velocity)) = pending.remove(&(k, ch)) {
                                notes.push(RawNote {
                                    start_tick: start,
                                    end_tick: abs_tick,
                                    midi_key: k,
                                    velocity,
                                    channel: ch,
                                });
                            }
                        } else {
                            pending.insert((k, ch), (abs_tick, v));
                        }
                    }
                    MidiMessage::NoteOff { key, .. } => {
                        let k = key.as_int();
                        if let Some((start, velocity)) = pending.remove(&(k, ch)) {
                            notes.push(RawNote {
                                start_tick: start,
                                end_tick: abs_tick,
                                midi_key: k,
                                velocity,
                                channel: ch,
                            });
                        }
                    }
                    MidiMessage::ProgramChange { program } => {
                        meta.program_changes.push((abs_tick, ch, program.as_int()));
                    }
                    _ => {}
                }
            }
            TrackEventKind::Meta(msg) => match msg {
                MetaMessage::TrackName(name_bytes) => {
                    if let Ok(s) = std::str::from_utf8(name_bytes) {
                        meta.name = s.to_string();
                    }
                }
                MetaMessage::Tempo(uspq) => {
                    meta.tempo_changes.push((abs_tick, uspq.as_int()));
                }
                MetaMessage::TimeSignature(num, den_pow, _clocks, _tpq) => {
                    meta.time_sig_changes.push((abs_tick, num, den_pow));
                }
                MetaMessage::KeySignature(sf, minor) => {
                    // sf is i8 (sharps/flats), minor is bool
                    meta.key_sig_changes.push((abs_tick, sf, minor));
                }
                _ => {}
            },
            _ => {}
        }
    }

    (notes, meta)
}

// ---------------------------------------------------------------------------
// Internal: Format 0 (single track → split by channel)
// ---------------------------------------------------------------------------

impl MidiToIrAdapter {
    fn parse_format0(
        &self,
        tracks: &[Vec<midly::TrackEvent<'_>>],
        divisions: u32,
    ) -> Result<Score> {
        let (notes, meta) = if let Some(t) = tracks.first() {
            collect_track_events(t)
        } else {
            return Ok(Score::new());
        };

        // Group notes by channel
        let mut by_channel: HashMap<u8, Vec<&RawNote>> = HashMap::new();
        for n in &notes {
            by_channel.entry(n.channel).or_default().push(n);
        }

        let mut score = Score::new();
        score.metadata = build_metadata(&meta);

        let mut channels: Vec<u8> = by_channel.keys().copied().collect();
        channels.sort();

        for ch in channels {
            let ch_notes = by_channel.get(&ch).unwrap();
            // Most recent program change for this channel (scan from the end).
            let program = meta
                .program_changes
                .iter()
                .rev()
                .find_map(|(_, c, p)| (*c == ch).then_some(*p))
                .unwrap_or(0);

            let part = build_part(
                ch_notes,
                &meta,
                divisions,
                &format!("P{}", ch + 1),
                &meta.name,
                ch,
                program,
            );
            score.children.push(ScoreChild::Part(part));
        }

        Ok(score)
    }

    // ---------------------------------------------------------------------------
    // Internal: Format 1/2 (multi-track → one part per track)
    // ---------------------------------------------------------------------------

    fn parse_format1(
        &self,
        tracks: &[Vec<midly::TrackEvent<'_>>],
        divisions: u32,
    ) -> Result<Score> {
        let mut score = Score::new();

        // Track 0 is often just the conductor track (tempo/time sig only).
        // Collect its meta events for the score metadata.
        let mut conductor_meta = TrackMeta::default();
        if let Some(t0) = tracks.first() {
            let (_, m) = collect_track_events(t0);
            conductor_meta = m;
        }
        score.metadata = build_metadata(&conductor_meta);

        for (i, track) in tracks.iter().enumerate() {
            let (notes, track_meta) = collect_track_events(track);
            if notes.is_empty() {
                continue; // skip conductor-only tracks
            }

            let all_refs: Vec<&RawNote> = notes.iter().collect();
            let ch = notes.first().map(|n| n.channel).unwrap_or(0);
            let program = track_meta
                .program_changes
                .iter()
                .map(|(_, _, p)| *p)
                .next_back()
                .unwrap_or(0);

            // Merge conductor meta with track-local meta
            let merged = merge_meta(&conductor_meta, &track_meta);

            let name = if track_meta.name.is_empty() {
                format!("Track {}", i + 1)
            } else {
                track_meta.name.clone()
            };

            let part = build_part(
                &all_refs,
                &merged,
                divisions,
                &format!("P{}", i + 1),
                &name,
                ch,
                program,
            );
            score.children.push(ScoreChild::Part(part));
        }

        Ok(score)
    }
}

// ---------------------------------------------------------------------------
// Part construction
// ---------------------------------------------------------------------------

fn build_metadata(meta: &TrackMeta) -> ScoreMetadata {
    let mut sm = ScoreMetadata::default();
    if !meta.name.is_empty() {
        sm.title = Some(meta.name.clone());
    }
    sm
}

fn merge_meta(conductor: &TrackMeta, track: &TrackMeta) -> TrackMeta {
    TrackMeta {
        name: if track.name.is_empty() {
            conductor.name.clone()
        } else {
            track.name.clone()
        },
        tempo_changes: if track.tempo_changes.is_empty() {
            conductor.tempo_changes.clone()
        } else {
            track.tempo_changes.clone()
        },
        time_sig_changes: if track.time_sig_changes.is_empty() {
            conductor.time_sig_changes.clone()
        } else {
            track.time_sig_changes.clone()
        },
        key_sig_changes: if track.key_sig_changes.is_empty() {
            conductor.key_sig_changes.clone()
        } else {
            track.key_sig_changes.clone()
        },
        program_changes: track.program_changes.clone(),
    }
}

/// Build a single [`Part`] from a set of raw MIDI notes.
fn make_time_sig(num: u8, den_pow: u8) -> TimeSignature {
    TimeSignature {
        beats: num.to_string(),
        // `den_pow` is the raw denominator-power byte from the MIDI file; a real
        // one is ≤ 7 (128th-note beat). Clamp so the u8 shift can't overflow on
        // a crafted value (den_pow ≥ 8).
        beat_type: 1u8 << den_pow.min(7),
        symbol: None,
    }
}

fn make_key_sig(fifths: i8, minor: bool) -> KeySignature {
    KeySignature {
        fifths,
        mode: if minor {
            KeyMode::Minor
        } else {
            KeyMode::Major
        },
    }
}

fn build_part(
    notes: &[&RawNote],
    meta: &TrackMeta,
    divisions: u32,
    part_id: &str,
    name: &str,
    channel: u8,
    program: u8,
) -> Part {
    let mut part = Part::new(part_id);
    part.name = name.to_string();
    part.midi_channel = channel;
    part.midi_program = program;

    // Determine measure boundaries from time signature events.
    let time_sigs = resolve_time_signatures(meta, divisions);
    let key_sigs = &meta.key_sig_changes;

    // Find the last tick among all notes.
    let last_tick = notes.iter().map(|n| n.end_tick).max().unwrap_or(0);

    // Build measure boundaries.
    let measure_boundaries = compute_measure_boundaries(&time_sigs, divisions, last_tick);

    // Build a sorted list of notes by start tick.
    let mut sorted_notes: Vec<WorkNote> = notes
        .iter()
        .map(|n| WorkNote {
            start_tick: n.start_tick,
            end_tick: n.end_tick,
            midi_key: n.midi_key,
            velocity: n.velocity,
            tied_from_prev: false,
        })
        .collect();
    sorted_notes.sort_by_key(|n| (n.start_tick, n.midi_key));

    // Determine key context (for sharp/flat note naming)
    let initial_key_fifths = key_sigs.first().map(|(_, f, _)| *f).unwrap_or(0);
    let use_sharps = initial_key_fifths >= 0;

    // Assign notes to measures and build IR.
    // Running dynamic: attach a mark only when the velocity band changes, so a
    // crescendo of equal-velocity notes does not spam a dynamic on every note.
    let mut prev_dyn: Option<&'static str> = None;
    let mut note_idx = 0;
    for (m_idx, (m_start, m_end)) in measure_boundaries.iter().enumerate() {
        let mut measure = Measure::new((m_idx + 1) as u32);

        // Set attributes on first measure or when time/key changes.
        if m_idx == 0 {
            let mut attrs = MeasureAttributes {
                divisions: divisions as u16,
                ..Default::default()
            };
            if let Some(&(_, num, den_pow)) = time_sigs.first() {
                attrs.time = Some(make_time_sig(num, den_pow));
            } else {
                attrs.time = Some(TimeSignature::default());
            }
            if let Some(&(_, fifths, minor)) = key_sigs.first() {
                attrs.key = Some(make_key_sig(fifths, minor));
            }
            attrs.clefs.insert(1, Clef::default());
            measure.attributes = Some(attrs);
        } else {
            // Check for time/key changes at this measure boundary.
            let mut attrs: Option<MeasureAttributes> = None;
            for &(tick, num, den_pow) in &time_sigs {
                if tick == *m_start {
                    let a = attrs.get_or_insert_with(|| MeasureAttributes {
                        divisions: divisions as u16,
                        ..Default::default()
                    });
                    a.time = Some(make_time_sig(num, den_pow));
                }
            }
            for &(tick, fifths, minor) in key_sigs {
                if tick == *m_start {
                    let a = attrs.get_or_insert_with(|| MeasureAttributes {
                        divisions: divisions as u16,
                        ..Default::default()
                    });
                    a.key = Some(make_key_sig(fifths, minor));
                }
            }
            measure.attributes = attrs;
        }

        // Add tempo directions.
        for &(tick, uspq) in &meta.tempo_changes {
            if tick >= *m_start && tick < *m_end {
                let bpm = 60_000_000.0 / uspq as f64;
                measure.directions.push(Direction {
                    tempo: Some(TempoDirection {
                        text: None,
                        beat_unit: Some("quarter".to_string()),
                        per_minute: Some(bpm),
                        dots: 0,
                        placement: Placement::Above,
                    }),
                    ..Direction::default()
                });
            }
        }

        // Collect notes in this measure.
        let mut voice_elements: Vec<VoiceElement> = Vec::new();
        let mut cursor = *m_start; // current time position in ticks

        while note_idx < sorted_notes.len() && sorted_notes[note_idx].start_tick < *m_end {
            let n = sorted_notes[note_idx];

            // Insert rest for any gap before this note.
            if n.start_tick > cursor {
                let gap_ticks = n.start_tick - cursor;
                if let Some(rest_dur) = quantize_ticks(gap_ticks, divisions) {
                    voice_elements.push(VoiceElement::Rest(Rest::new(rest_dur)));
                }
            }

            // Group simultaneous note-ons with the same duration into a chord.
            let mut group_end = note_idx + 1;
            while group_end < sorted_notes.len()
                && sorted_notes[group_end].start_tick == n.start_tick
                && sorted_notes[group_end].end_tick == n.end_tick
            {
                group_end += 1;
            }

            // Note duration: clamp to the measure boundary; a crossing note's
            // remainder continues into the next measure as a tied note.
            let crosses = n.end_tick > *m_end;
            let note_end = n.end_tick.min(*m_end);
            let note_ticks = note_end.saturating_sub(n.start_tick);

            if let Some(dur) = quantize_ticks(note_ticks, divisions) {
                let band = velocity_to_dynamic(n.velocity);
                let mark = if prev_dyn != Some(band) {
                    prev_dyn = Some(band);
                    Some(DynamicMark {
                        sign: band.to_string(),
                        placement: Placement::default(),
                    })
                } else {
                    None
                };
                let mut group_notes: Vec<Note> = sorted_notes[note_idx..group_end]
                    .iter()
                    .map(|wn| {
                        let pitch = midi_key_to_pitch(wn.midi_key, use_sharps);
                        let mut note = Note::new(pitch, dur.clone());
                        if wn.tied_from_prev {
                            note.ties.push(TieEvent {
                                tie_type: StartStop::Stop,
                            });
                        }
                        if crosses {
                            note.ties.push(TieEvent {
                                tie_type: StartStop::Start,
                            });
                        }
                        note
                    })
                    .collect();
                if let Some(mark) = mark {
                    if let Some(first) = group_notes.first_mut() {
                        first.dynamics.push(mark);
                    }
                }
                if group_notes.len() == 1 {
                    voice_elements.push(VoiceElement::Note(Box::new(
                        group_notes.pop().expect("one note"),
                    )));
                } else {
                    voice_elements.push(VoiceElement::Chord(Chord::new(dur, group_notes)));
                }
            }

            cursor = note_end;
            if crosses {
                // Re-queue the remainder at its sorted position so the next
                // measure emits it with the closing tie.
                let moved: Vec<WorkNote> = sorted_notes.drain(note_idx..group_end).collect();
                for mut wn in moved {
                    wn.start_tick = *m_end;
                    wn.tied_from_prev = true;
                    let pos = sorted_notes.partition_point(|x| {
                        (x.start_tick, x.midi_key) <= (wn.start_tick, wn.midi_key)
                    });
                    sorted_notes.insert(pos, wn);
                }
                // note_idx unchanged: draining shifted the remaining notes left.
            } else {
                note_idx = group_end;
            }
        }

        // Fill remaining measure time with rest.
        if cursor < *m_end {
            let gap = *m_end - cursor;
            if let Some(rest_dur) = quantize_ticks(gap, divisions) {
                voice_elements.push(VoiceElement::Rest(Rest::new(rest_dur)));
            }
        }

        if !voice_elements.is_empty() {
            let mut voice = Voice::new(1);
            voice.elements = voice_elements;
            measure.voices.push(voice);
        }

        part.measures.push(measure);
    }

    part
}

// ---------------------------------------------------------------------------
// Time signature / measure boundary computation
// ---------------------------------------------------------------------------

/// Resolve time signature events into (abs_tick, numerator, denominator_power).
fn resolve_time_signatures(meta: &TrackMeta, _divisions: u32) -> Vec<(u64, u8, u8)> {
    if meta.time_sig_changes.is_empty() {
        vec![(0, 4, 2)] // default 4/4
    } else {
        meta.time_sig_changes.clone()
    }
}

/// Compute measure boundary tick pairs: `[(start, end), ...]`.
fn compute_measure_boundaries(
    time_sigs: &[(u64, u8, u8)],
    divisions: u32,
    last_tick: u64,
) -> Vec<(u64, u64)> {
    let mut boundaries = Vec::new();
    let mut cursor: u64 = 0;
    let mut ts_idx = 0;

    while cursor < last_tick {
        // Advance time signature index if the next change is at or before cursor.
        while ts_idx + 1 < time_sigs.len() && time_sigs[ts_idx + 1].0 <= cursor {
            ts_idx += 1;
        }
        let (_, num, den_pow) = time_sigs[ts_idx];
        // Clamp the denominator power so the shift can't overflow on a crafted
        // time signature (den_pow ≥ 32).
        let den = 1u64 << (den_pow.min(31));
        // Measure length in ticks = (num / den) * 4 * divisions
        //   = num * 4 * divisions / den
        let measure_ticks = (num as u64) * 4 * (divisions as u64) / den;
        // A 0/N time signature (num == 0) — legal to encode, invalid musically —
        // yields a zero-length measure; without this guard `cursor` never
        // advances and the loop runs forever, growing `boundaries` until OOM.
        if measure_ticks == 0 {
            break;
        }
        let end = (cursor + measure_ticks).min(last_tick);
        boundaries.push((cursor, end));
        cursor += measure_ticks;
    }

    // Ensure at least one measure.
    if boundaries.is_empty() {
        boundaries.push((0, 0));
    }

    boundaries
}

// ---------------------------------------------------------------------------
// Duration quantisation
// ---------------------------------------------------------------------------

/// Candidate (base_frac_num, base_frac_den, dots, tuplet_actual, tuplet_normal).
/// All are fractions of a whole note.
const CANDIDATES: &[(i64, i64, u8, u8, u8)] = &[
    // Standard durations
    (1, 1, 0, 1, 1),  // whole
    (1, 1, 1, 1, 1),  // dotted whole
    (1, 2, 0, 1, 1),  // half
    (1, 2, 1, 1, 1),  // dotted half
    (1, 4, 0, 1, 1),  // quarter
    (1, 4, 1, 1, 1),  // dotted quarter
    (1, 8, 0, 1, 1),  // eighth
    (1, 8, 1, 1, 1),  // dotted eighth
    (1, 16, 0, 1, 1), // 16th
    (1, 16, 1, 1, 1), // dotted 16th
    (1, 32, 0, 1, 1), // 32nd
    (1, 32, 1, 1, 1), // dotted 32nd
    (1, 64, 0, 1, 1), // 64th
    // Triplets
    (1, 4, 0, 3, 2),  // triplet quarter
    (1, 8, 0, 3, 2),  // triplet eighth
    (1, 16, 0, 3, 2), // triplet 16th
    (1, 2, 0, 3, 2),  // triplet half
];

/// Compute the tick count for a candidate at the given divisions.
fn candidate_ticks(
    base_n: i64,
    base_d: i64,
    dots: u8,
    tuplet_actual: u8,
    tuplet_normal: u8,
    divisions: u32,
) -> i64 {
    let base = Frac::new(base_n, base_d);
    let dot_mult = crate::ir::duration::dot_multiplier(dots);
    let tuplet_mult = Frac::new(tuplet_normal as i64, tuplet_actual as i64);
    let actual = base * dot_mult * tuplet_mult;
    // ticks = actual_duration_whole_notes * 4 * divisions
    let ticks_frac = actual * Frac::from_integer(4 * divisions as i64);
    // Round to nearest integer
    *ticks_frac.numer() / *ticks_frac.denom()
}

/// Quantise a tick duration to the nearest standard musical duration.
/// Returns `None` for zero-length durations.
fn quantize_ticks(ticks: u64, divisions: u32) -> Option<Duration> {
    if ticks == 0 {
        return None;
    }

    let ticks_i64 = ticks as i64;
    let mut best_diff = i64::MAX;
    let mut best_idx = 0;

    for (i, &(bn, bd, dots, ta, tn)) in CANDIDATES.iter().enumerate() {
        let ct = candidate_ticks(bn, bd, dots, ta, tn, divisions);
        let diff = (ct - ticks_i64).abs();
        if diff < best_diff {
            best_diff = diff;
            best_idx = i;
        }
    }

    let &(bn, bd, dots, ta, tn) = &CANDIDATES[best_idx];
    Some(Duration {
        base: Frac::new(bn, bd),
        dots,
        tuplet_actual: ta,
        tuplet_normal: tn,
    })
}

// ---------------------------------------------------------------------------
// MIDI pitch conversion
// ---------------------------------------------------------------------------

/// Convert a MIDI key number (0–127) to an IR [`Pitch`].
///
/// Uses sharps or flats based on the `use_sharps` parameter.
fn midi_key_to_pitch(midi_key: u8, use_sharps: bool) -> Pitch {
    let octave = (midi_key as i32 / 12) - 1;
    let semitone = (midi_key % 12) as i32;

    // (PitchStep, alter_semitones)
    let (step, alter) = if use_sharps {
        match semitone {
            0 => (PitchStep::C, 0),
            1 => (PitchStep::C, 1),
            2 => (PitchStep::D, 0),
            3 => (PitchStep::D, 1),
            4 => (PitchStep::E, 0),
            5 => (PitchStep::F, 0),
            6 => (PitchStep::F, 1),
            7 => (PitchStep::G, 0),
            8 => (PitchStep::G, 1),
            9 => (PitchStep::A, 0),
            10 => (PitchStep::A, 1),
            11 => (PitchStep::B, 0),
            _ => unreachable!(),
        }
    } else {
        match semitone {
            0 => (PitchStep::C, 0),
            1 => (PitchStep::D, -1),
            2 => (PitchStep::D, 0),
            3 => (PitchStep::E, -1),
            4 => (PitchStep::E, 0),
            5 => (PitchStep::F, 0),
            6 => (PitchStep::G, -1),
            7 => (PitchStep::G, 0),
            8 => (PitchStep::A, -1),
            9 => (PitchStep::A, 0),
            10 => (PitchStep::B, -1),
            11 => (PitchStep::B, 0),
            _ => unreachable!(),
        }
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

    #[test]
    fn test_compute_measure_boundaries_zero_numerator_terminates() {
        // A 0/4 time signature (num == 0) yields zero-length measures; the loop
        // must break instead of spinning forever / OOMing.
        let bounds = compute_measure_boundaries(&[(0, 0, 2)], 480, 1920);
        assert!(!bounds.is_empty());
    }

    #[test]
    fn test_compute_measure_boundaries_huge_den_pow_no_overflow() {
        // A crafted denominator power (≥ 32) must not overflow the shift.
        let bounds = compute_measure_boundaries(&[(0, 4, 200)], 480, 1920);
        assert!(!bounds.is_empty());
    }

    #[test]
    fn test_zero_tpb_header_is_clamped() {
        // A MIDI header declaring 0 ticks-per-quarter must not divide-by-zero or
        // loop forever. Minimal format-0 SMF: header + one (empty) track.
        let bytes: &[u8] = &[
            b'M', b'T', b'h', b'd', 0, 0, 0, 6, // header chunk, len 6
            0, 0, // format 0
            0, 1, // 1 track
            0, 0, // division = 0 ticks-per-quarter (degenerate)
            b'M', b'T', b'r', b'k', 0, 0, 0, 4, // track chunk, len 4
            0, 0xFF, 0x2F, 0x00, // end-of-track
        ];
        // Must return (Ok or Err) without panicking/hanging.
        let _ = MidiToIrAdapter::new().convert_bytes(bytes);
    }

    #[test]
    fn test_midi_key_to_pitch_middle_c() {
        let p = midi_key_to_pitch(60, true);
        assert_eq!(p.step, PitchStep::C);
        assert_eq!(p.octave, 4);
        assert_eq!(p.alter, Alter::from_integer(0));
    }

    #[test]
    fn test_midi_key_to_pitch_sharps() {
        // MIDI 61 = C#4
        let p = midi_key_to_pitch(61, true);
        assert_eq!(p.step, PitchStep::C);
        assert_eq!(p.alter, Alter::from_integer(1));
        assert_eq!(p.octave, 4);
    }

    #[test]
    fn test_midi_key_to_pitch_flats() {
        // MIDI 61 = Db4
        let p = midi_key_to_pitch(61, false);
        assert_eq!(p.step, PitchStep::D);
        assert_eq!(p.alter, Alter::from_integer(-1));
        assert_eq!(p.octave, 4);
    }

    #[test]
    fn test_midi_key_roundtrip() {
        // Check that pitch.midi_number() == original key for all naturals
        for key in [60u8, 62, 64, 65, 67, 69, 71, 72] {
            let p = midi_key_to_pitch(key, true);
            assert_eq!(p.midi_number(), key as i32, "key={key}");
        }
        // Check sharps
        for key in [61u8, 63, 66, 68, 70] {
            let p = midi_key_to_pitch(key, true);
            assert_eq!(p.midi_number(), key as i32, "sharp key={key}");
            let p = midi_key_to_pitch(key, false);
            assert_eq!(p.midi_number(), key as i32, "flat key={key}");
        }
    }

    fn raw(start: u64, end: u64, key: u8) -> RawNote {
        RawNote {
            start_tick: start,
            end_tick: end,
            midi_key: key,
            velocity: 80,
            channel: 0,
        }
    }

    fn build_test_part(notes: &[RawNote], divisions: u32) -> Part {
        let refs: Vec<&RawNote> = notes.iter().collect();
        build_part(&refs, &TrackMeta::default(), divisions, "P1", "test", 0, 0)
    }

    #[test]
    fn test_simultaneous_notes_become_chord() {
        // Three simultaneous note-ons with the same duration are a chord, not
        // three sequential quarter notes (which would inflate the rhythm).
        let notes = [
            raw(0, 480, 60),
            raw(0, 480, 64),
            raw(0, 480, 67),
            raw(480, 960, 62),
        ];
        let part = build_test_part(&notes, 480);
        let elements = &part.measures[0].voices[0].elements;
        match &elements[0] {
            VoiceElement::Chord(c) => {
                let keys: Vec<u8> = c
                    .notes
                    .iter()
                    .map(|n| n.pitch.midi_number() as u8)
                    .collect();
                assert_eq!(keys, vec![60, 64, 67]);
                assert_eq!(c.duration.base, Frac::new(1, 4));
            }
            other => panic!("expected a chord, got {other:?}"),
        }
        match &elements[1] {
            VoiceElement::Note(n) => assert_eq!(n.pitch.midi_number(), 62),
            other => panic!("expected a note after the chord, got {other:?}"),
        }
    }

    #[test]
    fn test_note_crossing_barline_is_tied() {
        // A half note starting on beat 4 of a 4/4 bar (divisions=480) crosses
        // the barline: it must split into two tied quarters, not lose its
        // second half.
        let notes = [raw(0, 1440, 60), raw(1440, 2400, 64)];
        let part = build_test_part(&notes, 480);
        assert!(part.measures.len() >= 2, "need 2 measures");

        let m1 = &part.measures[0].voices[0].elements;
        let last = m1.last().expect("first measure has elements");
        match last {
            VoiceElement::Note(n) => {
                assert_eq!(n.pitch.midi_number(), 64);
                assert_eq!(n.duration.base, Frac::new(1, 4), "clamped to barline");
                assert!(
                    n.ties.iter().any(|t| t.tie_type == StartStop::Start),
                    "crossing note must start a tie"
                );
            }
            other => panic!("expected the crossing note, got {other:?}"),
        }
        let m2 = &part.measures[1].voices[0].elements;
        match &m2[0] {
            VoiceElement::Note(n) => {
                assert_eq!(n.pitch.midi_number(), 64, "remainder must not be dropped");
                assert_eq!(n.duration.base, Frac::new(1, 4));
                assert!(
                    n.ties.iter().any(|t| t.tie_type == StartStop::Stop),
                    "continuation must close the tie"
                );
            }
            other => panic!("expected the tied continuation, got {other:?}"),
        }
    }

    #[test]
    fn test_quantize_quarter() {
        // At divisions=480, quarter = 480 ticks
        let d = quantize_ticks(480, 480).unwrap();
        assert_eq!(d.base, Frac::new(1, 4));
        assert_eq!(d.dots, 0);
    }

    #[test]
    fn test_quantize_dotted_quarter() {
        // At divisions=480, dotted quarter = 720 ticks
        let d = quantize_ticks(720, 480).unwrap();
        assert_eq!(d.base, Frac::new(1, 4));
        assert_eq!(d.dots, 1);
    }

    #[test]
    fn test_quantize_triplet_quarter() {
        // At divisions=480, triplet quarter = 320 ticks
        let d = quantize_ticks(320, 480).unwrap();
        assert_eq!(d.base, Frac::new(1, 4));
        assert_eq!(d.tuplet_actual, 3);
        assert_eq!(d.tuplet_normal, 2);
    }

    #[test]
    fn test_quantize_eighth() {
        let d = quantize_ticks(240, 480).unwrap();
        assert_eq!(d.base, Frac::new(1, 8));
        assert_eq!(d.dots, 0);
    }

    #[test]
    fn test_quantize_whole() {
        let d = quantize_ticks(1920, 480).unwrap();
        assert_eq!(d.base, Frac::new(1, 1));
        assert_eq!(d.dots, 0);
    }

    #[test]
    fn test_measure_boundaries_4_4() {
        let time_sigs = vec![(0u64, 4u8, 2u8)]; // 4/4
        let boundaries = compute_measure_boundaries(&time_sigs, 480, 1920);
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0], (0, 1920));
    }

    #[test]
    fn test_measure_boundaries_two_measures() {
        let time_sigs = vec![(0u64, 4u8, 2u8)]; // 4/4
        let boundaries = compute_measure_boundaries(&time_sigs, 480, 3840);
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0], (0, 1920));
        assert_eq!(boundaries[1], (1920, 3840));
    }

    #[test]
    fn test_measure_boundaries_3_4() {
        let time_sigs = vec![(0u64, 3u8, 2u8)]; // 3/4
        let boundaries = compute_measure_boundaries(&time_sigs, 480, 2880);
        // 3/4 measure = 3 * 480 = 1440 ticks → 2 measures to reach 2880
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0], (0, 1440));
        assert_eq!(boundaries[1], (1440, 2880));
    }
}
