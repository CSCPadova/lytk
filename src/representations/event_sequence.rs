//! Event-based representation (EDT2).
//!
//! A piece is encoded as a flat sequence of integer event codes, mirroring
//! `muspy`'s event representation (and the "Performance RNN" style encoding).
//! The vocabulary, with `S = max_time_shift` and `V = velocity_bins`:
//!
//! | code range            | event        | meaning                              |
//! |-----------------------|--------------|--------------------------------------|
//! | `0 .. 128`            | note-on      | `code` = MIDI pitch                  |
//! | `128 .. 256`          | note-off     | `code - 128` = MIDI pitch            |
//! | `256 .. 256+S`        | time-shift   | advance `code - 256 + 1` time steps  |
//! | `256+S .. 256+S+V`    | velocity-set | set running velocity to bin `code-256-S` |
//!
//! Time shifts larger than `S` steps decompose into several time-shift events.
//! Velocity is quantised into `V` bins (`bin = velocity * V / 128`); decoding
//! is therefore banded. Encoding/decoding go through a [`NoteArray`], so the
//! resolution (time steps per quarter note) is inherited from it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::note_array::{NoteArray, NoteRow, DEFAULT_VELOCITY};

/// Default maximum time-shift, in time steps, encodable as a single event.
pub const DEFAULT_MAX_TIME_SHIFT: u32 = 100;

/// Default number of velocity bins.
pub const DEFAULT_VELOCITY_BINS: u8 = 32;

const OFFSET_NOTE_ON: u32 = 0;
const OFFSET_NOTE_OFF: u32 = 128;
const OFFSET_TIME_SHIFT: u32 = 256;

/// Options controlling the event vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventOptions {
    /// Largest single time-shift, in time steps.
    pub max_time_shift: u32,
    /// Number of velocity bins.
    pub velocity_bins: u8,
    /// Whether to emit velocity-set events (when false, velocity is dropped and
    /// decoding uses the default velocity).
    pub encode_velocity: bool,
}

impl Default for EventOptions {
    fn default() -> Self {
        Self {
            max_time_shift: DEFAULT_MAX_TIME_SHIFT,
            velocity_bins: DEFAULT_VELOCITY_BINS,
            encode_velocity: true,
        }
    }
}

/// An event-based encoding of a piece. Carries the vocabulary parameters so it
/// can be decoded without external context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSequence {
    /// Event codes.
    pub codes: Vec<u32>,
    /// Time steps per quarter note (inherited from the source note-array).
    pub resolution: u16,
    /// Largest single time-shift, in time steps.
    pub max_time_shift: u32,
    /// Number of velocity bins.
    pub velocity_bins: u8,
    /// Whether velocity-set events are present.
    pub encode_velocity: bool,
}

impl EventSequence {
    /// First code of the velocity-set range.
    fn offset_velocity(&self) -> u32 {
        OFFSET_TIME_SHIFT + self.max_time_shift
    }

    /// The size of the event vocabulary.
    pub fn vocab_size(&self) -> u32 {
        let mut v = OFFSET_TIME_SHIFT + self.max_time_shift;
        if self.encode_velocity {
            v += self.velocity_bins as u32;
        }
        v
    }

    /// A human-readable name for an event code (for inspection/debugging).
    pub fn event_name(&self, code: u32) -> String {
        let off_vel = self.offset_velocity();
        if code < OFFSET_NOTE_OFF {
            format!("note-on:{code}")
        } else if code < OFFSET_TIME_SHIFT {
            format!("note-off:{}", code - OFFSET_NOTE_OFF)
        } else if code < off_vel {
            format!("time-shift:{}", code - OFFSET_TIME_SHIFT + 1)
        } else if self.encode_velocity && code < off_vel + self.velocity_bins as u32 {
            format!("velocity:{}", code - off_vel)
        } else {
            format!("unknown:{code}")
        }
    }
}

/// Quantise a MIDI velocity into a bin index `0 .. velocity_bins`.
fn velocity_to_bin(velocity: u8, bins: u8) -> u32 {
    (velocity as u32 * bins as u32 / 128).min(bins as u32 - 1)
}

/// Inverse of [`velocity_to_bin`] (banded).
fn bin_to_velocity(bin: u32, bins: u8) -> u8 {
    (bin * 128 / bins as u32).clamp(1, 127) as u8
}

/// Encode a [`NoteArray`] as an [`EventSequence`].
pub fn to_event_sequence(arr: &NoteArray, opts: &EventOptions) -> EventSequence {
    let off_velocity = OFFSET_TIME_SHIFT + opts.max_time_shift;

    // Build timed (time, code) events. Notes are already sorted by
    // (onset, pitch, duration, velocity) in the NoteArray.
    let mut timed: Vec<(u32, u32)> = Vec::with_capacity(arr.notes.len() * 3);
    let mut last_bin: i64 = -1;
    for n in &arr.notes {
        if opts.encode_velocity {
            let bin = velocity_to_bin(n.velocity, opts.velocity_bins);
            if bin as i64 != last_bin {
                timed.push((n.onset, off_velocity + bin));
                last_bin = bin as i64;
            }
        }
        timed.push((n.onset, OFFSET_NOTE_ON + n.pitch as u32));
        timed.push((n.onset + n.duration, OFFSET_NOTE_OFF + n.pitch as u32));
    }

    // Stable sort by time keeps velocity-set before its note-on, and a note-off
    // before a same-time note-on of a following note.
    timed.sort_by_key(|(t, _)| *t);

    // Walk, inserting decomposed time-shifts between event times.
    let mut codes: Vec<u32> = Vec::with_capacity(timed.len());
    let mut cursor = 0u32;
    for (time, code) in timed {
        if time > cursor {
            let mut delta = time - cursor;
            while delta > opts.max_time_shift {
                codes.push(OFFSET_TIME_SHIFT + opts.max_time_shift - 1);
                delta -= opts.max_time_shift;
            }
            if delta > 0 {
                codes.push(OFFSET_TIME_SHIFT + delta - 1);
            }
            cursor = time;
        }
        codes.push(code);
    }

    EventSequence {
        codes,
        resolution: arr.resolution,
        max_time_shift: opts.max_time_shift,
        velocity_bins: opts.velocity_bins,
        encode_velocity: opts.encode_velocity,
    }
}

/// Decode an [`EventSequence`] back into a [`NoteArray`].
///
/// Note-offs are matched to the earliest open note-on of the same pitch (FIFO).
pub fn from_event_sequence(seq: &EventSequence) -> NoteArray {
    let off_velocity = seq.offset_velocity();
    let vocab = seq.vocab_size();

    let mut time = 0u32;
    let mut velocity = DEFAULT_VELOCITY;
    // pitch → queue of (onset, velocity) for open note-ons.
    let mut active: HashMap<u8, Vec<(u32, u8)>> = HashMap::new();
    let mut notes: Vec<NoteRow> = Vec::new();

    for &code in &seq.codes {
        if code >= vocab {
            continue; // unknown / out of range
        }
        if code < OFFSET_NOTE_OFF {
            let pitch = code as u8;
            active.entry(pitch).or_default().push((time, velocity));
        } else if code < OFFSET_TIME_SHIFT {
            let pitch = (code - OFFSET_NOTE_OFF) as u8;
            if let Some(q) = active.get_mut(&pitch) {
                if !q.is_empty() {
                    let (onset, vel) = q.remove(0); // FIFO
                    notes.push(NoteRow {
                        onset,
                        duration: time.saturating_sub(onset),
                        pitch,
                        velocity: vel,
                    });
                }
            }
        } else if code < off_velocity {
            time += code - OFFSET_TIME_SHIFT + 1;
        } else if seq.encode_velocity {
            let bin = code - off_velocity;
            velocity = bin_to_velocity(bin, seq.velocity_bins);
        }
    }

    notes.sort_by_key(|n| (n.onset, n.pitch, n.duration, n.velocity));
    NoteArray {
        resolution: seq.resolution,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::music::{Music, MusicDocument};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::representations::note_array::to_note_array;

    fn note(step: PitchStep, octave: i32, dur: Duration) -> Music {
        Music::Note {
            pitch: Pitch::new(step, octave),
            duration: dur,
            annotations: vec![],
        }
    }

    fn array_from(music: Music, resolution: u16) -> NoteArray {
        to_note_array(&MusicDocument::new(music), resolution)
    }

    #[test]
    fn test_single_note_event_codes() {
        // C4 quarter at resolution 24 (quarter = 24 steps), default options.
        let arr = array_from(note(PitchStep::C, 4, Duration::quarter()), 24);
        let seq = to_event_sequence(&arr, &EventOptions::default());
        // velocity 64 → bin 16 → 356+16 = 372; note-on 60; time-shift 24 →
        // 256+23 = 279; note-off 188.
        assert_eq!(seq.codes, vec![372, 60, 279, 188]);
        assert_eq!(seq.event_name(372), "velocity:16");
        assert_eq!(seq.event_name(60), "note-on:60");
        assert_eq!(seq.event_name(279), "time-shift:24");
        assert_eq!(seq.event_name(188), "note-off:60");
    }

    #[test]
    fn test_time_shift_decomposition() {
        // Quarter at resolution 480 = 480 steps; max_time_shift 100 →
        // 4 × (256+99=355) + (256+79=335).
        let arr = array_from(note(PitchStep::C, 4, Duration::quarter()), 480);
        let seq = to_event_sequence(&arr, &EventOptions::default());
        let shifts: Vec<u32> = seq
            .codes
            .iter()
            .copied()
            .filter(|&c| (OFFSET_TIME_SHIFT..OFFSET_TIME_SHIFT + 100).contains(&c))
            .collect();
        assert_eq!(shifts, vec![355, 355, 355, 355, 335]);
    }

    #[test]
    fn test_roundtrip_melody_exact() {
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::D, 4, Duration::quarter()),
            note(PitchStep::E, 4, Duration::half()),
        ]);
        let arr = array_from(m, 24);
        let seq = to_event_sequence(&arr, &EventOptions::default());
        let back = from_event_sequence(&seq);
        // Default velocity (64) is a bin centre, so the round-trip is exact.
        assert_eq!(arr, back);
    }

    #[test]
    fn test_roundtrip_chord_exact() {
        let m = Music::Chord {
            pitches: vec![
                (Pitch::new(PitchStep::C, 4), vec![]),
                (Pitch::new(PitchStep::E, 4), vec![]),
                (Pitch::new(PitchStep::G, 4), vec![]),
            ],
            duration: Duration::half(),
            annotations: vec![],
        };
        let arr = array_from(m, 24);
        let seq = to_event_sequence(&arr, &EventOptions::default());
        assert_eq!(arr, from_event_sequence(&seq));
    }

    #[test]
    fn test_velocity_change_roundtrips_banded() {
        // Velocities 64 and 96 are bin-exact (multiples of 4 with 32 bins).
        let mut soft = note(PitchStep::C, 4, Duration::quarter());
        if let Music::Note { annotations, .. } = &mut soft {
            annotations.push(crate::ir::annotation::Annotation::Dynamic(
                crate::ir::articulation::DynamicMark {
                    sign: "mp".to_string(),
                    placement: Default::default(),
                },
            ));
        }
        let mut loud = note(PitchStep::D, 4, Duration::quarter());
        if let Music::Note { annotations, .. } = &mut loud {
            annotations.push(crate::ir::annotation::Annotation::Dynamic(
                crate::ir::articulation::DynamicMark {
                    sign: "f".to_string(),
                    placement: Default::default(),
                },
            ));
        }
        let arr = array_from(Music::Sequential(vec![soft, loud]), 24);
        let seq = to_event_sequence(&arr, &EventOptions::default());
        // Two distinct velocity-set events.
        let vel_events: Vec<u32> = seq
            .codes
            .iter()
            .copied()
            .filter(|&c| c >= seq.offset_velocity())
            .collect();
        assert_eq!(vel_events.len(), 2);
        // mp=64 and f=96 are bin centres → exact round-trip.
        let back = from_event_sequence(&seq);
        let bvel: Vec<u8> = {
            let mut v: Vec<_> = back.notes.iter().map(|n| n.velocity).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(bvel, vec![64, 96]);
    }

    #[test]
    fn test_encode_velocity_false_has_no_velocity_events() {
        let arr = array_from(note(PitchStep::C, 4, Duration::quarter()), 24);
        let opts = EventOptions {
            encode_velocity: false,
            ..Default::default()
        };
        let seq = to_event_sequence(&arr, &opts);
        assert!(seq
            .codes
            .iter()
            .all(|&c| c < OFFSET_TIME_SHIFT + opts.max_time_shift));
        // Onset/duration/pitch still round-trip; velocity falls back to default.
        let back = from_event_sequence(&seq);
        assert_eq!(back.notes.len(), 1);
        assert_eq!(back.notes[0].pitch, 60);
        assert_eq!(back.notes[0].duration, 24);
        assert_eq!(back.notes[0].velocity, DEFAULT_VELOCITY);
    }

    #[test]
    fn test_vocab_size() {
        let seq = to_event_sequence(
            &array_from(note(PitchStep::C, 4, Duration::quarter()), 24),
            &EventOptions::default(),
        );
        // 256 + 100 time-shift + 32 velocity.
        assert_eq!(seq.vocab_size(), 388);
        let seq2 = to_event_sequence(
            &array_from(note(PitchStep::C, 4, Duration::quarter()), 24),
            &EventOptions {
                encode_velocity: false,
                ..Default::default()
            },
        );
        assert_eq!(seq2.vocab_size(), 356);
    }
}
