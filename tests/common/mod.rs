//! Shared helpers for semantic round-trip / fidelity tests (Epic C).
//!
//! A "signature" is a computable summary of the *musical content* of a Score —
//! pitches, durations, dynamics, articulations, ties/slurs, lyrics, harmonies,
//! time/key signatures. Round-trip tests compare signatures (or robust subsets)
//! before and after a conversion to assert content is preserved, rather than
//! merely "parses" or "is non-empty".

#![allow(dead_code)] // helpers are shared across test binaries; not all are used by each

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
use _core::ir::note::VoiceElement;
use _core::ir::score::Score;

// ---------------------------------------------------------------------------
// Conversion shortcuts
// ---------------------------------------------------------------------------

pub fn ly_to_score(src: &str) -> Score {
    LyToIrAdapter::new().convert_str(src).expect("LY → Score")
}

pub fn score_to_ly_score_path(score: &Score) -> String {
    IrToLyAdapter::new().convert(score).expect("Score → LY")
}

/// LilyPond → LilyPond via the Music-tree path (the path the CLI uses).
pub fn ly_to_ly_music(src: &str) -> String {
    let doc = LyToIrAdapter::new()
        .convert_str_to_music(src)
        .expect("LY → Music");
    IrToLyAdapter::new()
        .convert_music(&doc)
        .expect("Music → LY")
}

pub fn xml_to_score(src: &str) -> Score {
    MxmlToIrAdapter::new()
        .convert_str(src)
        .expect("XML → Score")
}

pub fn score_to_xml(score: &Score) -> String {
    IrToMxmlAdapter::new().convert(score).expect("Score → XML")
}

// ---------------------------------------------------------------------------
// Signature
// ---------------------------------------------------------------------------

/// A computable summary of a score's musical content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sig {
    pub parts: usize,
    /// All note MIDI numbers across all parts/voices, in document order.
    pub pitches: Vec<i32>,
    /// Total note count (chord notes counted individually).
    pub notes: usize,
    /// Total rest count.
    pub rests: usize,
    /// Dynamic signs attached to notes/rests (e.g. "mf").
    pub dynamics: Vec<String>,
    /// Articulation names attached to notes.
    pub articulations: Vec<String>,
    /// Lyric syllable texts.
    pub lyrics: Vec<String>,
    /// Harmonies as (root step, kind).
    pub harmonies: Vec<(String, String)>,
    /// Figured-bass figure counts per entry.
    pub figured_bass: usize,
    /// Tie events (start/stop) total.
    pub ties: usize,
    /// Slur events (start/stop) total.
    pub slurs: usize,
    /// Time signatures as "beats/beat_type".
    pub time_sigs: Vec<String>,
}

/// Multiset of pitches (sorted), robust to voice/ordering differences.
pub fn pitch_multiset(score: &Score) -> Vec<i32> {
    let mut v = signature(score).pitches;
    v.sort_unstable();
    v
}

/// Sorted `(onset, duration, pitch)` multiset in 480-steps-per-quarter, computed
/// via the Music-tree note-array. Unlike [`pitch_multiset`] this also captures
/// ONSET and DURATION, so it catches corruption (tuplet ratios, grace timing,
/// tie collapse, bar drift) that an unchanged pitch set would hide.
pub fn note_signature(score: &Score) -> Vec<(u32, u32, i32)> {
    let doc = _core::ir::lift::lift_to_music(score);
    let arr = _core::representations::to_note_array(&doc, 480);
    let mut v: Vec<(u32, u32, i32)> = arr
        .notes
        .iter()
        .map(|n| (n.onset, n.duration, n.pitch as i32))
        .collect();
    v.sort_unstable();
    v
}

pub fn signature(score: &Score) -> Sig {
    let mut s = Sig {
        parts: score.parts().len(),
        ..Default::default()
    };

    for part in score.parts() {
        for m in &part.measures {
            if let Some(attrs) = &m.attributes {
                if let Some(ts) = &attrs.time {
                    s.time_sigs.push(format!("{}/{}", ts.beats, ts.beat_type));
                }
            }
            for h in &m.harmonies {
                s.harmonies.push((h.root.step.clone(), h.kind.clone()));
            }
            s.figured_bass += m.figured_bass.len();
            for v in &m.voices {
                for e in &v.elements {
                    match e {
                        VoiceElement::Note(n) => {
                            s.notes += 1;
                            s.pitches.push(n.pitch.midi_number());
                            collect_note(&mut s, n);
                        }
                        VoiceElement::Rest(r) => {
                            s.rests += 1;
                            for d in &r.dynamics {
                                s.dynamics.push(d.sign.clone());
                            }
                        }
                        VoiceElement::Chord(c) => {
                            for n in &c.notes {
                                s.notes += 1;
                                s.pitches.push(n.pitch.midi_number());
                                collect_note(&mut s, n);
                            }
                        }
                    }
                }
            }
        }
    }
    s
}

fn collect_note(s: &mut Sig, n: &_core::ir::note::Note) {
    use _core::ir::articulation::StartStop;
    for d in &n.dynamics {
        s.dynamics.push(d.sign.clone());
    }
    for a in &n.articulations {
        s.articulations.push(a.name.clone());
    }
    for l in &n.lyrics {
        s.lyrics.push(l.text.clone());
    }
    s.ties += n.ties.len();
    s.slurs += n
        .slurs
        .iter()
        .filter(|x| matches!(x.slur_type, StartStop::Start | StartStop::Stop))
        .count();
}

/// Sorted dynamics for order-independent comparison.
pub fn sorted_dynamics(score: &Score) -> Vec<String> {
    let mut v = signature(score).dynamics;
    v.sort();
    v
}

/// Per-measure summed voice duration (grace notes excluded) for a part, as
/// `(numer, denom)` of a whole note. Used to check that bar splitting is
/// consistent across parts.
pub fn part_measure_durations(score: &Score, part_idx: usize) -> Vec<(i64, i64)> {
    use _core::ir::duration::Frac;
    let part = &score.parts()[part_idx];
    part.measures
        .iter()
        .map(|m| {
            let total: Frac = m
                .voices
                .iter()
                .flat_map(|v| &v.elements)
                .filter(|e| !matches!(e, VoiceElement::Note(n) if n.is_grace))
                .map(|e| match e {
                    VoiceElement::Note(n) => n.duration.actual_duration(),
                    VoiceElement::Rest(r) => r.duration.actual_duration(),
                    VoiceElement::Chord(c) => c.duration.actual_duration(),
                })
                .fold(Frac::from_integer(0), |a, d| a + d);
            (*total.numer(), *total.denom())
        })
        .collect()
}
