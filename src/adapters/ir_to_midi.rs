//! IR → MIDI adapter.
//!
//! Converts an IR [`Score`] to a Standard MIDI File (Format 1) using the
//! `midly` crate. Track 0 is the conductor (tempo, time sig, key sig),
//! followed by one track per part.
//!
//! MIDI is lossy: articulations, lyrics, ornaments, and notation details
//! are not emitted. Dynamics are approximated as velocity values.

use std::path::Path;

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use num::rational::Ratio;

use super::dynamics_velocity::dynamic_to_velocity;
use super::{AdapterError, FromIrAdapter, Result};
use crate::ir::duration::Frac;
use crate::ir::measure::KeyMode;
use crate::ir::note::VoiceElement;
use crate::ir::Score;

/// `(has_start, has_stop)` for a note's tie events — used to collapse a tie
/// chain into a single MIDI note (one NoteOn at the start, one NoteOff at the end).
fn tie_flags(ties: &[crate::ir::articulation::TieEvent]) -> (bool, bool) {
    use crate::ir::articulation::StartStop;
    (
        ties.iter().any(|t| t.tie_type == StartStop::Start),
        ties.iter().any(|t| t.tie_type == StartStop::Stop),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a beat_unit name + dots to a duration in quarter-note units.
/// E.g. "half" → 2.0, "quarter" → 1.0, "eighth" → 0.5, dotted "quarter" → 1.5.
fn beat_unit_to_quarters(beat_unit: Option<&str>, dots: u8) -> f64 {
    let base = match beat_unit.unwrap_or("quarter") {
        "whole" => 4.0,
        "half" => 2.0,
        "quarter" => 1.0,
        "eighth" => 0.5,
        "16th" => 0.25,
        "32nd" => 0.125,
        "64th" => 0.0625,
        _ => 1.0,
    };
    // Each dot adds half the remaining value: 1 dot → ×1.5, 2 dots → ×1.75
    let mut total = base;
    let mut add = base / 2.0;
    for _ in 0..dots {
        total += add;
        add /= 2.0;
    }
    total
}

/// Map a LilyPond `\set Staff.midiInstrument` name to a General MIDI program
/// number (0–127), falling back to 0 (Acoustic Grand Piano) for unknown names.
/// Delegates to the shared [`gm`](super::gm) table.
fn gm_program_from_name(name: &str) -> u8 {
    super::gm::gm_program_from_name(name).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Public adapter
// ---------------------------------------------------------------------------

/// Adapter that emits an IR [`Score`] as a Standard MIDI File.
pub struct IrToMidiAdapter {
    /// Default note velocity (0–127).
    velocity: u8,
    /// Ticks per quarter note in the output MIDI file.
    divisions: u16,
}

impl IrToMidiAdapter {
    pub fn new() -> Self {
        Self {
            velocity: 80,
            divisions: 384,
        }
    }

    /// Set the default note velocity.
    pub fn with_velocity(mut self, vel: u8) -> Self {
        self.velocity = vel;
        self
    }

    /// Set the output divisions (ticks per quarter note).
    pub fn with_divisions(mut self, divisions: u16) -> Self {
        self.divisions = divisions;
        self
    }

    /// Render a score to raw MIDI bytes.
    pub fn convert_bytes(&self, score: &Score) -> Result<Vec<u8>> {
        let header = Header::new(Format::Parallel, Timing::Metrical(u15::new(self.divisions)));
        let mut smf = Smf::new(header);

        // Track 0: conductor (tempo, time sig, key sig)
        smf.tracks.push(self.build_conductor_track(score));

        // One track per part (or per staff for multi-staff parts). Use the
        // part's explicit channel, else hand out the next free one, skipping
        // ch 9 (percussion).
        let mut ch: u8 = 0;
        let mut next_channel = |explicit: u8| -> u8 {
            if explicit != 0 {
                return explicit;
            }
            let c = ch;
            ch += 1;
            if ch == 9 {
                ch = 10;
            }
            c
        };
        for part in score.parts() {
            if part.staves > 1 {
                // Split multi-staff part into per-staff tracks
                for staff_num in 1..=part.staves {
                    let channel = next_channel(part.midi_channel);
                    let sub_part = self.filter_part_by_staff(part, staff_num);
                    smf.tracks.push(self.build_part_track(&sub_part, channel));
                }
            } else {
                let channel = next_channel(part.midi_channel);
                smf.tracks.push(self.build_part_track(part, channel));
            }
        }

        let mut buf = Vec::new();
        smf.write(&mut buf)
            .map_err(|e| AdapterError::Parse(format!("MIDI write error: {e}")))?;
        Ok(buf)
    }
}

impl IrToMidiAdapter {
    /// Create a sub-part containing only voices for a specific staff number.
    fn filter_part_by_staff(
        &self,
        part: &crate::ir::part::Part,
        staff_num: u8,
    ) -> crate::ir::part::Part {
        use crate::ir::note::VoiceElement;

        let mut sub = part.clone();
        sub.staves = 1;
        sub.name = if part.name.is_empty() {
            format!("Staff {staff_num}")
        } else {
            format!("{} {staff_num}", part.name)
        };

        for measure in &mut sub.measures {
            measure.voices.retain(|voice| {
                voice.elements.iter().any(|e| {
                    let s = match e {
                        VoiceElement::Note(n) => n.staff,
                        VoiceElement::Rest(r) => r.staff,
                        VoiceElement::Chord(c) => c.staff,
                    };
                    s == staff_num || s == 0
                })
            });
        }
        sub
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
// Conductor track (tempo, time signature, key signature)
// ---------------------------------------------------------------------------

impl IrToMidiAdapter {
    fn build_conductor_track<'a>(&self, score: &Score) -> Vec<TrackEvent<'a>> {
        let mut events: Vec<TrackEvent<'a>> = Vec::new();
        let mut abs_tick: u64 = 0;
        let mut last_emit_tick: u64 = 0;

        // Emit default tempo (120 BPM = 500000 µs/quarter) at tick 0
        // if no explicit tempo is found in measure 1.
        let mut has_initial_tempo = false;

        // Tempo, time and key signatures can live on any part, not just the
        // first (e.g. a score where only an inner staff declares `\time`).
        // Aggregate them per measure index across every part, taking the first
        // part that declares each at a given index.
        let parts = score.parts();
        let measure_count = parts.iter().map(|p| p.measures.len()).max().unwrap_or(0);
        // The running meter drives the per-measure tick advance even for parts
        // that never declare a time signature themselves.
        let mut current_ts: Option<crate::ir::measure::TimeSignature> = None;

        for mi in 0..measure_count {
            let measures_at: Vec<&crate::ir::measure::Measure> =
                parts.iter().filter_map(|p| p.measures.get(mi)).collect();

            // Tempo: first part with a tempo direction at this measure.
            if let Some(tempo) = measures_at
                .iter()
                .flat_map(|m| &m.directions)
                .find_map(|d| {
                    d.tempo
                        .as_ref()
                        .filter(|t| t.per_minute.unwrap_or(0.0) > 0.0)
                })
            {
                let bpm = tempo.per_minute.unwrap();
                let beat_quarters = beat_unit_to_quarters(tempo.beat_unit.as_deref(), tempo.dots);
                let quarter_bpm = bpm * beat_quarters;
                let uspq = (60_000_000.0 / quarter_bpm).round() as u32;
                let delta = abs_tick - last_emit_tick;
                events.push(TrackEvent {
                    delta: u28::new(delta as u32),
                    kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(uspq))),
                });
                last_emit_tick = abs_tick;
                if abs_tick == 0 {
                    has_initial_tempo = true;
                }
            }

            // Time signature: first part declaring one at this measure.
            if let Some(ts) = measures_at
                .iter()
                .find_map(|m| m.attributes.as_ref().and_then(|a| a.time.as_ref()))
            {
                let num: u8 = ts
                    .beats
                    .split('+')
                    .filter_map(|b| b.trim().parse::<u8>().ok())
                    .sum();
                let den_pow = (ts.beat_type as f64).log2() as u8;
                let delta = abs_tick - last_emit_tick;
                events.push(TrackEvent {
                    delta: u28::new(delta as u32),
                    kind: TrackEventKind::Meta(MetaMessage::TimeSignature(num, den_pow, 24, 8)),
                });
                last_emit_tick = abs_tick;
                current_ts = Some(ts.clone());
            }

            // Key signature: first part declaring one at this measure.
            if let Some(ks) = measures_at
                .iter()
                .find_map(|m| m.attributes.as_ref().and_then(|a| a.key.as_ref()))
            {
                let delta = abs_tick - last_emit_tick;
                events.push(TrackEvent {
                    delta: u28::new(delta as u32),
                    kind: TrackEventKind::Meta(MetaMessage::KeySignature(
                        ks.fifths,
                        ks.mode == KeyMode::Minor,
                    )),
                });
                last_emit_tick = abs_tick;
            }

            // Advance by the unified meter (or 4/4 until one is seen).
            abs_tick += match &current_ts {
                Some(ts) => {
                    let ticks_frac =
                        ts.beats_fraction() * Ratio::from_integer(4 * self.divisions as i64);
                    (*ticks_frac.numer() / *ticks_frac.denom()).max(0) as u64
                }
                None => 4 * self.divisions as u64,
            };
        }

        // Insert default tempo at tick 0 if none was found.
        if !has_initial_tempo {
            events.insert(
                0,
                TrackEvent {
                    delta: u28::new(0),
                    kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(500_000))),
                },
            );
        }

        // End of track
        let delta = 0u32;
        events.push(TrackEvent {
            delta: u28::new(delta),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });

        events
    }

    // -----------------------------------------------------------------------
    // Part track
    // -----------------------------------------------------------------------

    fn build_part_track<'a>(
        &self,
        part: &crate::ir::part::Part,
        midi_channel: u8,
    ) -> Vec<TrackEvent<'a>> {
        let mut events: Vec<TrackEvent<'a>> = Vec::new();
        let mut abs_tick: u64 = 0;

        // Track name
        let name = if part.name.is_empty() {
            &part.part_id
        } else {
            &part.name
        };
        // We need 'static lifetime — store as owned bytes via leak.
        // This is acceptable since MIDI writing happens once and the
        // memory for track names is negligible.
        let name_bytes: &'static [u8] = Vec::leak(name.as_bytes().to_vec());
        events.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(name_bytes)),
        });

        // Program change — resolve from midi_instrument name if midi_program is 0
        let program = if part.midi_program != 0 {
            part.midi_program
        } else {
            gm_program_from_name(&part.midi_instrument)
        };
        events.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel: u4::new(midi_channel.min(15)),
                message: MidiMessage::ProgramChange {
                    program: u7::new(program.min(127)),
                },
            },
        });

        let channel = u4::new(midi_channel.min(15));
        // Running velocity, updated whenever a note/chord carries a dynamic mark.
        // Dynamics persist until the next dynamic, matching MIDI playback.
        let mut cur_vel = self.velocity.min(127);

        // Collect all note events with absolute ticks, then sort.
        // This is necessary because multiple voices in a measure overlap
        // in time, and MIDI delta encoding requires chronological order.
        let mut timed: Vec<(u64, TrackEventKind<'a>)> = Vec::new();

        // The running meter drives each bar's expected length even when the bar
        // doesn't re-declare `<time>` (a `\time 3/8` piece declares it once, then
        // every later 3/8 bar inherits it). Without this carry, un-declared bars
        // were padded to the 4/4 default, shifting every later onset forward on
        // export and breaking the MIDI round trip (pedal, example2_1). Mirrors
        // the conductor track's `current_ts` carry.
        let mut current_ts: Option<crate::ir::measure::TimeSignature> = None;

        for measure in &part.measures {
            let measure_start = abs_tick;
            if let Some(ts) = measure.attributes.as_ref().and_then(|a| a.time.as_ref()) {
                current_ts = Some(ts.clone());
            }

            for voice in &measure.voices {
                let mut voice_tick = measure_start;

                for elem in &voice.elements {
                    match elem {
                        VoiceElement::Note(note) => {
                            let dur_ticks = self.duration_to_ticks(&note.duration);
                            let midi_key = note.pitch.midi_number().clamp(0, 127) as u8;

                            if let Some(d) = note.dynamics.last() {
                                cur_vel = dynamic_to_velocity(&d.sign);
                            }
                            let vel = u7::new(cur_vel);

                            if note.is_grace {
                                // Grace notes ornament the following note and must NOT
                                // consume metrical time: advancing voice_tick by their
                                // notated duration shifted every later onset, overflowed
                                // the bar, and on a round trip split/duplicated the
                                // displaced notes. Emit a short grace at the current tick
                                // and leave voice_tick untouched — mirroring the
                                // note-array representation (grace = zero duration).
                                let grace_len = dur_ticks.min(self.divisions as u64 / 2).max(1);
                                timed.push((
                                    voice_tick,
                                    TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOn {
                                            key: u7::new(midi_key),
                                            vel,
                                        },
                                    },
                                ));
                                timed.push((
                                    voice_tick + grace_len,
                                    TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOff {
                                            key: u7::new(midi_key),
                                            vel: u7::new(64),
                                        },
                                    },
                                ));
                                // No voice_tick advance.
                                continue;
                            }

                            // A tie chain is ONE sounding note: emit NoteOn only when
                            // this note begins it (not a continuation, i.e. no Stop) and
                            // NoteOff only when it ends it (not tied onward, i.e. no
                            // Start). Otherwise midi->IR (which splits notes across
                            // barlines into tied segments) -> midi would multiply the
                            // note-on/off events on every round trip.
                            let (has_start, has_stop) = tie_flags(&note.ties);
                            if !has_stop {
                                timed.push((
                                    voice_tick,
                                    TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOn {
                                            key: u7::new(midi_key),
                                            vel,
                                        },
                                    },
                                ));
                            }
                            if !has_start {
                                timed.push((
                                    voice_tick + dur_ticks,
                                    TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOff {
                                            key: u7::new(midi_key),
                                            vel: u7::new(64),
                                        },
                                    },
                                ));
                            }

                            voice_tick += dur_ticks;
                        }
                        VoiceElement::Rest(rest) => {
                            let dur_ticks = self.duration_to_ticks(&rest.duration);
                            voice_tick += dur_ticks;
                        }
                        VoiceElement::Chord(chord) => {
                            let dur_ticks = self.duration_to_ticks(&chord.duration);
                            if let Some(d) = chord.notes.iter().find_map(|n| n.dynamics.last()) {
                                cur_vel = dynamic_to_velocity(&d.sign);
                            }
                            let vel = u7::new(cur_vel);
                            for cn in &chord.notes {
                                let midi_key = cn.pitch.midi_number().clamp(0, 127) as u8;
                                timed.push((
                                    voice_tick,
                                    TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOn {
                                            key: u7::new(midi_key),
                                            vel,
                                        },
                                    },
                                ));
                                timed.push((
                                    voice_tick + dur_ticks,
                                    TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOff {
                                            key: u7::new(midi_key),
                                            vel: u7::new(64),
                                        },
                                    },
                                ));
                            }
                            voice_tick += dur_ticks;
                        }
                    }
                }

                if voice_tick > abs_tick {
                    abs_tick = voice_tick;
                }
            }

            // Ensure abs_tick advances by at least the measure's expected duration.
            // A senza-misura (free-time) bar has no fixed length, so its content
            // alone sets the boundary — never pad it to a meter.
            if !measure.senza_misura {
                let expected_end = measure_start + self.ticks_for_ts(current_ts.as_ref());
                if abs_tick < expected_end {
                    abs_tick = expected_end;
                }
            }
        }

        // Sort by absolute tick (stable sort preserves NoteOn-before-NoteOff
        // for simultaneous events within the same voice).
        timed.sort_by_key(|&(tick, _)| tick);

        // Convert absolute ticks to delta encoding.
        let mut last_tick: u64 = 0;
        for (tick, kind) in timed {
            let delta = tick - last_tick;
            events.push(TrackEvent {
                delta: u28::new(delta as u32),
                kind,
            });
            last_tick = tick;
        }

        // End of track
        events.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });

        events
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Convert a Duration to ticks at the configured divisions.
    fn duration_to_ticks(&self, dur: &crate::ir::duration::Duration) -> u64 {
        let actual = dur.actual_duration();
        // ticks = actual_duration_in_whole_notes * 4 * divisions
        let ticks_frac = actual * Frac::from_integer(4 * self.divisions as i64);
        let ticks = *ticks_frac.numer() / *ticks_frac.denom();
        ticks.max(0) as u64
    }

    /// Total ticks of a bar in the given (carried) time signature; 4/4 if none.
    fn ticks_for_ts(&self, ts: Option<&crate::ir::measure::TimeSignature>) -> u64 {
        match ts {
            Some(ts) => {
                let beats_frac = ts.beats_fraction();
                // ticks = beats_frac * 4 * divisions  (since quarter = 1/4 whole)
                let ticks_frac = beats_frac * Ratio::from_integer(4 * self.divisions as i64);
                (*ticks_frac.numer() / *ticks_frac.denom()).max(0) as u64
            }
            None => 4 * self.divisions as u64,
        }
    }
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
        // Part 0: a half + half (no time/key/tempo declared).
        let mut v0 = Voice::new(1);
        v0.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::half(),
        ))));
        v0.elements.push(VoiceElement::Note(Box::new(Note::new(
            Pitch::new(PitchStep::D, 4),
            Duration::half(),
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
    fn test_duration_to_ticks() {
        let adapter = IrToMidiAdapter::new(); // divisions=384
        assert_eq!(adapter.duration_to_ticks(&Duration::quarter()), 384);
        assert_eq!(adapter.duration_to_ticks(&Duration::half()), 768);
        assert_eq!(adapter.duration_to_ticks(&Duration::whole()), 1536);
        assert_eq!(adapter.duration_to_ticks(&Duration::eighth()), 192);
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
}
