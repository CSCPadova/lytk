use tree_sitter::Node;

use crate::ir::articulation::{LyricSyllable, StartStop, SyllabicType};
use crate::ir::note::VoiceElement;
use crate::ir::Part;

use super::consume::extract_string_value;
use super::state::WalkState;

/// Parse a `\lyricmode { ... }` block into a list of `LyricSyllable`s.
/// Lyrics are symbols separated by `--` (hyphen) or `__` (extend).
pub(super) fn parse_lyric_block(state: &WalkState, block: Node) -> Vec<LyricSyllable> {
    let mut syllables: Vec<LyricSyllable> = Vec::new();
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;
    let mut pending_hyphen = false;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let text = state.text(node).to_string();
                // Handle `_` as melisma extender (skip)
                if text == "_" {
                    // Check for `__` (double underscore = extender line)
                    if let Some(next) = children.get(i + 1) {
                        if next.kind() == "symbol" && state.text(*next) == "_" {
                            // `__` = extender line on previous syllable
                            if let Some(last) = syllables.last_mut() {
                                last.extend = true;
                            }
                            i += 2;
                            continue;
                        }
                    }
                    // Single `_` = melisma skip (note gets no syllable)
                    syllables.push(LyricSyllable {
                        text: String::new(),
                        syllabic: SyllabicType::Single,
                        number: 0, // marker: number=0 means "skip"
                        extend: false,
                        elision: false,
                    });
                    i += 1;
                    continue;
                }
                let next_is_hyphen = peek_lyric_hyphen(state, &children, i + 1);
                let syllabic = if pending_hyphen {
                    if next_is_hyphen {
                        SyllabicType::Middle
                    } else {
                        SyllabicType::End
                    }
                } else {
                    if next_is_hyphen {
                        SyllabicType::Begin
                    } else {
                        SyllabicType::Single
                    }
                };
                syllables.push(LyricSyllable {
                    text,
                    syllabic,
                    number: 1,
                    extend: false,
                    elision: false,
                });
                pending_hyphen = false;
            }
            "punctuation" => {
                let t = state.text(node);
                if t == "-" {
                    // Check for "--" (double hyphen = syllable separator)
                    if let Some(next) = children.get(i + 1) {
                        if next.kind() == "punctuation" && state.text(*next) == "-" {
                            pending_hyphen = true;
                            i += 2;
                            continue;
                        }
                    }
                    // Single "-" is also a syllable separator in LilyPond lyrics
                    pending_hyphen = true;
                }
                if t == "_" {
                    // `_` as punctuation = melisma skip
                    syllables.push(LyricSyllable {
                        text: String::new(),
                        syllabic: SyllabicType::Single,
                        number: 0,
                        extend: false,
                        elision: false,
                    });
                }
            }
            "escaped_word" => {
                let text = state.text(node);
                if text == "\\lyricsto" {
                    // Skip \lyricsto "voiceName" — we handle this at a higher level
                    i += 1;
                    if i < children.len() && children[i].kind() == "string" {
                        i += 1; // skip voice name string
                    }
                    continue;
                }
                // Check for variable reference
                let var_name = text.trim_start_matches('\\');
                if let Some(lyrics) = state.lyric_definitions.get(var_name) {
                    syllables.extend(lyrics.clone());
                }
            }
            "expression_block" => {
                // Nested block — recurse
                let inner = parse_lyric_block(state, node);
                syllables.extend(inner);
            }
            _ => {}
        }
        i += 1;
    }
    syllables
}

/// Check if position `start` begins a hyphen separator ("--" or single "-").
pub(super) fn peek_lyric_hyphen(state: &WalkState, children: &[Node], start: usize) -> bool {
    if let Some(a) = children.get(start) {
        if a.kind() == "punctuation" && state.text(*a) == "-" {
            return true;
        }
    }
    false
}

/// Extract the voice name from a `\lyricsto "voiceName"` inside a lyric block.
pub(super) fn extract_lyricsto_voice(state: &WalkState, block: Node) -> Option<String> {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    for i in 0..children.len() {
        let node = children[i];
        if node.kind() == "escaped_word" && state.text(node) == "\\lyricsto" {
            if let Some(next) = children.get(i + 1) {
                if next.kind() == "string" {
                    return Some(extract_string_value(state, *next));
                }
            }
        }
    }
    None
}

/// Attach a list of lyric syllables to the notes of a part, distributing
/// one syllable per note (skipping rests, tied notes, and melisma notes).
/// Syllables with `number == 0` are melisma skips — the note gets no lyric.
pub(super) fn attach_lyrics_to_part(part: &mut Part, syllables: &[LyricSyllable]) {
    let mut syl_idx = 0;
    // Slur depth counter — persists across measures (slurs can span barlines).
    // Used to detect slur melisma: notes 2..N inside a slur don't consume syllables
    // when \autoBeamOff is active (matching LilyPond's slurMelismaBusy rule).
    let mut open_slurs: u32 = 0;

    for measure in &mut part.measures {
        for voice in &mut measure.voices {
            for elem in &mut voice.elements {
                if syl_idx >= syllables.len() {
                    return;
                }
                match elem {
                    VoiceElement::Note(note) => {
                        // Count slur starts/stops on this note to maintain open_slurs depth.
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

                        // Grace notes never consume syllables (but still update slur state).
                        if note.is_grace {
                            open_slurs = open_slurs.saturating_add(starts).saturating_sub(stops);
                            continue;
                        }

                        // Tied continuation notes don't consume syllables.
                        let is_tied_cont = note.ties.iter().any(|t| t.tie_type == StartStop::Stop);

                        // Slur melisma: with \autoBeamOff, notes 2..N of a slur don't consume
                        // syllables (LilyPond's slurMelismaBusy). A note is interior to a slur
                        // when open_slurs > 0 and this note does NOT start a new slur.
                        let in_slur_melisma = note.no_auto_beam
                            && open_slurs > 0
                            && !note.slurs.iter().any(|s| s.slur_type == StartStop::Start);

                        // Update slur depth after determining melisma status.
                        open_slurs = open_slurs.saturating_add(starts).saturating_sub(stops);

                        if is_tied_cont || note.in_melisma || in_slur_melisma {
                            continue;
                        }

                        // Consume next syllable.
                        let syl = &syllables[syl_idx];
                        syl_idx += 1;
                        // number == 0 is an explicit `_` skip: note gets no lyric.
                        if syl.number != 0 {
                            note.lyrics.push(syl.clone());
                        }
                    }
                    VoiceElement::Chord(chord) => {
                        // Chords consume exactly one syllable (attached to the first note).
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
                        }
                        if syl_idx < syllables.len() {
                            let syl = &syllables[syl_idx];
                            syl_idx += 1;
                            if syl.number != 0 {
                                if let Some(first) = chord.notes.first_mut() {
                                    first.lyrics.push(syl.clone());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
