//! Lyrics emission.

use crate::ir::articulation::{LyricSyllable, StartStop, SyllabicType};
use crate::ir::note::VoiceElement;
use crate::ir::Part;

use super::helpers::{index_to_alpha, part_var_name};

#[derive(Debug, Clone)]
pub(super) enum LyricEvent {
    Syllable(LyricSyllable),
    Skip,
}

/// Extract lyrics from a part's notes, grouped by lyric number.
///
/// Walks notes in the same order as `attach_lyrics_to_part` in `ly_to_ir.rs`:
/// grace notes, tied continuations, `in_melisma` notes, and slur-interior notes
/// (when `no_auto_beam` is set) are automatically skipped by the voice -- no `_`
/// skip is needed in lyricmode for these. Only notes that *should* consume a
/// syllable but have no lyric attached get a `_` skip.
fn extract_lyrics(part: &Part) -> std::collections::BTreeMap<u8, Vec<LyricEvent>> {
    let mut lyrics_by_number: std::collections::BTreeMap<u8, Vec<LyricEvent>> =
        std::collections::BTreeMap::new();

    let mut open_slurs: u32 = 0;

    for measure in &part.measures {
        for voice in &measure.voices {
            for elem in &voice.elements {
                match elem {
                    VoiceElement::Note(note) => {
                        let starts = note
                            .slurs
                            .iter()
                            .filter(|s| s.slur_type == StartStop::Start)
                            .count() as u32;
                        let stops = note
                            .slurs
                            .iter()
                            .filter(|s| s.slur_type == StartStop::Stop)
                            .count() as u32;

                        if note.is_grace {
                            open_slurs = open_slurs.saturating_add(starts).saturating_sub(stops);
                            continue;
                        }

                        let is_tied_cont = note.ties.iter().any(|t| t.tie_type == StartStop::Stop);
                        let in_slur_melisma = note.no_auto_beam
                            && open_slurs > 0
                            && !note.slurs.iter().any(|s| s.slur_type == StartStop::Start);

                        open_slurs = open_slurs.saturating_add(starts).saturating_sub(stops);

                        // These notes are automatically skipped -- no lyric event needed
                        if is_tied_cont || note.in_melisma || in_slur_melisma {
                            continue;
                        }

                        // This note consumes a syllable position
                        if !note.lyrics.is_empty() {
                            for syl in &note.lyrics {
                                lyrics_by_number
                                    .entry(syl.number)
                                    .or_default()
                                    .push(LyricEvent::Syllable(syl.clone()));
                            }
                        } else {
                            // Note consumes a position but has no lyric -- emit skip
                            for lyrics in lyrics_by_number.values_mut() {
                                lyrics.push(LyricEvent::Skip);
                            }
                        }
                    }
                    VoiceElement::Chord(chord) => {
                        if let Some(first) = chord.notes.first() {
                            let starts = first
                                .slurs
                                .iter()
                                .filter(|s| s.slur_type == StartStop::Start)
                                .count() as u32;
                            let stops = first
                                .slurs
                                .iter()
                                .filter(|s| s.slur_type == StartStop::Stop)
                                .count() as u32;
                            open_slurs = open_slurs.saturating_add(starts).saturating_sub(stops);

                            if !first.lyrics.is_empty() {
                                for syl in &first.lyrics {
                                    lyrics_by_number
                                        .entry(syl.number)
                                        .or_default()
                                        .push(LyricEvent::Syllable(syl.clone()));
                                }
                            } else {
                                for lyrics in lyrics_by_number.values_mut() {
                                    lyrics.push(LyricEvent::Skip);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    lyrics_by_number
}

/// Check if a part has any lyrics on its notes.
pub(super) fn part_has_lyrics(part: &Part) -> bool {
    part.measures.iter().any(|m| {
        m.voices.iter().any(|v| {
            v.elements.iter().any(|e| match e {
                VoiceElement::Note(n) => !n.lyrics.is_empty(),
                VoiceElement::Chord(c) => c.notes.first().is_some_and(|n| !n.lyrics.is_empty()),
                _ => false,
            })
        })
    })
}

/// Emit a lyrics variable for a part.
pub(super) fn emit_lyrics_variable(part: &Part, lines: &mut Vec<String>) {
    let lyrics_map = extract_lyrics(part);
    if lyrics_map.is_empty() {
        return;
    }

    let var = part_var_name(part);

    for (&number, events) in &lyrics_map {
        let suffix = if lyrics_map.len() > 1 {
            format!("Verse{}", index_to_alpha(number as usize))
        } else {
            "Lyrics".to_string()
        };
        let lyrics_var = format!("{var}{suffix}");
        lines.push(format!("{lyrics_var} = \\lyricmode {{"));

        let mut tokens: Vec<String> = Vec::new();
        let mut i = 0;
        while i < events.len() {
            match &events[i] {
                LyricEvent::Skip => {
                    tokens.push("_".to_string());
                }
                LyricEvent::Syllable(syl) => {
                    let text = escape_lyric_text(&syl.text);
                    match syl.syllabic {
                        SyllabicType::Begin | SyllabicType::Middle => {
                            tokens.push(format!("{text} --"));
                        }
                        SyllabicType::End | SyllabicType::Single => {
                            tokens.push(text);
                        }
                    }
                    if syl.extend {
                        tokens.push("__".to_string());
                    }
                }
            }
            i += 1;
        }

        // Group tokens into lines of ~72 chars
        let pad = "  ";
        let mut current_line: Vec<&str> = Vec::new();
        let mut current_len = 0usize;
        for token in &tokens {
            current_len += token.len() + 1;
            current_line.push(token);
            if current_len > 72 {
                lines.push(format!("{pad}{}", current_line.join(" ")));
                current_line.clear();
                current_len = 0;
            }
        }
        if !current_line.is_empty() {
            lines.push(format!("{pad}{}", current_line.join(" ")));
        }

        lines.push("}".to_string());
        lines.push(String::new());
    }
}

/// Escape special characters in lyric text for LilyPond.
fn escape_lyric_text(text: &str) -> String {
    // Wrap in quotes if the text contains spaces or special chars
    if text.contains(' ') || text.contains('"') || text.contains('\\') {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        text.to_string()
    }
}

/// Emit lyrics references in the score block for a part.
pub(super) fn emit_lyrics_refs(
    part: &Part,
    voice_name: &str,
    indent: usize,
    lines: &mut Vec<String>,
) {
    let lyrics_map = extract_lyrics(part);
    if lyrics_map.is_empty() {
        return;
    }

    let pad = " ".repeat(indent);
    let var = part_var_name(part);

    for &number in lyrics_map.keys() {
        let suffix = if lyrics_map.len() > 1 {
            format!("Verse{}", index_to_alpha(number as usize))
        } else {
            "Lyrics".to_string()
        };
        let lyrics_var = format!("{var}{suffix}");
        lines.push(format!(
            "{pad}\\new Lyrics \\lyricsto \"{voice_name}\" \\{lyrics_var}"
        ));
    }
}
