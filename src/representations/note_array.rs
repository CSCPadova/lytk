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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::adapters::dynamics_velocity::{
    lilypond_dynamic, lilypond_velocity, LILYPOND_DEFAULT_VELOCITY,
};
use crate::ir::annotation::Annotation;
use crate::ir::articulation::DynamicMark;
use crate::ir::duration::{Duration, Frac};
use crate::ir::music::{Music, MusicDocument};
use crate::ir::pitch::{Pitch, PitchStep};

/// Default time steps per quarter note. High enough that standard note values
/// (down to 128th notes and simple tuplets) land on integer ticks.
pub const DEFAULT_RESOLUTION: u16 = 480;

/// Default note velocity (MIDI), used when no dynamic is in effect: what
/// lytk's MIDI export plays (LilyPond's default volume).
pub(crate) const DEFAULT_VELOCITY: u8 = LILYPOND_DEFAULT_VELOCITY;

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
    ///
    /// Uses saturating addition so adversarial `onset`/`duration` values from a
    /// user-supplied note array (`from_note_array`) can't overflow `u32`.
    pub fn length(&self) -> u32 {
        self.notes
            .iter()
            .map(|n| n.onset.saturating_add(n.duration))
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
    // quarter = 1/4 whole, so steps = t * 4 * resolution, computed in i128 so
    // no numerator or denominator the IR holds can overflow it.
    let num = i128::from(*t.numer()) * 4 * i128::from(resolution);
    let den = i128::from(*t.denom());
    // Rounded integer division for non-negative values, saturating at u32.
    ((num * 2 + den) / (den * 2)).clamp(0, i128::from(u32::MAX)) as u32
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

/// What the walk carries along a voice: the velocity of the dynamic in
/// force (LilyPond's table, as the MIDI export plays it) and the
/// transposition (semitones from written to sounding pitch).
#[derive(Clone, Copy)]
struct Perf {
    vel: u8,
    transpose: i32,
    /// Whether transpositions apply (sounding pitch) or not (written).
    sounding: bool,
}

/// Update the running velocity from a note/chord's dynamic annotations.
fn apply_dynamics(vel: &mut u8, annotations: &[Annotation]) {
    for ann in annotations {
        if let Annotation::Dynamic(DynamicMark { sign, .. }) = ann {
            *vel = lilypond_velocity(sign);
        }
    }
}

/// Emit a note row, collapsing tie chains into a single sounding note.
///
/// Tie conventions differ across import paths — the lift path (MusicXML/MIDI)
/// marks the first segment `TieStart` and the continuation `TieStop`, while ABC
/// (and the LilyPond `~`) mark only `TieStart` on the first segment. So fusion
/// is *forward-looking*: a pitch with an open tie is extended by the next
/// same-pitch note regardless of whether that note carries `TieStop`. `open`
/// maps a MIDI pitch to the index (in `out`) of the note currently absorbing
/// its tie continuation, and is scoped per simultaneous voice so ties never
/// cross voices. Mirrors the chain-collapsing rule in `ir_to_midi`'s
/// `tie_flags`.
fn emit_or_fuse(
    pitch: i32,
    dur: Frac,
    velocity: u8,
    time: Frac,
    has_start: bool,
    open: &mut HashMap<i32, usize>,
    out: &mut Vec<FlatNote>,
) {
    // Continuation of an open tie — only where the held note ends: extend it,
    // don't re-articulate.
    let held = open
        .get(&pitch)
        .copied()
        .filter(|&idx| out[idx].onset + out[idx].duration == time);
    if let Some(idx) = held {
        out[idx].duration += dur;
        if !has_start {
            open.remove(&pitch);
        }
    } else {
        let idx = out.len();
        out.push(FlatNote {
            onset: time,
            duration: dur,
            pitch,
            velocity,
        });
        // A new note of the pitch ends whatever tie was left dangling.
        if has_start {
            open.insert(pitch, idx);
        } else {
            open.remove(&pitch);
        }
    }
}

/// The velocity a note was played with, when it carries one (MIDI import,
/// MusicXML `<note dynamics>`); it wins over the running dynamic.
fn own_velocity(annotations: &[Annotation]) -> Option<u8> {
    annotations.iter().find_map(|a| match a {
        Annotation::Velocity(v) => Some(*v),
        _ => None,
    })
}

/// Whether an annotation list opens a tie.
fn has_tie_start(annotations: &[Annotation]) -> bool {
    annotations.contains(&Annotation::TieStart)
}

/// Recursively flatten `music` into absolute-timed notes. `time` is the start
/// position (whole-note `Frac`); returns the end position. `perf` carries the
/// dynamic and the transposition in force along the walk.
fn walk(
    music: &Music,
    time: Frac,
    perf: &mut Perf,
    open: &mut HashMap<i32, usize>,
    out: &mut Vec<FlatNote>,
) -> Frac {
    match music {
        Music::Sequential(items) => {
            let mut t = time;
            for m in items {
                t = walk(m, t, perf, open, out);
            }
            t
        }
        Music::Simultaneous(items) => {
            // Each branch starts at the same time; the block ends at the
            // latest branch end. A tie from before may end in any branch (a
            // bar that splits into voices), and one may run on after it: the
            // ties the branches close and open are the block's.
            // Each branch starts with the block's dynamic and transposition
            // (a part's dynamics don't reach the next part); the first
            // branch, the voice that goes on, carries its own on after it.
            let mut end = time;
            let mut closed: Vec<i32> = Vec::new();
            let mut opened: HashMap<i32, usize> = HashMap::new();
            let entry = *perf;
            let mut after: Option<Perf> = None;
            for m in items {
                let mut branch = entry;
                let mut branch_open = open.clone();
                let held: Vec<(i32, usize, Frac)> = open
                    .iter()
                    .map(|(p, &i)| (*p, i, out[i].duration))
                    .collect();
                let e = walk(m, time, &mut branch, &mut branch_open, out);
                after.get_or_insert(branch);
                // A tie this branch carried on and ended (another voice's note
                // of the same pitch doesn't end it).
                closed.extend(
                    held.iter()
                        .filter(|(p, i, d)| out[*i].duration > *d && branch_open.get(p) != Some(i))
                        .map(|(p, ..)| *p),
                );
                opened.extend(
                    branch_open
                        .into_iter()
                        .filter(|(p, idx)| open.get(p) != Some(idx)),
                );
                if e > end {
                    end = e;
                }
            }
            for p in closed {
                open.remove(&p);
            }
            open.extend(opened);
            if let Some(a) = after {
                *perf = a;
            }
            end
        }
        Music::Context { content, .. }
        | Music::Variable { content, .. }
        | Music::Tuplet { content, .. } => walk(content, time, perf, open, out),
        Music::Grace { content, .. } => {
            // Grace notes do not consume time; they sound at `time`.
            walk(content, time, perf, open, out);
            time
        }
        Music::Note {
            pitch,
            duration,
            annotations,
        } => {
            apply_dynamics(&mut perf.vel, annotations);
            let dur = duration.actual_duration();
            emit_or_fuse(
                pitch.midi_number() + perf.transpose,
                dur,
                own_velocity(annotations).unwrap_or(perf.vel),
                time,
                has_tie_start(annotations),
                open,
                out,
            );
            time + dur
        }
        Music::Chord {
            pitches,
            duration,
            annotations,
        } => {
            apply_dynamics(&mut perf.vel, annotations);
            let dur = duration.actual_duration();
            // A chord-level tie ties every pitch; a pitch may also carry its own.
            let chord_tie = has_tie_start(annotations);
            for (p, pitch_anns) in pitches {
                emit_or_fuse(
                    p.midi_number() + perf.transpose,
                    dur,
                    own_velocity(pitch_anns)
                        .or(own_velocity(annotations))
                        .unwrap_or(perf.vel),
                    time,
                    chord_tie || has_tie_start(pitch_anns),
                    open,
                    out,
                );
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
        } => walk_repeat(*count, body, alternatives, time, perf, open, out),
        // A dynamic between notes (a MusicXML direction) sets the level.
        Music::Direction(d) => {
            if let Some(dm) = &d.dynamic {
                perf.vel = lilypond_velocity(&dm.sign);
            }
            time
        }
        Music::Transposition(t) => {
            if perf.sounding {
                perf.transpose = t.semitones();
            }
            time
        }
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
    perf: &mut Perf,
    open: &mut HashMap<i32, usize>,
    out: &mut Vec<FlatNote>,
) -> Frac {
    let reps = count.max(1) as usize;
    let mut t = time;
    if alternatives.is_empty() {
        for _ in 0..reps {
            t = walk(body, t, perf, open, out);
        }
    } else {
        for i in 0..reps {
            t = walk(body, t, perf, open, out);
            let alt = &alternatives[i.min(alternatives.len() - 1)];
            t = walk(alt, t, perf, open, out);
        }
    }
    t
}

/// Encode a Music document as a [`NoteArray`] at the given resolution
/// (time steps per quarter note). Pitches are sounding pitches: a B♭
/// clarinet's written D is a C.
pub fn to_note_array(doc: &MusicDocument, resolution: u16) -> NoteArray {
    to_note_array_pitched(doc, resolution, true)
}

/// [`to_note_array`], with written pitches when `sounding` is false.
pub fn to_note_array_pitched(doc: &MusicDocument, resolution: u16, sounding: bool) -> NoteArray {
    let mut flat: Vec<FlatNote> = Vec::new();
    let mut perf = Perf {
        vel: DEFAULT_VELOCITY,
        transpose: 0,
        sounding,
    };
    let mut open: HashMap<i32, usize> = HashMap::new();
    walk(
        &doc.music,
        Frac::from_integer(0),
        &mut perf,
        &mut open,
        &mut flat,
    );

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
/// so `to_note_array` recovers the exact same rows. Each velocity is kept
/// exactly (an [`Annotation::Velocity`]); a note away from the default also
/// gets the nearest dynamic mark, for notation. The output is faithful to the
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
                    sign: lilypond_dynamic(n.velocity),
                    placement: Default::default(),
                }));
                annotations.push(Annotation::Velocity(n.velocity));
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
    use crate::ir::articulation::DynamicType;
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
                sign: DynamicType::Ppp,
                placement: Default::default(),
            }));
        }
        let mut loud = note(PitchStep::D, 4, Duration::quarter());
        if let Music::Note { annotations, .. } = &mut loud {
            annotations.push(Annotation::Dynamic(DynamicMark {
                sign: DynamicType::Fff,
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

    /// Build a note carrying tie annotations (lift/ABC style).
    fn tied(step: PitchStep, octave: i32, dur: Duration, anns: Vec<Annotation>) -> Music {
        Music::Note {
            pitch: Pitch::new(step, octave),
            duration: dur,
            annotations: anns,
        }
    }

    #[test]
    fn a_tie_into_a_multi_voice_bar_closes_there() {
        // e'4~ | << { e'4 } \\ { r4 } >> | e'4: the tie ends in the second
        // bar; the third e' is a note of its own.
        let q = || Duration::new(Frac::new(1, 4));
        let music = Music::Sequential(vec![
            tied(PitchStep::E, 4, q(), vec![Annotation::TieStart]),
            Music::Simultaneous(vec![
                Music::Sequential(vec![tied(PitchStep::E, 4, q(), vec![Annotation::TieStop])]),
                Music::Sequential(vec![Music::Rest {
                    duration: q(),
                    is_measure_rest: false,
                }]),
            ]),
            note(PitchStep::E, 4, q()),
        ]);
        let arr = to_note_array(&doc(music), 480);
        let got: Vec<(u32, u32)> = arr.notes.iter().map(|n| (n.onset, n.duration)).collect();
        assert_eq!(got, vec![(0, 960), (960, 480)]);
    }

    #[test]
    fn test_tie_collapses_to_single_note() {
        // c'2~ c'4 — a half tied to a quarter is ONE sounding note (1440 steps),
        // not two re-articulations. (lift convention: TieStart then TieStop.)
        let m = Music::Sequential(vec![
            tied(
                PitchStep::C,
                4,
                Duration::half(),
                vec![Annotation::TieStart],
            ),
            tied(
                PitchStep::C,
                4,
                Duration::quarter(),
                vec![Annotation::TieStop],
            ),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 1, "tied notes must collapse to one row");
        assert_eq!(arr.notes[0].onset, 0);
        assert_eq!(arr.notes[0].duration, 960 + 480);
        assert_eq!(arr.notes[0].pitch, 60);
    }

    #[test]
    fn test_tie_chain_of_three_collapses() {
        // c'4~ c'4~ c'4 — three-segment chain → one note of 1440 steps.
        // (middle segment carries both TieStop and TieStart, as the lift path emits.)
        let m = Music::Sequential(vec![
            tied(
                PitchStep::C,
                4,
                Duration::quarter(),
                vec![Annotation::TieStart],
            ),
            tied(
                PitchStep::C,
                4,
                Duration::quarter(),
                vec![Annotation::TieStop, Annotation::TieStart],
            ),
            tied(
                PitchStep::C,
                4,
                Duration::quarter(),
                vec![Annotation::TieStop],
            ),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 1);
        assert_eq!(arr.notes[0].duration, 1440);
    }

    #[test]
    fn test_tie_forward_only_collapses_abc_style() {
        // ABC attaches only a TieStart to the first note; the continuation has
        // no TieStop. Forward-looking fusion must still collapse them.
        let m = Music::Sequential(vec![
            tied(
                PitchStep::G,
                4,
                Duration::quarter(),
                vec![Annotation::TieStart],
            ),
            note(PitchStep::G, 4, Duration::quarter()),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 1);
        assert_eq!(arr.notes[0].duration, 960);
    }

    #[test]
    fn test_untied_repeated_pitch_stays_separate() {
        // Regression: two un-tied same-pitch quarters must remain TWO notes
        // (fusion must not over-collapse repeated pitches).
        let m = Music::Sequential(vec![
            note(PitchStep::C, 4, Duration::quarter()),
            note(PitchStep::C, 4, Duration::quarter()),
        ]);
        let arr = to_note_array(&doc(m), 480);
        assert_eq!(arr.notes.len(), 2);
        assert_eq!(arr.notes[0].onset, 0);
        assert_eq!(arr.notes[1].onset, 480);
    }

    #[test]
    fn test_tie_does_not_cross_simultaneous_voices() {
        // An open tie in one voice must not fuse a same-pitch note in a parallel
        // voice. Voice 1: c'4~ c'4 (→ one 960 note). Voice 2: c'2 (independent).
        let m = Music::Simultaneous(vec![
            Music::Sequential(vec![
                tied(
                    PitchStep::C,
                    4,
                    Duration::quarter(),
                    vec![Annotation::TieStart],
                ),
                tied(
                    PitchStep::C,
                    4,
                    Duration::quarter(),
                    vec![Annotation::TieStop],
                ),
            ]),
            Music::Sequential(vec![note(PitchStep::C, 4, Duration::half())]),
        ]);
        let arr = to_note_array(&doc(m), 480);
        // Voice 1 collapses to one 960 note; voice 2 is one 960 note → 2 total.
        assert_eq!(arr.notes.len(), 2);
        assert!(arr.notes.iter().all(|n| n.pitch == 60 && n.duration == 960));
    }
}
