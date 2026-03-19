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

use super::{AdapterError, FromIrAdapter, Result};
use crate::ir::duration::Frac;
use crate::ir::measure::KeyMode;
use crate::ir::note::VoiceElement;
use crate::ir::Score;

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
            divisions: 480,
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

        // One track per part
        for part in score.parts() {
            smf.tracks.push(self.build_part_track(part));
        }

        let mut buf = Vec::new();
        smf.write(&mut buf)
            .map_err(|e| AdapterError::Parse(format!("MIDI write error: {e}")))?;
        Ok(buf)
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

        if let Some(part) = score.parts().into_iter().next() {
            for measure in &part.measures {
                // We only need one part's structure for conductor events.
                // Check directions for tempo.
                for dir in &measure.directions {
                    if let Some(tempo) = &dir.tempo {
                        if let Some(bpm) = tempo.per_minute {
                            if bpm > 0.0 {
                                let uspq = (60_000_000.0 / bpm).round() as u32;
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
                        }
                    }
                }

                // Check attributes for time sig and key sig changes.
                if let Some(attrs) = &measure.attributes {
                    if let Some(ts) = &attrs.time {
                        let num: u8 = ts
                            .beats
                            .split('+')
                            .filter_map(|b| b.trim().parse::<u8>().ok())
                            .sum();
                        let den_pow = (ts.beat_type as f64).log2() as u8;
                        let delta = abs_tick - last_emit_tick;
                        events.push(TrackEvent {
                            delta: u28::new(delta as u32),
                            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(
                                num, den_pow, 24, // MIDI clocks per metronome click
                                8,  // 32nd notes per quarter note
                            )),
                        });
                        last_emit_tick = abs_tick;
                    }
                    if let Some(ks) = &attrs.key {
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
                }

                // Advance abs_tick by this measure's duration.
                abs_tick += self.measure_ticks(measure);
            }
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

    fn build_part_track<'a>(&self, part: &crate::ir::part::Part) -> Vec<TrackEvent<'a>> {
        let mut events: Vec<TrackEvent<'a>> = Vec::new();
        let mut last_emit_tick: u64 = 0;
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

        // Program change
        events.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel: u4::new(part.midi_channel.min(15)),
                message: MidiMessage::ProgramChange {
                    program: u7::new(part.midi_program.min(127)),
                },
            },
        });

        let channel = u4::new(part.midi_channel.min(15));
        let vel = u7::new(self.velocity.min(127));

        for measure in &part.measures {
            let measure_start = abs_tick;

            for voice in &measure.voices {
                // Reset voice cursor to measure start.
                let mut voice_tick = measure_start;

                for elem in &voice.elements {
                    match elem {
                        VoiceElement::Note(note) => {
                            let dur_ticks = self.duration_to_ticks(&note.duration);
                            let midi_key = note.pitch.midi_number().clamp(0, 127) as u8;

                            // NoteOn
                            let on_delta = voice_tick - last_emit_tick;
                            events.push(TrackEvent {
                                delta: u28::new(on_delta as u32),
                                kind: TrackEventKind::Midi {
                                    channel,
                                    message: MidiMessage::NoteOn {
                                        key: u7::new(midi_key),
                                        vel,
                                    },
                                },
                            });
                            last_emit_tick = voice_tick;

                            // NoteOff
                            let off_tick = voice_tick + dur_ticks;
                            let off_delta = off_tick - last_emit_tick;
                            events.push(TrackEvent {
                                delta: u28::new(off_delta as u32),
                                kind: TrackEventKind::Midi {
                                    channel,
                                    message: MidiMessage::NoteOff {
                                        key: u7::new(midi_key),
                                        vel: u7::new(64),
                                    },
                                },
                            });
                            last_emit_tick = off_tick;

                            voice_tick += dur_ticks;
                        }
                        VoiceElement::Rest(rest) => {
                            let dur_ticks = self.duration_to_ticks(&rest.duration);
                            voice_tick += dur_ticks;
                        }
                        VoiceElement::Chord(chord) => {
                            let dur_ticks = self.duration_to_ticks(&chord.duration);
                            // All notes on simultaneously
                            for cn in &chord.notes {
                                let midi_key = cn.pitch.midi_number().clamp(0, 127) as u8;
                                let on_delta = voice_tick - last_emit_tick;
                                events.push(TrackEvent {
                                    delta: u28::new(on_delta as u32),
                                    kind: TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOn {
                                            key: u7::new(midi_key),
                                            vel,
                                        },
                                    },
                                });
                                last_emit_tick = voice_tick;
                            }
                            // All notes off
                            let off_tick = voice_tick + dur_ticks;
                            for cn in &chord.notes {
                                let midi_key = cn.pitch.midi_number().clamp(0, 127) as u8;
                                let off_delta = off_tick - last_emit_tick;
                                events.push(TrackEvent {
                                    delta: u28::new(off_delta as u32),
                                    kind: TrackEventKind::Midi {
                                        channel,
                                        message: MidiMessage::NoteOff {
                                            key: u7::new(midi_key),
                                            vel: u7::new(64),
                                        },
                                    },
                                });
                                last_emit_tick = off_tick;
                            }
                            voice_tick += dur_ticks;
                        }
                    }
                }

                // Update abs_tick to the furthest voice position.
                if voice_tick > abs_tick {
                    abs_tick = voice_tick;
                }
            }

            // Ensure abs_tick advances by at least the measure's expected duration.
            let expected_end = measure_start + self.measure_ticks(measure);
            if abs_tick < expected_end {
                abs_tick = expected_end;
            }
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

    /// Compute a measure's total duration in ticks based on its time signature.
    fn measure_ticks(&self, measure: &crate::ir::measure::Measure) -> u64 {
        if let Some(attrs) = &measure.attributes {
            if let Some(ts) = &attrs.time {
                let beats_frac = ts.beats_fraction();
                // beats_fraction is already beats/beat_type
                // ticks = beats_frac * 4 * divisions  (since quarter=1/4 of whole)
                let ticks_frac = beats_frac * Ratio::from_integer(4 * self.divisions as i64);
                return (*ticks_frac.numer() / *ticks_frac.denom()).max(0) as u64;
            }
        }
        // Default: 4/4
        4 * self.divisions as u64
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
            children: vec![ScoreChild::Part(part)],
        }
    }

    #[test]
    fn test_duration_to_ticks() {
        let adapter = IrToMidiAdapter::new(); // divisions=480
        assert_eq!(adapter.duration_to_ticks(&Duration::quarter()), 480);
        assert_eq!(adapter.duration_to_ticks(&Duration::half()), 960);
        assert_eq!(adapter.duration_to_ticks(&Duration::whole()), 1920);
        assert_eq!(adapter.duration_to_ticks(&Duration::eighth()), 240);
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
    fn test_convert_returns_unsupported() {
        let score = Score::new();
        let adapter = IrToMidiAdapter::new();
        assert!(adapter.convert(&score).is_err());
    }
}
