//! Statistics of a score: what `lytk.info` and `lytk info --json` report.

use std::collections::BTreeSet;

use super::duration::Frac;
use super::note::{Note, VoiceElement};
use super::score::Score;
use super::Part;

/// A part's counts.
#[derive(Debug, Clone, PartialEq)]
pub struct PartInfo {
    pub measures: usize,
    /// Distinct voice numbers in its measures.
    pub voices: usize,
    /// Sounding notes: chord members and grace notes each count.
    pub notes: usize,
}

/// A score's counts.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreInfo {
    pub parts: Vec<PartInfo>,
    pub note_count: usize,
    pub voice_count: usize,
    /// Bars of the score (its longest part's).
    pub bar_count: usize,
    /// Length in quarter notes: each bar as long as its longest voice.
    pub duration_quarters: Frac,
    /// Lyric syllables.
    pub lyric_count: usize,
    /// Chord symbols (harmonies).
    pub chord_symbol_count: usize,
    pub grace_note_count: usize,
}

fn notes(element: &VoiceElement) -> &[Note] {
    match element {
        VoiceElement::Note(n) => std::slice::from_ref(n.as_ref()),
        VoiceElement::Chord(c) => &c.notes,
        VoiceElement::Rest(_) => &[],
    }
}

fn part_length(part: &Part) -> Frac {
    part.measures
        .iter()
        .map(|m| {
            m.voices
                .iter()
                .map(|v| {
                    v.elements
                        .iter()
                        .map(VoiceElement::metric_duration)
                        .sum::<Frac>()
                })
                .max()
                .unwrap_or_default()
        })
        .sum()
}

/// The counts of `score`.
pub fn score_info(score: &Score) -> ScoreInfo {
    let parts = score.parts();
    let mut info = ScoreInfo {
        parts: Vec::new(),
        note_count: 0,
        voice_count: 0,
        bar_count: parts.iter().map(|p| p.measures.len()).max().unwrap_or(0),
        duration_quarters: parts
            .iter()
            .map(|p| part_length(p))
            .max()
            .unwrap_or_default()
            * Frac::from_integer(4),
        lyric_count: 0,
        chord_symbol_count: 0,
        grace_note_count: 0,
    };
    for part in parts {
        let mut voices = BTreeSet::new();
        let mut part_notes = 0;
        for measure in &part.measures {
            info.chord_symbol_count += measure.harmonies.len();
            for voice in &measure.voices {
                voices.insert(voice.number);
                for note in voice.elements.iter().flat_map(notes) {
                    part_notes += 1;
                    info.lyric_count += note.lyrics.len();
                    info.grace_note_count += usize::from(note.is_grace);
                }
            }
        }
        info.note_count += part_notes;
        info.voice_count += voices.len();
        info.parts.push(PartInfo {
            measures: part.measures.len(),
            voices: voices.len(),
            notes: part_notes,
        });
    }
    info
}
