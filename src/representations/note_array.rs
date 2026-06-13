//! Note-based representation (EDT1).
//!
//! A piece is encoded as a flat, time-ordered list of note rows
//! `(onset, duration, pitch, velocity)`, where `onset` and `duration` are
//! integer time steps and `resolution` is the number of time steps per
//! quarter note. This mirrors `muspy`'s note representation.
//!
//! `to_note_array` flattens the Music tree to absolute time, unfolding repeats
//! and resolving simultaneity, grace notes and tuplets. `from_note_array`
//! reconstructs a Music tree whose flattening reproduces the same rows exactly
//! (onset/duration/pitch are exact; velocity is exact when it round-trips
//! through the dynamics map, otherwise banded).

use serde::{Deserialize, Serialize};

use crate::adapters::dynamics_velocity::{dynamic_to_velocity, velocity_to_dynamic};
use crate::ir::annotation::Annotation;
use crate::ir::articulation::DynamicMark;
use crate::ir::duration::{Duration, Frac};
use crate::ir::music::{Music, MusicDocument};
use crate::ir::pitch::{Pitch, PitchStep};

/// Default time steps per quarter note. High enough that standard note values
/// (down to 128th notes and simple tuplets) land on integer ticks.
pub const DEFAULT_RESOLUTION: u16 = 480;

/// Default note velocity (MIDI), used when no dynamic is in effect.
pub(crate) const DEFAULT_VELOCITY: u8 = 64;

/// A single note in the note-based representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRow {
    /// Absolute start time in time steps.
    pub onset: u32,
    /// Duration in time steps.
    pub duration: u32,
    /// MIDI pitch number (0–127).
    pub pitch: u8,
    /// MIDI velocity (1–127).
    pub velocity: u8,
}

/// The note-based representation of a piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteArray {
    /// Time steps per quarter note.
    pub resolution: u16,
    /// Note rows, sorted by `(onset, pitch, duration, velocity)`.
    pub notes: Vec<NoteRow>,
}

impl NoteArray {
    /// The total length of the piece in time steps (end of the last note).
    pub fn length(&self) -> u32 {
        self.notes
            .iter()
            .map(|n| n.onset + n.duration)
            .max()
            .unwrap_or(0)
    }
}

/// A note collected during the tree walk, timed in whole-note `Frac` units.
struct FlatNote {
    onset: Frac,
    duration: Frac,
    pitch: i32,
    velocity: u8,
}

/// Convert a whole-note `Frac` time to integer time steps for the given
/// resolution (time steps per quarter note). Rounds to nearest.
fn frac_to_steps(t: Frac, resolution: u16) -> u32 {
    // quarter = 1/4 whole, so steps = t * 4 * resolution.
    let scaled = t * Frac::from_integer(4 * resolution as i64);
    let num = *scaled.numer();
    let den = *scaled.denom();
    // Rounded integer division for non-negative values.
    (((num * 2 + den) / (den * 2)).max(0)) as u32
}

/// Inverse of [`frac_to_steps`].
fn steps_to_frac(steps: u32, resolution: u16) -> Frac {
    Frac::new(steps as i64, 4 * resolution as i64)
}

/// Build a [`Pitch`] from a MIDI number using sharp spelling.
fn pitch_from_midi(midi: u8) -> Pitch {
    // C at octave -1 is MIDI 0; transposing spells the target with sharps.
    Pitch::new(PitchStep::C, -1).transposed(midi as i32)
}

/// Update the running velocity from a note/chord's dynamic annotations.
fn apply_dynamics(vel: &mut u8, annotations: &[Annotation]) {
    for ann in annotations {
        if let Annotation::Dynamic(DynamicMark { sign, .. }) = ann {
            let v = dynamic_to_velocity(sign);
            if v > 0 {
                *vel = v;
            }
        }
    }
}

/// Recursively flatten `music` into absolute-timed notes. `time` is the start
/// position (whole-note `Frac`); returns the end position. `vel` carries the
/// running velocity across the walk.
fn walk(music: &Music, time: Frac, vel: &mut u8, out: &mut Vec<FlatNote>) -> Frac {
    match music {
        Music::Sequential(items) => {
            let mut t = time;
            for m in items {
                t = walk(m, t, vel, out);
            }
            t
        }
        Music::Simultaneous(items) => {
            // Each branch starts at the same time; the block ends at the
            // latest branch end.
            let mut end = time;
            for m in items {
                let e = walk(m, time, vel, out);
                if e > end {
                    end = e;
                }
            }
            end
        }
        Music::Context { content, .. }
        | Music::Variable { content, .. }
        | Music::Tuplet { content, .. } => walk(content, time, vel, out),
        Music::Grace { content, .. } => {
            // Grace notes do not consume time; they sound at `time`.
            walk(content, time, vel, out);
            time
        }
        Music::Note {
            pitch,
            duration,
            annotations,
        } => {
            apply_dynamics(vel, annotations);
            let dur = duration.actual_duration();
            out.push(FlatNote {
                onset: time,
                duration: dur,
                pitch: pitch.midi_number(),
                velocity: *vel,
            });
            time + dur
        }
        Music::Chord {
            pitches,
            duration,
            annotations,
        } => {
            apply_dynamics(vel, annotations);
            let dur = duration.actual_duration();
            for (p, _) in pitches {
                out.push(FlatNote {
                    onset: time,
                    duration: dur,
                    pitch: p.midi_number(),
                    velocity: *vel,
                });
            }
            time + dur
        }
        Music::Rest { duration, .. } | Music::Skip { duration } => {
            time + duration.actual_duration()
        }
        Music::Repeat {
            count,
            body,
            alternatives,
            ..
        } => walk_repeat(*count, body, alternatives, time, vel, out),
        // Attribute events / directions carry no notes and no time.
        _ => time,
    }
}

/// Unfold a repeat into its played sequence. With alternatives, repetition `i`
/// plays the body followed by alternative `min(i, last)`; without, the body is
/// played `count` times.
fn walk_repeat(
    count: u16,
    body: &Music,
    alternatives: &[Music],
    time: Frac,
    vel: &mut u8,
    out: &mut Vec<FlatNote>,
) -> Frac {
    let reps = count.max(1) as usize;
    let mut t = time;
    if alternatives.is_empty() {
        for _ in 0..reps {
            t = walk(body, t, vel, out);
        }
    } else {
        for i in 0..reps {
            t = walk(body, t, vel, out);
            let alt = &alternatives[i.min(alternatives.len() - 1)];
            t = walk(alt, t, vel, out);
        }
    }
    t
}

/// Encode a Music document as a [`NoteArray`] at the given resolution
/// (time steps per quarter note).
pub fn to_note_array(doc: &MusicDocument, resolution: u16) -> NoteArray {
    let mut flat: Vec<FlatNote> = Vec::new();
    let mut vel = DEFAULT_VELOCITY;
    walk(&doc.music, Frac::from_integer(0), &mut vel, &mut flat);

    let mut notes: Vec<NoteRow> = flat
        .iter()
        .map(|f| NoteRow {
            onset: frac_to_steps(f.onset, resolution),
            duration: frac_to_steps(f.duration, resolution),
            pitch: f.pitch.clamp(0, 127) as u8,
            velocity: f.velocity.clamp(1, 127),
        })
        .collect();
    notes.sort_by_key(|n| (n.onset, n.pitch, n.duration, n.velocity));

    NoteArray { resolution, notes }
}

/// Reconstruct a Music document from a [`NoteArray`].
///
/// Each note becomes its own parallel branch `Skip(onset) · Note(duration)`,
/// so `to_note_array` recovers the exact same rows (onset/duration/pitch
/// exactly; velocity via the dynamics band). The output is faithful to the
/// played content, not idiomatic notation.
pub fn from_note_array(arr: &NoteArray) -> MusicDocument {
    let branches: Vec<Music> = arr
        .notes
        .iter()
        .map(|n| {
            let mut seq: Vec<Music> = Vec::new();
            if n.onset > 0 {
                seq.push(Music::Skip {
                    duration: Duration::new(steps_to_frac(n.onset, arr.resolution)),
                });
            }
            let mut annotations = Vec::new();
            if n.velocity != DEFAULT_VELOCITY {
                annotations.push(Annotation::Dynamic(DynamicMark {
                    sign: velocity_to_dynamic(n.velocity).to_string(),
                    placement: Default::default(),
                }));
            }
            seq.push(Music::Note {
                pitch: pitch_from_midi(n.pitch),
                duration: Duration::new(steps_to_frac(n.duration, arr.resolution)),
                annotations,
            });
            Music::Sequential(seq)
        })
        .collect();

    MusicDocument::new(Music::Simultaneous(branches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::music::{ContextType, RepeatType};

    fn note(step: PitchStep, octave: i32, dur: Duration) -> Music {
        Music::Note {
            pitch: Pitch::new(step, octave),
            duration: dur,
            annotations: vec![],
        }
    }

    fn doc(music: Music) -> MusicDocument {
        MusicDocument::new(music)
    }

    #[test]
    fn test_simple_melody_onsets_and_durations() {
        // c'4 d'4 e'4 f'4 in a staff.
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::D, 4, Duration::quarter()),
            note(PitchStep::E, 4, Duration::quarter()),
            note(PitchStep::F, 4, Duration::quarter()),
        ])
        .in_context(ContextType::Staff, None);

        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 4);
        // Quarter note = 480 steps; onsets at 0, 480, 960, 1440.
        let onsets: Vec<u32> = arr.notes.iter().map(|n| n.onset).collect();
        assert_eq!(onsets, vec![0, 480, 960, 1440]);
        assert!(arr.notes.iter().all(|n| n.duration == 480));
        // Pitches C4=60, D4=62, E4=64, F4=65.
        let pitches: Vec<u8> = arr.notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 62, 64, 65]);
        assert_eq!(arr.length(), 1920);
    }

    #[test]
    fn test_rest_and_skip_advance_time() {
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            Music::Rest {
                duration: Duration::quarter(),
                is_measure_rest: false,
            },
            note(PitchStep::E, 4, Duration::quarter()),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 2);
        assert_eq!(arr.notes[0].onset, 0);
        // Second note starts after note + rest = 960.
        assert_eq!(arr.notes[1].onset, 960);
    }

    #[test]
    fn test_chord_notes_share_onset() {
        let m = Music::Chord {
            pitches: vec![
                (Pitch::new(PitchStep::C, 4), vec![]),
                (Pitch::new(PitchStep::E, 4), vec![]),
                (Pitch::new(PitchStep::G, 4), vec![]),
            ],
            duration: Duration::half(),
            annotations: vec![],
        };
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 3);
        assert!(arr.notes.iter().all(|n| n.onset == 0));
        assert!(arr.notes.iter().all(|n| n.duration == 960));
        let pitches: Vec<u8> = arr.notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 64, 67]);
    }

    #[test]
    fn test_simultaneous_voices_overlap() {
        // Two parallel voices: { c'2 } and { e'4 g'4 }.
        let m = Music::Simultaneous(vec![
            Music::Sequential(vec![note(PitchStep::C, 4, Duration::half())]),
            Music::Sequential(vec![
                note(PitchStep::E, 4, Duration::quarter()),
                note(PitchStep::G, 4, Duration::quarter()),
            ]),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 3);
        // C4 (onset 0, dur 960), E4 (onset 0, dur 480), G4 (onset 480, dur 480).
        let rows: Vec<(u32, u32, u8)> = arr
            .notes
            .iter()
            .map(|n| (n.onset, n.duration, n.pitch))
            .collect();
        assert!(rows.contains(&(0, 960, 60)));
        assert!(rows.contains(&(0, 480, 64)));
        assert!(rows.contains(&(480, 480, 67)));
    }

    #[test]
    fn test_tuplet_durations() {
        // \tuplet 3/2 { c'8 d'8 e'8 } — three triplet eighths fill a quarter.
        let mut e = Duration::eighth();
        e.tuplet_actual = 3;
        e.tuplet_normal = 2;
        let m = Music::Tuplet {
            normal: 2,
            actual: 3,
            content: Box::new(Music::Sequential(vec![
                note(PitchStep::C, 4, e.clone()),
                note(PitchStep::D, 4, e.clone()),
                note(PitchStep::E, 4, e),
            ])),
        };
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 3);
        // Each triplet eighth = 480/3 = 160 steps; total = quarter = 480.
        assert!(arr.notes.iter().all(|n| n.duration == 160));
        assert_eq!(arr.length(), 480);
    }

    #[test]
    fn test_grace_note_shares_onset_no_time() {
        // \grace c'8 d'4 — grace sounds at the main note's onset, consumes no time.
        let m = Music::Sequential(vec![
            Music::Grace {
                content: Box::new(note(PitchStep::C, 5, Duration::eighth())),
                slash: true,
            },
            note(PitchStep::D, 4, Duration::quarter()),
            note(PitchStep::E, 4, Duration::quarter()),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 3);
        // Grace C5 (onset 0) and main D4 (onset 0) share onset; E4 at 480.
        let onsets: Vec<u32> = arr.notes.iter().map(|n| n.onset).collect();
        assert_eq!(onsets, vec![0, 0, 480]);
    }

    #[test]
    fn test_dynamics_set_velocity() {
        let mut soft = note(PitchStep::C, 4, Duration::quarter());
        if let Music::Note { annotations, .. } = &mut soft {
            annotations.push(Annotation::Dynamic(DynamicMark {
                sign: "ppp".to_string(),
                placement: Default::default(),
            }));
        }
        let mut loud = note(PitchStep::D, 4, Duration::quarter());
        if let Music::Note { annotations, .. } = &mut loud {
            annotations.push(Annotation::Dynamic(DynamicMark {
                sign: "fff".to_string(),
                placement: Default::default(),
            }));
        }
        // Third note inherits the running (loud) velocity.
        let plain = note(PitchStep::E, 4, Duration::quarter());

        let arr = to_note_array(&doc(Music::Sequential(vec![soft, loud, plain])), 480);
        assert_eq!(arr.notes.len(), 3);
        // Sorted by onset; ppp << fff and the third stays loud.
        assert!(arr.notes[0].velocity < arr.notes[1].velocity);
        assert_eq!(arr.notes[1].velocity, arr.notes[2].velocity);
    }

    #[test]
    fn test_repeat_unfolds() {
        // \repeat volta 3 { c'4 } → three notes at 0, 480, 960.
        let m = Music::Repeat {
            repeat_type: RepeatType::Volta,
            count: 3,
            body: Box::new(Music::Sequential(vec![note(
                PitchStep::C,
                4,
                Duration::quarter(),
            )])),
            alternatives: vec![],
        };
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 3);
        let onsets: Vec<u32> = arr.notes.iter().map(|n| n.onset).collect();
        assert_eq!(onsets, vec![0, 480, 960]);
    }

    #[test]
    fn test_repeat_with_alternatives_unfolds() {
        // \repeat volta 2 { c'4 } \alternative { { d'4 } { e'4 } }
        // → c' d' (rep 1), c' e' (rep 2): C4,D4,C4,E4 at 0,480,960,1440.
        let m = Music::Repeat {
            repeat_type: RepeatType::Volta,
            count: 2,
            body: Box::new(Music::Sequential(vec![note(
                PitchStep::C,
                4,
                Duration::quarter(),
            )])),
            alternatives: vec![
                Music::Sequential(vec![note(PitchStep::D, 4, Duration::quarter())]),
                Music::Sequential(vec![note(PitchStep::E, 4, Duration::quarter())]),
            ],
        };
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 4);
        let pitches: Vec<u8> = arr.notes.iter().map(|n| n.pitch).collect();
        // Sorted by (onset, pitch): (0,60),(480,62),(960,60),(1440,64).
        assert_eq!(pitches, vec![60, 62, 60, 64]);
    }

    #[test]
    fn test_roundtrip_onset_duration_pitch_exact() {
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::E, 4, Duration::half()),
            note(PitchStep::G, 4, Duration::eighth()),
        ]);
        let arr = to_note_array(&doc(m), 480);
        let arr2 = to_note_array(&from_note_array(&arr), 480);
        // Compare onset/duration/pitch (velocity is banded through dynamics).
        let strip = |a: &NoteArray| -> Vec<(u32, u32, u8)> {
            a.notes
                .iter()
                .map(|n| (n.onset, n.duration, n.pitch))
                .collect()
        };
        assert_eq!(strip(&arr), strip(&arr2));
    }

    #[test]
    fn test_roundtrip_default_velocity_exact() {
        // With default velocity (no dynamics), the full array round-trips.
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::D, 4, Duration::quarter()),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert!(arr.notes.iter().all(|n| n.velocity == DEFAULT_VELOCITY));
        let arr2 = to_note_array(&from_note_array(&arr), 480);
        assert_eq!(arr, arr2);
    }
}
