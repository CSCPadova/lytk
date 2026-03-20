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

/// Map a LilyPond `\set Staff.midiInstrument` name to a General MIDI program number (0–127).
/// Returns 0 (Acoustic Grand Piano) for unrecognised names.
fn gm_program_from_name(name: &str) -> u8 {
    match name.to_ascii_lowercase().trim() {
        // Piano
        "acoustic grand" | "acoustic grand piano" => 0,
        "bright acoustic" | "bright acoustic piano" => 1,
        "electric grand" | "electric grand piano" => 2,
        "honky-tonk" | "honky-tonk piano" => 3,
        "electric piano 1" | "rhodes piano" => 4,
        "electric piano 2" | "chorused piano" => 5,
        "harpsichord" => 6,
        "clavinet" | "clav" => 7,
        // Chromatic percussion
        "celesta" => 8,
        "glockenspiel" => 9,
        "music box" => 10,
        "vibraphone" => 11,
        "marimba" => 12,
        "xylophone" => 13,
        "tubular bells" => 14,
        "dulcimer" => 15,
        // Organ
        "drawbar organ" => 16,
        "percussive organ" => 17,
        "rock organ" => 18,
        "church organ" | "church organ reed" => 19,
        "reed organ" => 20,
        "accordion" => 21,
        "harmonica" => 22,
        "concertina" => 23,
        // Guitar
        "acoustic guitar (nylon)" | "nylon string guitar" => 24,
        "acoustic guitar (steel)" | "steel string guitar" => 25,
        "electric guitar (jazz)" => 26,
        "electric guitar (clean)" => 27,
        "electric guitar (muted)" => 28,
        "overdriven guitar" => 29,
        "distorted guitar" => 30,
        "guitar harmonics" => 31,
        // Bass
        "acoustic bass" => 32,
        "electric bass (finger)" | "electric bass" => 33,
        "electric bass (pick)" => 34,
        "fretless bass" => 35,
        "slap bass 1" => 36,
        "slap bass 2" => 37,
        "synth bass 1" => 38,
        "synth bass 2" => 39,
        // Strings
        "violin" => 40,
        "viola" => 41,
        "cello" => 42,
        "contrabass" | "double bass" => 43,
        "tremolo strings" => 44,
        "pizzicato strings" => 45,
        "orchestral harp" | "harp" => 46,
        "timpani" => 47,
        // Ensemble
        "string ensemble 1" | "string ensemble" => 48,
        "string ensemble 2" => 49,
        "synthstrings 1" | "synth strings 1" => 50,
        "synthstrings 2" | "synth strings 2" => 51,
        "choir aahs" => 52,
        "voice oohs" => 53,
        "synth voice" => 54,
        "orchestra hit" => 55,
        // Brass
        "trumpet" => 56,
        "trombone" => 57,
        "tuba" => 58,
        "muted trumpet" => 59,
        "french horn" => 60,
        "brass section" => 61,
        "synthbrass 1" | "synth brass 1" => 62,
        "synthbrass 2" | "synth brass 2" => 63,
        // Reed
        "soprano sax" => 64,
        "alto sax" => 65,
        "tenor sax" => 66,
        "baritone sax" => 67,
        "oboe" => 68,
        "english horn" => 69,
        "bassoon" => 70,
        "clarinet" => 71,
        // Pipe
        "piccolo" => 72,
        "flute" => 73,
        "recorder" => 74,
        "pan flute" => 75,
        "blown bottle" | "bottle" => 76,
        "shakuhachi" => 77,
        "whistle" => 78,
        "ocarina" => 79,
        // Synth lead
        "lead 1 (square)" | "square" => 80,
        "lead 2 (sawtooth)" | "sawtooth" => 81,
        "lead 3 (calliope)" => 82,
        "lead 4 (chiff)" => 83,
        "lead 5 (charang)" => 84,
        "lead 6 (voice)" => 85,
        "lead 7 (fifths)" => 86,
        "lead 8 (bass+lead)" => 87,
        // Synth pad
        "pad 1 (new age)" => 88,
        "pad 2 (warm)" => 89,
        "pad 3 (polysynth)" => 90,
        "pad 4 (choir)" => 91,
        "pad 5 (bowed)" => 92,
        "pad 6 (metallic)" => 93,
        "pad 7 (halo)" => 94,
        "pad 8 (sweep)" => 95,
        // Ethnic / Percussive / Sound effects
        "sitar" => 104,
        "banjo" => 105,
        "shamisen" => 106,
        "koto" => 107,
        "bagpipe" => 109,
        "fiddle" => 110,
        "shanai" => 111,
        _ => 0,
    }
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

        // One track per part (assign channels sequentially, skip ch 9 = percussion)
        let mut ch: u8 = 0;
        for part in score.parts() {
            let channel = if part.midi_channel != 0 {
                part.midi_channel
            } else {
                let c = ch;
                ch += 1;
                if ch == 9 {
                    ch = 10; // skip GM percussion channel
                }
                c
            };
            smf.tracks.push(self.build_part_track(part, channel));
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

        if let Some(part) = score.parts().into_iter().next() {
            for measure in &part.measures {
                // We only need one part's structure for conductor events.
                // Check directions for tempo.
                for dir in &measure.directions {
                    if let Some(tempo) = &dir.tempo {
                        if let Some(bpm) = tempo.per_minute {
                            if bpm > 0.0 {
                                // Convert beat-unit BPM to quarter-note BPM.
                                // MIDI tempo is always µs per quarter note.
                                let beat_quarters =
                                    beat_unit_to_quarters(tempo.beat_unit.as_deref(), tempo.dots);
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
        let vel = u7::new(self.velocity.min(127));

        // Collect all note events with absolute ticks, then sort.
        // This is necessary because multiple voices in a measure overlap
        // in time, and MIDI delta encoding requires chronological order.
        let mut timed: Vec<(u64, TrackEventKind<'a>)> = Vec::new();

        for measure in &part.measures {
            let measure_start = abs_tick;

            for voice in &measure.voices {
                let mut voice_tick = measure_start;

                for elem in &voice.elements {
                    match elem {
                        VoiceElement::Note(note) => {
                            let dur_ticks = self.duration_to_ticks(&note.duration);
                            let midi_key = note.pitch.midi_number().clamp(0, 127) as u8;

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

                            voice_tick += dur_ticks;
                        }
                        VoiceElement::Rest(rest) => {
                            let dur_ticks = self.duration_to_ticks(&rest.duration);
                            voice_tick += dur_ticks;
                        }
                        VoiceElement::Chord(chord) => {
                            let dur_ticks = self.duration_to_ticks(&chord.duration);
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
            let expected_end = measure_start + self.measure_ticks(measure);
            if abs_tick < expected_end {
                abs_tick = expected_end;
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
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        }
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
