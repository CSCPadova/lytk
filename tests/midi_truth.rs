//! MIDI against the truth, not against lytk itself.
//!
//! The round-trip boards in `fidelity.rs` compare lytk's MIDI reader with
//! lytk's MIDI writer, so a mistake the two share cancels out. These boards
//! compare with independent sources:
//!
//! - **source truth**: `from_midi(x.midi)` against `from_lilypond(x.ly)` for
//!   the MIDI files LilyPond rendered from the `ly/` fixtures;
//! - **LilyPond parity**: our `ly → MIDI` against LilyPond's own MIDI for the
//!   same file, note by note;
//! - **synthetic cases**: small files built here, each with the notation it
//!   should read as;
//! - **corpus**: the MusicXML fixtures written as MIDI and read back;
//! - **notation quality** of what the reader produces.
//!
//! Every count is gated on a committed baseline that may only rise.
//! Run with output: `cargo test --test midi_truth -- --nocapture`

mod common;

use common::smf::{self, Ev, Track};
use common::{bar_lengths, common_count, equal_bar_counts, note_signature, safe, steps};

use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::ToIrAdapter;
use _core::ir::duration::Frac;
use _core::ir::note::VoiceElement;
use _core::ir::score::Score;

use std::path::Path;

// ---------------------------------------------------------------------------
// Committed baselines (may only rise; the "≤" ones may only fall)
// ---------------------------------------------------------------------------

/// Source truth, summed over the pairs: source notes found in the import by
/// (onset, pitch), by (onset, duration, pitch); time and key signatures found
/// at their positions; pairs with the source's bar structure.
const TRUTH_ONSET_PITCH: usize = 7938;
const TRUTH_FULL: usize = 7464;
const TRUTH_TIME_SIGS: usize = 18;
const TRUTH_KEY_SIGS: usize = 7;
const TRUTH_BARS: usize = 1;
/// LilyPond parity, summed over the pairs: LilyPond's notes matched by ours
/// on (on, pitch), (on, off, pitch), (on, off, pitch, velocity).
const PARITY_ON: usize = 7998;
const PARITY_ON_OFF: usize = 7995;
const PARITY_FULL: usize = 4319;
/// Synthetic cases read exactly as written (notes; bar structure).
const SYNTH_NOTES: usize = 51;
const SYNTH_BARS: usize = 51;
/// MusicXML corpus → MIDI → IR: note signature and bar structure kept.
///
/// The one planned drop (121 → 113, epic I phase B): the writer now plays as
/// LilyPond does — grace notes before the beat, staccato shortened, repeats
/// played out, transposing instruments at sounding pitch — and today's reader
/// can't undo that yet (phase C: grace and staccato detection). Nine fixtures
/// were fixed on the way (pickups and incomplete bars: 21e, 46c–g, 02e, 33i,
/// 43d). 45b now plays its first ending correctly, which the lift's note
/// signature doesn't (no forward repeat barline; phase L).
const CORPUS_NOTES: usize = 121;
const CORPUS_BARS: usize = 120;
/// Notation quality over every import above (the fixture MIDIs and the
/// synthetic cases): these may only fall … (Epic I phase C: voices are
/// counted per staff, now that a keyboard's two tracks are one two-staff
/// part.) Six performed cases joined in phase D (2026-09-27): with them the
/// count was 2485 before played triplets were read beat by beat and drifting
/// beats tracked, 2470 after (with the hand-split cost model); the swing
/// cases add rests that are written (off-beats after rests: 1, and 8 for the
/// comping guitar), and so does "a 16th rest first" (1), and the strummed
/// swing chords (8: rests on the beats under the bass). 2518 after the phase
/// D review: the rhythm cases write rests, and a gap of exactly a third of a
/// note (a dotted quarter and an eighth rest) is a rest again — the fixture
/// MIDIs' durations rose with it (TRUTH_FULL 7392 → 7404); then 2478 once
/// runs of LilyPond staccatos were found (TRUTH_FULL 7464).
const QUALITY_MAX_RESTS: usize = 2478;
const QUALITY_MAX_ODD_DURATIONS: usize = 0;
const QUALITY_MAX_VOICES: usize = 4;
/// … and this may only rise: imports whose parts all have the same bar count.
const QUALITY_EQUAL_BARS: usize = 56;

/// (LilyPond source, movement, LilyPond-rendered MIDI).
const PAIRS: [(&str, usize, &str); 5] = [
    ("chopin_n.ly", 0, "chopin_n.midi"),
    ("example.ly", 0, "example.midi"),
    ("example2.ly", 0, "example2_0.midi"),
    ("example2.ly", 1, "example2_1.midi"),
    ("pedal.ly", 0, "pedal.midi"),
];

fn ly_movement(file: &str, movement: usize) -> Score {
    let path = Path::new("tests/fixtures/ly").join(file);
    let mut scores = LyToIrAdapter::new()
        .convert_file_multi(&path)
        .expect("LilyPond fixture parses");
    scores.swap_remove(movement)
}

fn read_midi(bytes: &[u8]) -> Option<Score> {
    safe(|| MidiToIrAdapter::new().convert_bytes(bytes).ok())
}

/// (position in whole notes, label) of every time or key signature, taken
/// from the part with the most of them.
fn signatures_at(score: &Score, key: bool) -> Vec<(u32, String)> {
    let lens = bar_lengths(score);
    let mut best: Vec<(u32, String)> = Vec::new();
    for part in score.parts() {
        let mut pos = Frac::from_integer(0);
        let mut found = Vec::new();
        for (i, m) in part.measures.iter().enumerate() {
            if let Some(a) = &m.attributes {
                let label = if key {
                    a.key.as_ref().map(|k| format!("{} {:?}", k.fifths, k.mode))
                } else {
                    a.time
                        .as_ref()
                        .map(|t| format!("{}/{}", t.beats, t.beat_type))
                };
                if let Some(l) = label {
                    found.push((steps(pos), l));
                }
            }
            pos += lens
                .get(i)
                .copied()
                .unwrap_or_else(|| Frac::from_integer(0));
        }
        // Repeated identical signatures (one per bar) count once.
        found.dedup_by(|b, a| a.1 == b.1);
        if found.len() > best.len() {
            best = found;
        }
    }
    best
}

#[derive(Default)]
struct Quality {
    imports: usize,
    rests: usize,
    odd_durations: usize,
    max_voices: usize,
    equal_bars: usize,
}

impl Quality {
    fn add(&mut self, score: &Score) {
        self.imports += 1;
        if equal_bar_counts(score) {
            self.equal_bars += 1;
        }
        for part in score.parts() {
            for m in &part.measures {
                // Voices a staff has in the bar (a grand staff has two staves).
                let mut per_staff = std::collections::BTreeMap::<u8, usize>::new();
                for v in &m.voices {
                    let staff = v
                        .elements
                        .iter()
                        .map(|e| match e {
                            VoiceElement::Note(n) => n.staff,
                            VoiceElement::Rest(r) => r.staff,
                            VoiceElement::Chord(c) => c.staff,
                        })
                        .find(|s| *s > 0)
                        .unwrap_or(1);
                    *per_staff.entry(staff).or_default() += 1;
                }
                let most = per_staff.values().copied().max().unwrap_or(0);
                self.max_voices = self.max_voices.max(most);
                for v in &m.voices {
                    for e in &v.elements {
                        let d = match e {
                            VoiceElement::Rest(r) => {
                                if !r.is_spacer {
                                    self.rests += 1;
                                }
                                &r.duration
                            }
                            VoiceElement::Note(n) => &n.duration,
                            VoiceElement::Chord(c) => &c.duration,
                        };
                        if d.musicxml_type().is_none() || d.dots > 1 {
                            self.odd_durations += 1;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Synthetic cases
// ---------------------------------------------------------------------------

/// A synthetic file and the (onset, duration, pitch) notes and bar lengths
/// (in whole notes) it should read as. Ticks are 480 per quarter, the same
/// scale as note signatures.
struct Case {
    name: &'static str,
    bytes: Vec<u8>,
    notes: Vec<(u32, u32, i32)>,
    bars: Vec<Frac>,
}

fn q(n: i64, d: i64) -> Frac {
    Frac::new(n, d)
}

fn bars(spec: &[(usize, Frac)]) -> Vec<Frac> {
    spec.iter()
        .flat_map(|(n, len)| std::iter::repeat_n(*len, *n))
        .collect()
}

/// Deterministic jitter in [-amp, amp] (a tiny LCG; no dependency).
fn jitter(seed: &mut u32, amp: i32) -> i32 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    ((*seed >> 16) % (2 * amp as u32 + 1)) as i32 - amp
}

fn cases() -> Vec<Case> {
    let four = [(0, Ev::Time(4, 4))];
    let scale = [60u8, 62, 64, 65, 67, 69, 71, 72];
    let mut out = Vec::new();

    // Two bars of quarters, each note held slightly short (98 % gate).
    let legato: Vec<_> = (0..8u32)
        .map(|i| (i * 480, i * 480 + 470, scale[i as usize], 90))
        .collect();
    let quarters: Vec<_> = (0..8u32)
        .map(|i| (i * 480, 480, scale[i as usize] as i32))
        .collect();
    out.push(Case {
        name: "legato quarters",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &legato)]),
        notes: quarters.clone(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // The same quarters played at half length. Timing alone can't tell that
    // from eighths and eighth rests, so that is how it reads (a staccato is
    // recognised by how LilyPond and lytk play it: 4 louder).
    let detached: Vec<_> = (0..8u32)
        .map(|i| (i * 480, i * 480 + 240, scale[i as usize], 90))
        .collect();
    let eighths: Vec<_> = (0..8u32)
        .map(|i| (i * 480, 240, scale[i as usize] as i32))
        .collect();
    out.push(Case {
        name: "detached quarters",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &detached)]),
        notes: eighths,
        bars: bars(&[(2, q(1, 1))]),
    });
    // Staccato quarters as LilyPond plays them: half length, 4 louder.
    let staccato: Vec<_> = (0..8u32)
        .map(|i| {
            (
                i * 480,
                i * 480 + 240,
                scale[i as usize],
                if i % 2 == 0 { 94 } else { 90 },
            )
        })
        .collect();
    out.push(Case {
        name: "staccato quarters",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &staccato)]),
        notes: (0..8u32)
            .map(|i| {
                (
                    i * 480,
                    if i % 2 == 0 { 480 } else { 240 },
                    scale[i as usize] as i32,
                )
            })
            .collect(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // Two groups of eighth-note triplets, then a half note.
    let mut trip: Vec<_> = (0..6u32)
        .map(|i| (i * 160, i * 160 + 160, scale[i as usize], 90))
        .collect();
    trip.push((960, 1920, 72, 90));
    let mut trip_notes: Vec<_> = (0..6u32)
        .map(|i| (i * 160, 160, scale[i as usize] as i32))
        .collect();
    trip_notes.push((960, 960, 72));
    out.push(Case {
        name: "eighth triplets",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &trip)]),
        notes: trip_notes,
        bars: bars(&[(1, q(1, 1))]),
    });

    // A quintuplet of sixteenths on beat 1, then a dotted half.
    let mut quint: Vec<_> = (0..5u32)
        .map(|i| (i * 96, i * 96 + 96, scale[i as usize], 90))
        .collect();
    quint.push((480, 1920, 72, 90));
    let mut quint_notes: Vec<_> = (0..5u32)
        .map(|i| (i * 96, 96, scale[i as usize] as i32))
        .collect();
    quint_notes.push((480, 1440, 72));
    out.push(Case {
        name: "quintuplet",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &quint)]),
        notes: quint_notes,
        bars: bars(&[(1, q(1, 1))]),
    });

    // A septuplet of sixteenths on beat 1 (ticks rounded, as a sequencer
    // writes them), then a dotted half.
    let sept_on = |i: u32| (i * 480 + 3) / 7;
    let mut sept: Vec<_> = (0..7u32)
        .map(|i| (sept_on(i), sept_on(i + 1), scale[i as usize], 90))
        .collect();
    sept.push((480, 1920, 72, 90));
    let mut sept_notes: Vec<_> = (0..7i64)
        .map(|i| (steps(q(i, 28)), steps(q(1, 28)), scale[i as usize] as i32))
        .collect();
    sept_notes.push((480, 1440, 72));
    out.push(Case {
        name: "septuplet",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &sept)]),
        notes: sept_notes,
        bars: bars(&[(1, q(1, 1))]),
    });

    // A one-beat pickup written as a 1/4 bar before the 4/4 (how MuseScore,
    // and lytk from now on, write a pickup).
    let pickup_conductor = [(0, Ev::Time(1, 4)), (480, Ev::Time(4, 4))];
    let mut pick = vec![(0, 480, 67, 90)];
    pick.extend((0..4u32).map(|i| (480 + i * 480, 960 + i * 480, scale[i as usize], 90)));
    pick.push((2400, 4320, 67, 90));
    let mut pick_notes = vec![(0, 480, 67)];
    pick_notes.extend((0..4u32).map(|i| (480 + i * 480, 480, scale[i as usize] as i32)));
    pick_notes.push((2400, 1920, 67));
    out.push(Case {
        name: "pickup",
        bytes: smf::build(480, &pickup_conductor, &[Track::new("p", 0, &pick)]),
        notes: pick_notes,
        bars: vec![q(1, 4), q(1, 1), q(1, 1)],
    });

    // A half note across the bar line (tied in notation), after a dotted half.
    let tie = [
        (0, 1440, 60, 90),
        (1440, 2400, 62, 90),
        (2400, 3840, 64, 90),
    ];
    out.push(Case {
        name: "note across the bar line",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &tie)]),
        notes: vec![(0, 1440, 60), (1440, 960, 62), (2400, 1440, 64)],
        bars: bars(&[(2, q(1, 1))]),
    });

    // Two bars of 3/4, then two of 2/4.
    let meter_conductor = [(0, Ev::Time(3, 4)), (2880, Ev::Time(2, 4))];
    let meter: Vec<_> = (0..10u32)
        .map(|i| (i * 480, i * 480 + 480, scale[(i % 8) as usize], 90))
        .collect();
    let meter_notes: Vec<_> = (0..10u32)
        .map(|i| (i * 480, 480, scale[(i % 8) as usize] as i32))
        .collect();
    out.push(Case {
        name: "meter change",
        bytes: smf::build(480, &meter_conductor, &[Track::new("p", 0, &meter)]),
        notes: meter_notes,
        bars: bars(&[(2, q(3, 4)), (2, q(1, 2))]),
    });

    // Dotted quarter + eighth, then a half.
    let dotted = [(0, 720, 60, 90), (720, 960, 62, 90), (960, 1920, 64, 90)];
    out.push(Case {
        name: "dotted quarter",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &dotted)]),
        notes: vec![(0, 720, 60), (720, 240, 62), (960, 960, 64)],
        bars: bars(&[(1, q(1, 1))]),
    });

    // Chord notes struck a few ticks apart (a played chord).
    let spread = [
        (0, 470, 60, 90),
        (6, 472, 64, 90),
        (11, 475, 67, 90),
        (480, 1910, 65, 90),
    ];
    out.push(Case {
        name: "spread chord",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &spread)]),
        notes: vec![(0, 480, 60), (0, 480, 64), (0, 480, 67), (480, 1440, 65)],
        bars: bars(&[(1, q(1, 1))]),
    });

    // Two voices on one track: a held whole note under moving quarters.
    let two = [
        (0, 1920, 48, 90),
        (0, 480, 64, 90),
        (480, 960, 67, 90),
        (960, 1440, 64, 90),
        (1440, 1920, 67, 90),
    ];
    out.push(Case {
        name: "two voices, one track",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &two)]),
        notes: vec![
            (0, 1920, 48),
            (0, 480, 64),
            (480, 480, 67),
            (960, 480, 64),
            (1440, 480, 67),
        ],
        bars: bars(&[(1, q(1, 1))]),
    });

    // A piano as two tracks on one channel (right and left hand).
    let rh: Vec<_> = (0..4u32)
        .map(|i| (i * 480, i * 480 + 480, scale[i as usize + 4], 90))
        .collect();
    let lh = [(0, 1920, 48, 90)];
    let mut piano_notes: Vec<_> = (0..4u32)
        .map(|i| (i * 480, 480, scale[i as usize + 4] as i32))
        .collect();
    piano_notes.push((0, 1920, 48));
    out.push(Case {
        name: "piano, two tracks",
        bytes: smf::build(
            480,
            &four,
            &[
                Track::new("Piano RH", 0, &rh),
                Track::new("Piano LH", 0, &lh),
            ],
        ),
        notes: piano_notes,
        bars: bars(&[(1, q(1, 1))]),
    });

    // Drums on channel 10: kick, snare and hi-hat.
    let drums = [
        (0, 120, 36, 100),
        (0, 120, 42, 80),
        (480, 600, 38, 100),
        (480, 600, 42, 80),
        (960, 1080, 36, 100),
        (960, 1080, 42, 80),
        (1440, 1560, 38, 100),
        (1440, 1560, 42, 80),
    ];
    out.push(Case {
        name: "drums",
        bytes: smf::build(480, &four, &[Track::new("Drums", 9, &drums)]),
        notes: drums
            .iter()
            .map(|&(on, _, p, _)| (on, 480, p as i32))
            .collect(),
        bars: bars(&[(1, q(1, 1))]),
    });

    // Humanised quarters: onsets and ends moved by up to ±10 and ±20 ticks.
    for amp in [10, 20] {
        let mut seed = 7u32;
        let played: Vec<_> = (0..8u32)
            .map(|i| {
                let on = (i as i32 * 480 + if i == 0 { 0 } else { jitter(&mut seed, amp) }) as u32;
                let off = (i as i32 * 480 + 450 + jitter(&mut seed, amp)) as u32;
                (on, off, scale[i as usize], 90)
            })
            .collect();
        out.push(Case {
            name: if amp == 10 {
                "humanised ±10 ticks"
            } else {
                "humanised ±20 ticks"
            },
            bytes: smf::build(480, &four, &[Track::new("p", 0, &played)]),
            notes: quarters.clone(),
            bars: bars(&[(2, q(1, 1))]),
        });
    }

    // Performed (phase D). Eighths played legato — each held into the next by
    // up to 30 ticks — with onsets ±15 ticks and uneven velocities.
    let mut seed = 11u32;
    let legato: Vec<_> = (0..16u32)
        .map(|i| {
            let on = (i as i32 * 240 + if i == 0 { 0 } else { jitter(&mut seed, 15) }) as u32;
            let off = (i as i32 * 240 + 255 + jitter(&mut seed, 15)) as u32;
            let vel = (80 + jitter(&mut seed, 12)) as u8;
            (on, off, scale[(i % 8) as usize], vel)
        })
        .collect();
    out.push(Case {
        name: "performed legato eighths",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &legato)]),
        notes: (0..16u32)
            .map(|i| (i * 240, 240, scale[(i % 8) as usize] as i32))
            .collect(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // Eighth triplets played ±10 ticks.
    let mut seed = 5u32;
    let trips: Vec<_> = (0..12u32)
        .map(|i| {
            let on = (i as i32 * 160 + if i == 0 { 0 } else { jitter(&mut seed, 10) }) as u32;
            let off = (i as i32 * 160 + 150 + jitter(&mut seed, 10)) as u32;
            (on, off, scale[(i % 8) as usize], 85)
        })
        .collect();
    out.push(Case {
        name: "performed triplets",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &trips)]),
        notes: (0..12u32)
            .map(|i| (i * 160, 160, scale[(i % 8) as usize] as i32))
            .collect(),
        bars: bars(&[(1, q(1, 1))]),
    });

    // A piano played on one track: right-hand quarters over left-hand halves.
    let mut seed = 3u32;
    let mut both: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut both_notes = Vec::new();
    for i in 0..8u32 {
        let on = (i as i32 * 480 + if i == 0 { 0 } else { jitter(&mut seed, 8) }) as u32;
        let key = [72u8, 74, 76, 77, 79, 77, 76, 74][i as usize];
        both.push((on, on + 440, key, 80));
        both_notes.push((i * 480, 480, key as i32));
    }
    for i in 0..4u32 {
        let on = (i as i32 * 960 + if i == 0 { 0 } else { jitter(&mut seed, 8) }) as u32;
        let key = [48u8, 43, 45, 41][i as usize];
        both.push((on, on + 900, key, 70));
        both_notes.push((i * 960, 960, key as i32));
    }
    out.push(Case {
        name: "piano, one track",
        bytes: smf::build(480, &four, &[Track::new("Piano", 0, &both)]),
        notes: both_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Sixteenths played ±20 ticks (a sixth of their length), 90 % gate,
    // velocities 80 ± 10.
    let mut seed = 17u32;
    let sixteenths: Vec<_> = (0..32u32)
        .map(|i| {
            let on = (i as i32 * 120 + if i == 0 { 0 } else { jitter(&mut seed, 20) }) as u32;
            let off = (i as i32 * 120 + 108 + jitter(&mut seed, 20)) as u32;
            let vel = (80 + jitter(&mut seed, 10)) as u8;
            (on, off, scale[(i % 8) as usize], vel)
        })
        .collect();
    out.push(Case {
        name: "performed 16ths ±20",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &sixteenths)]),
        notes: (0..32u32)
            .map(|i| (i * 120, 120, scale[(i % 8) as usize] as i32))
            .collect(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // Quarters and eighths played ±20 ticks, each held 20-60 ticks into the
    // next (legato overlaps).
    let mut seed = 23u32;
    let rhythm = [480u32, 240, 240, 480, 480, 240, 240, 240, 240, 960];
    let starts: Vec<u32> = rhythm
        .iter()
        .scan(0, |t, d| {
            let s = *t;
            *t += d;
            Some(s)
        })
        .collect();
    let overlapped: Vec<_> = starts
        .iter()
        .zip(&rhythm)
        .enumerate()
        .map(|(i, (&s, &d))| {
            let on = (s as i32 + if i == 0 { 0 } else { jitter(&mut seed, 20) }) as u32;
            let off = (s + d) as i32 + 40 + jitter(&mut seed, 20);
            (on, off as u32, scale[i % 8], 85)
        })
        .collect();
    out.push(Case {
        name: "performed legato overlaps",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &overlapped)]),
        notes: starts
            .iter()
            .zip(&rhythm)
            .enumerate()
            .map(|(i, (&s, &d))| (s, d, scale[i % 8] as i32))
            .collect(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // A piano on one track whose hands cross middle C: the right hand's
    // melody dips to A3, the left hand's line climbs to E4.
    let mut seed = 29u32;
    let rh = [72u8, 67, 64, 60, 59, 57, 60, 64];
    let lh = [48u8, 52, 55, 60, 64, 60, 55, 48];
    let mut crossing: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut crossing_notes = Vec::new();
    for i in 0..8u32 {
        let on = (i as i32 * 480 + if i == 0 { 0 } else { jitter(&mut seed, 8) }) as u32;
        crossing.push((on, on + 440, rh[i as usize], 85));
        crossing.push((on + 3, on + 430, lh[i as usize] - 12, 70));
        crossing_notes.push((i * 480, 480, rh[i as usize] as i32));
        crossing_notes.push((i * 480, 480, lh[i as usize] as i32 - 12));
    }
    out.push(Case {
        name: "piano, one track, hands cross",
        bytes: smf::build(480, &four, &[Track::new("Piano", 0, &crossing)]),
        notes: crossing_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Dotted eighth and sixteenth pairs, played ±20.
    let mut seed = 31u32;
    let mut dotted: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut dotted_notes = Vec::new();
    for i in 0..8u32 {
        for (k, (at, len)) in [(0u32, 360u32), (360, 120)].into_iter().enumerate() {
            let start = i * 480 + at;
            let on = (start as i32 + if start == 0 { 0 } else { jitter(&mut seed, 20) }) as u32;
            let off = (start as i32 + len as i32 * 9 / 10 + jitter(&mut seed, 15)) as u32;
            let key = scale[((i as usize) + k) % 8];
            dotted.push((on, off, key, 85));
            dotted_notes.push((start, len, key as i32));
        }
    }
    out.push(Case {
        name: "performed dotted pairs ±20",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &dotted)]),
        notes: dotted_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // A bar of sixteenths and triplet eighths, beat by beat, played ±15.
    let mut seed = 37u32;
    let mut mixed: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut mixed_notes = Vec::new();
    for beat in 0..8u32 {
        let (n, len) = if beat % 2 == 0 {
            (4u32, 120u32)
        } else {
            (3, 160)
        };
        for k in 0..n {
            let start = beat * 480 + k * len;
            let on = (start as i32 + if start == 0 { 0 } else { jitter(&mut seed, 15) }) as u32;
            let off = (start as i32 + len as i32 * 9 / 10 + jitter(&mut seed, 10)) as u32;
            let key = scale[((beat + k) % 8) as usize];
            mixed.push((on, off, key, 85));
            mixed_notes.push((start, len, key as i32));
        }
    }
    out.push(Case {
        name: "performed 16ths and triplets",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &mixed)]),
        notes: mixed_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Played 16ths whose last note is 4 louder by chance: no staccato (the
    // +4 rule is how LilyPond and lytk write one, not how anyone plays).
    let offsets = [0i32, 5, -6, 4];
    let loud_last: Vec<_> = (0..32u32)
        .map(|i| {
            let on = (i as i32 * 120 + offsets[i as usize % 4]) as u32;
            (
                on,
                on + 100,
                scale[(i % 8) as usize],
                if i == 31 { 84 } else { 80 },
            )
        })
        .collect();
    out.push(Case {
        name: "played, last note louder",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &loud_last)]),
        notes: (0..32u32)
            .map(|i| (i * 120, 120, scale[(i % 8) as usize] as i32))
            .collect(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // Swung eighths, played: long-short pairs at 2:1 and at 3:2, ±8 ticks,
    // read as straight eighths. (A 3:1 shuffle reads as dotted rhythms, as
    // it is usually written: "performed dotted pairs".)
    for (name, at) in [("played swing 2:1", 320u32), ("played swing 3:2", 288)] {
        let mut seed = 41u32;
        let mut swung: Vec<(u32, u32, u8, u8)> = Vec::new();
        let mut straight = Vec::new();
        for b in 0..8u32 {
            let j = if b == 0 { 0 } else { jitter(&mut seed, 8) };
            let on = (b as i32 * 480 + j) as u32;
            swung.push((on, b * 480 + at - 20, scale[b as usize], 85));
            let off = (b as i32 * 480 + at as i32 + jitter(&mut seed, 8)) as u32;
            swung.push((off, b * 480 + 470, scale[(b as usize + 2) % 8], 80));
            straight.push((b * 480, 240, scale[b as usize] as i32));
            straight.push((b * 480 + 240, 240, scale[(b as usize + 2) % 8] as i32));
        }
        out.push(Case {
            name,
            bytes: smf::build(480, &four, &[Track::new("p", 0, &swung)]),
            notes: straight,
            bars: bars(&[(2, q(1, 1))]),
        });
    }

    // Swing with one beat of real triplets, and off-beats after rests.
    let mut seed = 43u32;
    let mut mixed_swing: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut mixed_swing_notes = Vec::new();
    for b in 0..8u32 {
        let j = |seed: &mut u32| if b == 0 { 0 } else { jitter(seed, 6) };
        if b == 5 {
            for k in 0..3u32 {
                let on = (b as i32 * 480 + k as i32 * 160 + j(&mut seed)) as u32;
                mixed_swing.push((on, on + 140, scale[k as usize], 85));
                mixed_swing_notes.push((b * 480 + k * 160, 160, scale[k as usize] as i32));
            }
            continue;
        }
        if b % 3 != 2 {
            let on = (b as i32 * 480 + j(&mut seed)) as u32;
            mixed_swing.push((on, b * 480 + 300, 60, 85));
            mixed_swing_notes.push((b * 480, 240, 60));
        }
        let off = (b as i32 * 480 + 320 + j(&mut seed)) as u32;
        mixed_swing.push((off, b * 480 + 470, 64, 80));
        mixed_swing_notes.push((b * 480 + 240, 240, 64));
    }
    out.push(Case {
        name: "swing, a triplet, rests",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &mixed_swing)]),
        notes: mixed_swing_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Swung comping: off-beat chords on a guitar over a walking bass on its
    // own track (the bass has nothing inside a beat: it isn't warped).
    let k = [4i32, -5, 6, -3, 7, -7, 5, -4];
    let j = [0i32, 5, -4, 7, -6, 3, -3, 6];
    let mut guitar = Track::new("Guitar", 1, &[]);
    guitar.events.push((0, Ev::Program(24)));
    let mut bass = Track::new("Bass", 2, &[]);
    bass.events.push((0, Ev::Program(32)));
    let mut comping_notes = Vec::new();
    for b in 0..8u32 {
        let on = (b as i32 * 480 + 320 + k[b as usize]) as u32;
        for key in [60u8, 64, 67] {
            guitar.notes.push((on, b * 480 + 470, key, 80));
            comping_notes.push((b * 480 + 240, 240, key as i32));
        }
        let at = (b as i32 * 480 + j[b as usize]) as u32;
        bass.notes.push((at, b * 480 + 440, 43, 85));
        comping_notes.push((b * 480, 480, 43));
    }
    out.push(Case {
        name: "swung comping, two tracks",
        bytes: smf::build(480, &four, &[guitar, bass]),
        notes: comping_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Swung pairs with downbeats played off the click: one late, one early,
    // then all of them 40 ticks behind; and strummed off-beat chords.
    let swung_with = |lag: &dyn Fn(u32) -> i32| {
        let j = [0i32, 5, -4, 6, -5, 3, -6, 4];
        let mut v: Vec<(u32, u32, u8, u8)> = Vec::new();
        for b in 0..8u32 {
            let d = lag(b) + j[b as usize];
            v.push(((b as i32 * 480 + d) as u32, b * 480 + 300, 60, 85));
            let off = b as i32 * 480 + 320 + d.clamp(-6, 46) - j[(b as usize + 3) % 8];
            v.push((off as u32, b * 480 + 470, 64, 80));
        }
        v
    };
    let straight_pairs: Vec<(u32, u32, i32)> = (0..8u32)
        .flat_map(|b| [(b * 480, 240, 60), (b * 480 + 240, 240, 64)])
        .collect();
    for (name, notes) in [
        (
            "swing, a late downbeat",
            swung_with(&|b| if b == 3 { 35 } else { 0 }),
        ),
        (
            "swing, an early downbeat",
            swung_with(&|b| if b == 4 { -35 } else { 0 }),
        ),
        (
            "swing, 40 ticks behind",
            swung_with(&|b| if b == 0 { 0 } else { 40 }),
        ),
    ] {
        out.push(Case {
            name,
            bytes: smf::build(480, &four, &[Track::new("p", 0, &notes)]),
            notes: straight_pairs.clone(),
            bars: bars(&[(2, q(1, 1))]),
        });
    }
    let mut strum: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut strum_notes = Vec::new();
    let jj = [0u32, 5, 3, 6, 2, 4, 1, 7];
    for b in 0..8u32 {
        strum.push((b * 480 + jj[b as usize], b * 480 + 440, 43, 85));
        strum_notes.push((b * 480, 480, 43));
        for (k, d) in [(60u8, 320u32), (64, 332), (67, 345)] {
            strum.push((b * 480 + d + jj[(b as usize + 2) % 8], b * 480 + 470, k, 80));
            strum_notes.push((b * 480 + 240, 240, k as i32));
        }
    }
    out.push(Case {
        name: "swing, strummed chords",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &strum)]),
        notes: strum_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Rhythms whose notes the metric cost used to pull onto strong beats:
    // hits after rests, long syncopations, an anticipation, a dotted quarter
    // and a rest (each played within 7 ticks).
    let five = [(0, Ev::Time(5, 4))];
    /// (name, meter, played (on, off, key), written notes, bar lengths)
    type Rhythm<'a> = (
        &'static str,
        &'a [(u32, Ev)],
        Vec<(u32, u32, u8)>,
        Vec<(u32, u32, i32)>,
        Vec<Frac>,
    );
    let rhythms: Vec<Rhythm<'_>> = vec![
        (
            "r4 c4 r2 bars",
            &four,
            vec![
                (487, 887, 60),
                (2394, 2794, 62),
                (4325, 4725, 64),
                (6233, 6633, 65),
            ],
            vec![
                (480, 480, 60),
                (2400, 480, 62),
                (4320, 480, 64),
                (6240, 480, 65),
            ],
            bars(&[(4, q(1, 1))]),
        ),
        (
            "c4 r8 d8 r2 | e1",
            &four,
            vec![(0, 450, 60), (727, 900, 62), (1925, 3800, 64)],
            vec![(0, 480, 60), (720, 240, 62), (1920, 1920, 64)],
            bars(&[(2, q(1, 1))]),
        ),
        (
            "c8 d2 e4. | f1",
            &four,
            vec![
                (0, 216, 60),
                (247, 1150, 62),
                (1193, 1880, 64),
                (1925, 3800, 65),
            ],
            vec![
                (0, 240, 60),
                (240, 960, 62),
                (1200, 720, 64),
                (1920, 1920, 65),
            ],
            bars(&[(2, q(1, 1))]),
        ),
        (
            "c2. d8 e8~ | e1",
            &four,
            vec![(0, 1400, 60), (1447, 1660, 62), (1687, 3800, 64)],
            vec![(0, 1440, 60), (1440, 240, 62), (1680, 2160, 64)],
            bars(&[(2, q(1, 1))]),
        ),
        (
            "5/4: c4 d1 | e4 f1",
            &five,
            vec![
                (0, 450, 60),
                (487, 2350, 62),
                (2407, 2850, 64),
                (2887, 4750, 65),
            ],
            vec![
                (0, 480, 60),
                (480, 1920, 62),
                (2400, 480, 64),
                (2880, 1920, 65),
            ],
            bars(&[(2, q(5, 4))]),
        ),
        (
            "c4. r8 d4 e4 | f1",
            &four,
            vec![
                (0, 684, 60),
                (967, 1420, 62),
                (1447, 1880, 64),
                (1925, 3800, 65),
            ],
            vec![
                (0, 720, 60),
                (960, 480, 62),
                (1440, 480, 64),
                (1920, 1920, 65),
            ],
            bars(&[(2, q(1, 1))]),
        ),
    ];
    for (name, meter, played, written, bar_lens) in rhythms {
        out.push(Case {
            name,
            bytes: smf::build(
                480,
                meter,
                &[Track::new(
                    "p",
                    0,
                    &played
                        .iter()
                        .map(|&(a, b, k)| (a, b, k, 85))
                        .collect::<Vec<_>>(),
                )],
            ),
            notes: written,
            bars: bar_lens,
        });
    }

    // Swung pairs with one beat of sixteenths: the sixteenths stay.
    let mut seed = 47u32;
    let mut swung16: Vec<(u32, u32, u8, u8)> = Vec::new();
    let mut swung16_notes = Vec::new();
    for b in 0..8u32 {
        if b == 6 {
            for (i, at) in [2880u32, 3004, 3115, 3243].into_iter().enumerate() {
                swung16.push((at, at + 100, scale[i], 85));
                swung16_notes.push((2880 + i as u32 * 120, 120, scale[i] as i32));
            }
            continue;
        }
        let on = (b as i32 * 480 + if b == 0 { 0 } else { jitter(&mut seed, 6) }) as u32;
        swung16.push((on, b * 480 + 300, 60, 85));
        let off = (b as i32 * 480 + 320 + jitter(&mut seed, 6)) as u32;
        swung16.push((off, b * 480 + 470, 64, 80));
        swung16_notes.push((b * 480, 240, 60));
        swung16_notes.push((b * 480 + 240, 240, 64));
    }
    out.push(Case {
        name: "swing with a beat of 16ths",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &swung16)]),
        notes: swung16_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Halves played detached (65 % of their length): halves, not quarters
    // and rests (they are marked staccato).
    let halves = [
        (0u32, 624u32, 60u8, 80u8),
        (967, 1591, 62, 80),
        (1915, 2539, 64, 80),
        (2889, 3513, 65, 80),
    ];
    out.push(Case {
        name: "played detached halves",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &halves)]),
        notes: vec![
            (0, 960, 60),
            (960, 960, 62),
            (1920, 960, 64),
            (2880, 960, 65),
        ],
        bars: bars(&[(2, q(1, 1))]),
    });

    // Quarters up to 40 ticks off (more than a 64th): still quarters.
    let off40 = [0u32, 520, 925, 1475, 1935, 2360, 2915, 3320];
    out.push(Case {
        name: "played quarters ±40",
        bytes: smf::build(
            480,
            &four,
            &[Track::new(
                "p",
                0,
                &off40
                    .iter()
                    .enumerate()
                    .map(|(i, &on)| (on, on + 440, scale[i], 85))
                    .collect::<Vec<_>>(),
            )],
        ),
        notes: quarters.clone(),
        bars: bars(&[(2, q(1, 1))]),
    });

    // Eighth triplets with one sloppy beat (835 is 35 ticks off 800).
    let sloppy = [
        0u32, 165, 315, 480, 630, 835, 960, 1125, 1275, 1440, 1605, 1765,
    ];
    out.push(Case {
        name: "triplets, one sloppy beat",
        bytes: smf::build(
            480,
            &four,
            &[Track::new(
                "p",
                0,
                &sloppy
                    .iter()
                    .enumerate()
                    .map(|(i, &on)| (on, on + 150, scale[i % 8], 85))
                    .collect::<Vec<_>>(),
            )],
        ),
        notes: {
            let mut n: Vec<_> = (0..12u32)
                .map(|i| (i * 160, 160, scale[(i % 8) as usize] as i32))
                .collect();
            // The last is held to the bar's end.
            n[11].1 = 160;
            n
        },
        bars: bars(&[(1, q(1, 1))]),
    });

    // 6/8 eighths up to 40 ticks off.
    let six_eight = [(0, Ev::Time(6, 8))];
    let e68 = [0u32, 280, 440, 760, 925, 1175];
    out.push(Case {
        name: "6/8 eighths ±40",
        bytes: smf::build(
            480,
            &six_eight,
            &[Track::new(
                "p",
                0,
                &e68.iter()
                    .enumerate()
                    .map(|(i, &on)| (on, on + 220, scale[i], 85))
                    .collect::<Vec<_>>(),
            )],
        ),
        notes: (0..6u32)
            .map(|i| (i * 240, 240, scale[i as usize] as i32))
            .collect(),
        bars: bars(&[(1, q(3, 4))]),
    });

    // A chord rolled over 70 ticks, then a dotted half.
    let rolled = [
        (0u32, 470u32, 60u8, 80u8),
        (35, 470, 64, 80),
        (70, 470, 67, 80),
        (480, 1910, 65, 80),
    ];
    out.push(Case {
        name: "rolled chord",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &rolled)]),
        notes: vec![(0, 480, 60), (0, 480, 64), (0, 480, 67), (480, 1440, 65)],
        bars: bars(&[(1, q(1, 1))]),
    });

    // One bar: triplets, then sixteenths, then a half, played ±6.
    let fam = [
        (0u32, 150u32, 60u8),
        (166, 310, 62),
        (313, 470, 64),
        (485, 590, 65),
        (595, 710, 67),
        (726, 830, 69),
        (835, 950, 71),
        (966, 1855, 72),
    ];
    out.push(Case {
        name: "triplets then 16ths",
        bytes: smf::build(
            480,
            &four,
            &[Track::new(
                "p",
                0,
                &fam.iter()
                    .map(|&(a, b, k)| (a, b, k, 85))
                    .collect::<Vec<_>>(),
            )],
        ),
        notes: vec![
            (0, 160, 60),
            (160, 160, 62),
            (320, 160, 64),
            (480, 120, 65),
            (600, 120, 67),
            (720, 120, 69),
            (840, 120, 71),
            (960, 960, 72),
        ],
        bars: bars(&[(1, q(1, 1))]),
    });

    // A take that starts 100 ticks late, and one played steadily 3 % slower
    // than the file's tempo: quarters from bar 1.
    let jit = [0u32, 7, 3, 9, 1, 4, 2, 6];
    for (name, first, gap) in [
        ("played, starting late", 100u32, 480u32),
        ("played 3 % slower", 0, 494),
    ] {
        let played: Vec<_> = (0..8u32)
            .map(|i| {
                let on = first + i * gap + if i == 0 { 0 } else { jit[i as usize] };
                (on, on + gap * 9 / 10, scale[i as usize], 85)
            })
            .collect();
        out.push(Case {
            name,
            bytes: smf::build(480, &four, &[Track::new("p", 0, &played)]),
            notes: quarters.clone(),
            bars: bars(&[(2, q(1, 1))]),
        });
    }

    // Rubato at 96 ticks a quarter (gaps 96 + 3i): at such a resolution most
    // ticks lie within two of some tuplet point, and yet this is a
    // performance.
    let mut at = 0u32;
    let low: Vec<_> = (0..16u32)
        .map(|i| {
            let n = (at, at + (96 + 3 * i) * 9 / 10, scale[(i % 8) as usize], 85);
            at += 96 + 3 * i;
            n
        })
        .collect();
    out.push(Case {
        name: "rubato at 96 ppq",
        bytes: smf::build(96, &four, &[Track::new("p", 0, &low)]),
        notes: (0..16u32)
            .map(|i| (i * 480, 480, scale[(i % 8) as usize] as i32))
            .collect(),
        bars: bars(&[(4, q(1, 1))]),
    });

    // A first note a 16th after the bar line keeps its rest.
    let mut rest_first: Vec<(u32, u32, u8, u8)> = vec![(120, 470, scale[0], 85)];
    let mut rest_first_notes = vec![(120, 360, scale[0] as i32)];
    for i in 1..8u32 {
        let on = i * 480 + jit[i as usize];
        rest_first.push((on, on + 440, scale[i as usize], 85));
        rest_first_notes.push((i * 480, 480, scale[i as usize] as i32));
    }
    out.push(Case {
        name: "played, a 16th rest first",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &rest_first)]),
        notes: rest_first_notes,
        bars: bars(&[(2, q(1, 1))]),
    });

    // Rubato: quarters slowing from 120 to about 96 bpm with no tempo event
    // (beat tracking reads them as quarters).
    let mut t = 0u32;
    let mut rubato = Vec::new();
    for i in 0..8u32 {
        let ioi = 480 + i * 17;
        rubato.push((t, t + ioi * 9 / 10, scale[i as usize], 85));
        t += ioi;
    }
    out.push(Case {
        name: "performed rubato",
        bytes: smf::build(480, &four, &[Track::new("p", 0, &rubato)]),
        notes: quarters.clone(),
        bars: bars(&[(2, q(1, 1))]),
    });
    out
}

// ---------------------------------------------------------------------------
// The board
// ---------------------------------------------------------------------------

#[test]
fn midi_truth_board() {
    let mut quality = Quality::default();

    // ----- Source truth and LilyPond parity -----
    let (mut onset_pitch, mut full, mut src_notes) = (0, 0, 0);
    let (mut ts_ok, mut ts_total, mut ks_ok, mut ks_total, mut bars_ok) = (0, 0, 0, 0, 0);
    let (mut par_on, mut par_on_off, mut par_full, mut par_total) = (0, 0, 0, 0);
    println!("\n===== MIDI against the source =====");
    for (ly, movement, midi) in PAIRS {
        let source = ly_movement(ly, movement);
        let bytes = std::fs::read(Path::new("tests/fixtures/midi").join(midi)).expect("fixture");
        let Some(import) = read_midi(&bytes) else {
            println!("{midi}: import failed");
            continue;
        };
        quality.add(&import);
        let src = note_signature(&source);
        let imp = note_signature(&import);
        let op = common_count(
            &src.iter().map(|n| (n.0, n.2)).collect::<Vec<_>>(),
            &imp.iter().map(|n| (n.0, n.2)).collect::<Vec<_>>(),
        );
        let fu = common_count(&src, &imp);
        let (src_ts, imp_ts) = (signatures_at(&source, false), signatures_at(&import, false));
        let (src_ks, imp_ks) = (signatures_at(&source, true), signatures_at(&import, true));
        let ts = common_count(&src_ts, &imp_ts);
        let ks = common_count(&src_ks, &imp_ks);
        let same_bars = bar_lengths(&source) == bar_lengths(&import);
        println!(
            "{midi:<16} notes {}/{} by onset+pitch, {}/{} with duration; time sigs {}/{}, keys {}/{}; bars {} vs {}",
            op,
            src.len(),
            fu,
            src.len(),
            ts,
            src_ts.len(),
            ks,
            src_ks.len(),
            bar_lengths(&import).len(),
            bar_lengths(&source).len(),
        );
        onset_pitch += op;
        full += fu;
        src_notes += src.len();
        ts_ok += ts;
        ts_total += src_ts.len();
        ks_ok += ks;
        ks_total += src_ks.len();
        bars_ok += usize::from(same_bars);

        // LilyPond parity: our export of the same source, note by note.
        let ours = IrToMidiAdapter::new()
            .convert_bytes(&source)
            .expect("export");
        let theirs = smf::notes(&bytes);
        let mine = smf::notes(&ours);
        let scale = |n: &smf::SmfNote, p: u32| (n.on * 384 / p, n.off * 384 / p);
        let (pt, po) = (smf::ppq(&bytes) as u32, smf::ppq(&ours) as u32);
        let key = |v: &[smf::SmfNote], p: u32, what: u8| -> Vec<(u32, u32, u8, u8)> {
            v.iter()
                .map(|n| {
                    let (on, off) = scale(n, p);
                    match what {
                        0 => (on, 0, n.pitch, 0),
                        1 => (on, off, n.pitch, 0),
                        _ => (on, off, n.pitch, n.vel),
                    }
                })
                .collect()
        };
        let a = common_count(&key(&theirs, pt, 0), &key(&mine, po, 0));
        let b = common_count(&key(&theirs, pt, 1), &key(&mine, po, 1));
        let c = common_count(&key(&theirs, pt, 2), &key(&mine, po, 2));
        println!(
            "{:<16} LilyPond parity: {a}/{n} on+pitch, {b}/{n} +off, {c}/{n} +velocity (ours: {m} notes)",
            "",
            n = theirs.len(),
            m = mine.len()
        );
        par_on += a;
        par_on_off += b;
        par_full += c;
        par_total += theirs.len();
    }

    // ----- Synthetic cases -----
    println!("\n===== Synthetic MIDI =====");
    let (mut synth_notes, mut synth_bars, mut synth_total) = (0, 0, 0);
    for case in cases() {
        synth_total += 1;
        let Some(import) = read_midi(&case.bytes) else {
            println!("{:<26} import failed", case.name);
            continue;
        };
        quality.add(&import);
        let got = note_signature(&import);
        let mut want = case.notes.clone();
        want.sort_unstable();
        let notes_ok = got == want;
        let bars_ok_case = bar_lengths(&import) == case.bars;
        synth_notes += usize::from(notes_ok);
        synth_bars += usize::from(bars_ok_case);
        println!(
            "{:<26} notes {} ({} of {} right), bars {}",
            case.name,
            if notes_ok { "ok" } else { "WRONG" },
            common_count(&got, &want),
            want.len(),
            if bars_ok_case { "ok" } else { "WRONG" },
        );
        if !notes_ok {
            let missing: Vec<_> = want.iter().filter(|w| !got.contains(w)).take(6).collect();
            let extra: Vec<_> = got.iter().filter(|g| !want.contains(g)).take(6).collect();
            println!("    want {missing:?}\n    got  {extra:?}");
        }
    }

    // ----- MusicXML corpus → MIDI → IR -----
    let (mut corpus_notes, mut corpus_bars, mut corpus_total) = (0, 0, 0);
    let mut xml: Vec<_> = std::fs::read_dir("tests/fixtures/xml")
        .expect("xml fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .collect();
    xml.sort();
    let mut corpus_fails = Vec::new();
    let mut corpus_bar_fails: Vec<String> = Vec::new();
    for path in xml {
        let Some(source) = safe(|| MxmlToIrAdapter::new().convert_file(&path).ok()) else {
            continue;
        };
        corpus_total += 1;
        let back = safe(|| {
            let bytes = IrToMidiAdapter::new().convert_bytes(&source).ok()?;
            MidiToIrAdapter::new().convert_bytes(&bytes).ok()
        });
        let Some(back) = back else {
            corpus_fails.push(path.file_name().unwrap().to_string_lossy().to_string());
            continue;
        };
        let ok = note_signature(&source) == note_signature(&back);
        corpus_notes += usize::from(ok);
        let same_bars = bar_lengths(&source) == bar_lengths(&back);
        corpus_bars += usize::from(same_bars);
        if !same_bars {
            corpus_bar_fails.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
        if !ok && corpus_fails.len() < 30 {
            corpus_fails.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }

    // ----- Report -----
    println!("\n===== MIDI truth board =====");
    println!(
        "source truth : {onset_pitch}/{src_notes} onset+pitch, {full}/{src_notes} +duration, time sigs {ts_ok}/{ts_total}, keys {ks_ok}/{ks_total}, bars {bars_ok}/{}",
        PAIRS.len()
    );
    println!("LilyPond     : {par_on}/{par_total} on+pitch, {par_on_off}/{par_total} +off, {par_full}/{par_total} +velocity");
    println!("synthetic    : notes {synth_notes}/{synth_total}, bars {synth_bars}/{synth_total}");
    println!(
        "corpus       : notes {corpus_notes}/{corpus_total}, bars {corpus_bars}/{corpus_total}"
    );
    println!("corpus misses: {}", corpus_fails.join(", "));
    println!("corpus bars  : {}", corpus_bar_fails.join(", "));
    println!(
        "quality      : {} imports, {} rests, {} odd durations, max {} voices a staff has in a bar, {} with equal bar counts",
        quality.imports, quality.rests, quality.odd_durations, quality.max_voices, quality.equal_bars
    );

    let rise = [
        ("source onset+pitch", onset_pitch, TRUTH_ONSET_PITCH),
        ("source +duration", full, TRUTH_FULL),
        ("source time sigs", ts_ok, TRUTH_TIME_SIGS),
        ("source keys", ks_ok, TRUTH_KEY_SIGS),
        ("source bars", bars_ok, TRUTH_BARS),
        ("parity on+pitch", par_on, PARITY_ON),
        ("parity +off", par_on_off, PARITY_ON_OFF),
        ("parity +velocity", par_full, PARITY_FULL),
        ("synthetic notes", synth_notes, SYNTH_NOTES),
        ("synthetic bars", synth_bars, SYNTH_BARS),
        ("corpus notes", corpus_notes, CORPUS_NOTES),
        ("corpus bars", corpus_bars, CORPUS_BARS),
        ("equal bar counts", quality.equal_bars, QUALITY_EQUAL_BARS),
    ];
    for (name, got, base) in rise {
        assert!(got >= base, "{name} regressed: {got} < {base}");
    }
    let fall = [
        ("rests", quality.rests, QUALITY_MAX_RESTS),
        (
            "odd durations",
            quality.odd_durations,
            QUALITY_MAX_ODD_DURATIONS,
        ),
        ("voices a bar", quality.max_voices, QUALITY_MAX_VOICES),
    ];
    for (name, got, base) in fall {
        assert!(got <= base, "{name} got worse: {got} > {base}");
    }
}

#[test]
fn smf_builder_roundtrips_notes() {
    let bytes = smf::build(
        480,
        &[(0, Ev::Time(3, 4))],
        &[Track::new("a", 2, &[(0, 480, 60, 90), (480, 960, 60, 80)])],
    );
    let n = smf::notes(&bytes);
    assert_eq!(n.len(), 2);
    assert_eq!(
        (n[1].on, n[1].off, n[1].pitch, n[1].vel, n[1].channel),
        (480, 960, 60, 80, 2)
    );
    assert!(smf::dump(&bytes).contains("time 3/4"));
}
