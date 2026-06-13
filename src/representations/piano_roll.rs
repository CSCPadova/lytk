//! Piano-roll representation (EDT3).
//!
//! A piece is encoded as a dense `T × 128` matrix (row-major, one row per time
//! step, one column per MIDI pitch), mirroring `muspy`'s piano-roll. Each note
//! fills the cells `[onset, onset+duration)` of its pitch column with its
//! velocity (or `1` in binary mode); empty cells are `0`.
//!
//! Decoding reconstructs notes from contiguous nonzero runs per pitch column.
//! This is **lossy for repeated same-pitch notes**: two back-to-back notes of
//! the same pitch with no gap merge into one held note (the classic piano-roll
//! limitation — there is no onset marker). Notes separated by at least one
//! empty step, and all distinct-pitch material, round-trip exactly.
//!
//! Encoding/decoding go through a [`NoteArray`], so the resolution (time steps
//! per quarter note) is inherited from it.

use serde::{Deserialize, Serialize};

use super::note_array::{NoteArray, NoteRow, DEFAULT_VELOCITY};

/// Number of MIDI pitches (columns).
pub const PITCH_COUNT: usize = 128;

/// A dense `T × 128` piano-roll matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PianoRoll {
    /// Time steps per quarter note (inherited from the source note-array).
    pub resolution: u16,
    /// Number of time steps `T` (rows).
    pub num_steps: u32,
    /// Whether cells hold velocities (`true`) or just on/off as `1`/`0`.
    pub encode_velocity: bool,
    /// Row-major `T × 128` data: `data[t * 128 + pitch]`.
    pub data: Vec<u8>,
}

impl PianoRoll {
    /// The value at time step `t`, pitch `pitch` (0 if out of range).
    pub fn cell(&self, t: u32, pitch: u8) -> u8 {
        if t >= self.num_steps || pitch as usize >= PITCH_COUNT {
            return 0;
        }
        self.data[t as usize * PITCH_COUNT + pitch as usize]
    }

    /// The matrix shape `(num_steps, 128)`.
    pub fn shape(&self) -> (usize, usize) {
        (self.num_steps as usize, PITCH_COUNT)
    }
}

/// Encode a [`NoteArray`] as a [`PianoRoll`]. When `encode_velocity` is false,
/// on-cells are `1`.
pub fn to_piano_roll(arr: &NoteArray, encode_velocity: bool) -> PianoRoll {
    let num_steps = arr.length();
    let mut data = vec![0u8; num_steps as usize * PITCH_COUNT];

    for n in &arr.notes {
        if n.pitch as usize >= PITCH_COUNT {
            continue;
        }
        let value = if encode_velocity {
            n.velocity.max(1)
        } else {
            1
        };
        let start = n.onset as usize;
        let end = (n.onset + n.duration).min(num_steps) as usize;
        for t in start..end {
            data[t * PITCH_COUNT + n.pitch as usize] = value;
        }
    }

    PianoRoll {
        resolution: arr.resolution,
        num_steps,
        encode_velocity,
        data,
    }
}

/// Decode a [`PianoRoll`] back into a [`NoteArray`].
///
/// Each contiguous nonzero run in a pitch column becomes one note; its velocity
/// is read at the run's start (or the default velocity in binary mode).
/// Adjacent same-pitch notes therefore merge.
pub fn from_piano_roll(pr: &PianoRoll) -> NoteArray {
    let mut notes: Vec<NoteRow> = Vec::new();

    for pitch in 0..PITCH_COUNT {
        let mut t = 0u32;
        while t < pr.num_steps {
            let v = pr.data[t as usize * PITCH_COUNT + pitch];
            if v == 0 {
                t += 1;
                continue;
            }
            // Start of a run.
            let start = t;
            let start_vel = v;
            while t < pr.num_steps && pr.data[t as usize * PITCH_COUNT + pitch] != 0 {
                t += 1;
            }
            let velocity = if pr.encode_velocity {
                start_vel
            } else {
                DEFAULT_VELOCITY
            };
            notes.push(NoteRow {
                onset: start,
                duration: t - start,
                pitch: pitch as u8,
                velocity,
            });
        }
    }

    notes.sort_by_key(|n| (n.onset, n.pitch, n.duration, n.velocity));
    NoteArray {
        resolution: pr.resolution,
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
    fn test_single_note_cells() {
        // C4 (60) quarter at resolution 4 → 4 steps filled in column 60.
        let arr = array_from(note(PitchStep::C, 4, Duration::quarter()), 4);
        let pr = to_piano_roll(&arr, true);
        assert_eq!(pr.shape(), (4, 128));
        for t in 0..4 {
            assert_eq!(pr.cell(t, 60), 64, "step {t} should be on");
        }
        assert_eq!(pr.cell(0, 59), 0);
        assert_eq!(pr.cell(0, 61), 0);
    }

    #[test]
    fn test_chord_columns() {
        let m = Music::Chord {
            pitches: vec![
                (Pitch::new(PitchStep::C, 4), vec![]),
                (Pitch::new(PitchStep::E, 4), vec![]),
                (Pitch::new(PitchStep::G, 4), vec![]),
            ],
            duration: Duration::half(),
            annotations: vec![],
        };
        let arr = array_from(m, 4);
        let pr = to_piano_roll(&arr, true);
        assert_eq!(pr.num_steps, 8);
        for &p in &[60u8, 64, 67] {
            for t in 0..8 {
                assert_eq!(pr.cell(t, p), 64);
            }
        }
    }

    #[test]
    fn test_roundtrip_distinct_pitches_exact() {
        // Adjacent but distinct pitches round-trip exactly.
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::D, 4, Duration::quarter()),
            note(PitchStep::E, 4, Duration::half()),
        ]);
        let arr = array_from(m, 4);
        let pr = to_piano_roll(&arr, true);
        assert_eq!(arr, from_piano_roll(&pr));
    }

    #[test]
    fn test_roundtrip_with_gap_exact() {
        // Two same-pitch notes separated by a rest round-trip exactly.
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            Music::Rest {
                duration: Duration::quarter(),
                is_measure_rest: false,
            },
            note(PitchStep::C, 4, Duration::quarter()),
        ]);
        let arr = array_from(m, 4);
        let pr = to_piano_roll(&arr, true);
        let back = from_piano_roll(&pr);
        assert_eq!(back.notes.len(), 2);
        assert_eq!(arr, back);
    }

    #[test]
    fn test_repeated_same_pitch_merges() {
        // Documented lossy case: two adjacent C4 quarters merge into one half.
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::C, 4, Duration::quarter()),
        ]);
        let arr = array_from(m, 4);
        assert_eq!(arr.notes.len(), 2);
        let back = from_piano_roll(&to_piano_roll(&arr, true));
        assert_eq!(back.notes.len(), 1, "adjacent same-pitch notes merge");
        assert_eq!(back.notes[0].onset, 0);
        assert_eq!(back.notes[0].duration, 8);
    }

    #[test]
    fn test_binary_mode() {
        let arr = array_from(note(PitchStep::C, 4, Duration::quarter()), 4);
        let pr = to_piano_roll(&arr, false);
        assert!(!pr.encode_velocity);
        assert_eq!(pr.cell(0, 60), 1, "binary on-cell is 1");
        // Decode uses the default velocity.
        let back = from_piano_roll(&pr);
        assert_eq!(back.notes.len(), 1);
        assert_eq!(back.notes[0].pitch, 60);
        assert_eq!(back.notes[0].duration, 4);
        assert_eq!(back.notes[0].velocity, DEFAULT_VELOCITY);
    }

    #[test]
    fn test_note_ending_at_last_step_roundtrips() {
        // A note that fills the final step must still close (falling edge at T).
        let arr = array_from(note(PitchStep::C, 4, Duration::whole()), 4);
        let pr = to_piano_roll(&arr, true);
        assert_eq!(pr.num_steps, 16);
        let back = from_piano_roll(&pr);
        assert_eq!(back.notes.len(), 1);
        assert_eq!(back.notes[0].duration, 16);
    }

    #[test]
    fn test_empty_when_no_notes() {
        let arr = NoteArray {
            resolution: 4,
            notes: vec![],
        };
        let pr = to_piano_roll(&arr, true);
        assert_eq!(pr.num_steps, 0);
        assert!(pr.data.is_empty());
        assert_eq!(from_piano_roll(&pr).notes.len(), 0);
    }
}
