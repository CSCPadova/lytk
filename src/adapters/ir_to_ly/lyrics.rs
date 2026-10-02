//! Lyrics emission: one `\lyricmode` line per verse, sung on the voice that
//! carries the lyrics, one token per note it sings. The line says
//! `\set ignoreMelismata = ##t` (as musicxml2ly does): the IR already says
//! which note sings what, so LilyPond's slur, tie and beam rules must not
//! choose again.

use std::collections::BTreeMap;

use crate::ir::articulation::{LyricSyllable, StartStop, SyllabicType};
use crate::ir::duration::{Duration, Frac};
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::note::{Note, VoiceElement};
use crate::ir::Part;

use super::helpers::{index_to_alpha, part_var_name};
use super::maps::{length_to_ly, pitch_to_ly};

/// Every voice of a part that sings, as (staff, voice number), the one with
/// the most syllables first; the staff is `None` in a one-staff part.
fn lyric_voices(part: &Part) -> Vec<(Option<u8>, u8)> {
    let mut count: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    for v in part.measures.iter().flat_map(|m| &m.voices) {
        for e in &v.elements {
            if e.notes().first().is_some_and(|n| !n.lyrics.is_empty()) {
                *count.entry((e.staff().max(1), v.number)).or_default() += 1;
            }
        }
    }
    let mut voices: Vec<((u8, u8), usize)> = count.into_iter().collect();
    voices.sort_by_key(|&(key, n)| (std::cmp::Reverse(n), key));
    voices
        .into_iter()
        .map(|((staff, voice), _)| ((part.staves > 1).then_some(staff), voice))
        .collect()
}

/// The voice a part's lyrics are sung on: its music carries it through
/// every bar (see `bar_voices`). Others that sing get a NullVoice.
pub(super) fn lyric_voice(part: &Part) -> Option<(Option<u8>, u8)> {
    lyric_voices(part).first().copied()
}

/// Whether a part has any lyrics on its notes.
pub(super) fn part_has_lyrics(part: &Part) -> bool {
    lyric_voice(part).is_some()
}

/// For a multi-staff part, the staff whose voice sings the lyrics.
pub(super) fn lyric_staff(part: &Part) -> Option<u8> {
    lyric_voice(part).and_then(|(staff, _)| staff)
}

/// The notes the lyrics' voice sings, in order: in each bar the voice the
/// music writes first (the lyrics' voice when the bar has it, see
/// `bar_voices`), without grace notes and rests.
fn sung_notes(part: &Part, staff: Option<u8>, voice: u8) -> Vec<&Note> {
    part.measures
        .iter()
        .filter_map(|m| {
            super::bar_voices(m, staff, Some(voice))
                .first()
                .map(|&i| &m.voices[i].elements)
        })
        .flatten()
        .filter_map(|e| e.notes().first().filter(|n| !n.is_grace))
        .collect()
}

/// The notes of one voice on a staff, without grace notes and rests: what
/// its NullVoice sings.
fn voice_notes(part: &Part, staff: Option<u8>, voice: u8) -> Vec<&Note> {
    part.measures
        .iter()
        .flat_map(|m| &m.voices)
        .filter(|v| v.number == voice)
        .flat_map(|v| &v.elements)
        .filter(|e| staff.is_none_or(|s| e.staff().max(1) == s))
        .filter_map(|e| e.notes().first().filter(|n| !n.is_grace))
        .collect()
}

/// Each verse (by number): its name and a token per sung note.
type Verses = BTreeMap<u8, (Option<String>, Vec<String>)>;

fn verses(notes: &[&Note]) -> Verses {
    let mut verses = Verses::new();
    for s in notes.iter().flat_map(|n| &n.lyrics) {
        let name = &mut verses.entry(s.number).or_default().0;
        if name.is_none() {
            name.clone_from(&s.name);
        }
    }
    for (&number, (_, tokens)) in verses.iter_mut() {
        tokens.extend(notes.iter().map(|n| {
            n.lyrics
                .iter()
                .find(|s| s.number == number)
                .map_or_else(|| "_".to_string(), syllable_to_ly)
        }));
        while tokens.last().is_some_and(|t| t == "_") {
            tokens.pop();
        }
    }
    verses
}

/// A part's lyric lines: (the staff, the voice `\lyricsto` follows — the
/// part's own or a NullVoice's name and variable —, its verses, each with
/// its variable).
struct Line {
    staff: Option<u8>,
    voice: u8,
    /// `None` for the part's own voice; the NullVoice's name otherwise.
    null_voice: Option<String>,
    verses: Vec<(String, Option<String>, Vec<String>)>,
}

fn lines(part: &Part) -> Vec<Line> {
    let var = part_var_name(part);
    lyric_voices(part)
        .into_iter()
        .enumerate()
        .map(|(k, (staff, voice))| {
            let (notes, null_voice, prefix) = if k == 0 {
                (sung_notes(part, staff, voice), None, var.clone())
            } else {
                let name = format!("{var}NullVoice{}", index_to_alpha(k));
                (voice_notes(part, staff, voice), Some(name.clone()), name)
            };
            let verses = verses(&notes);
            let count = verses.len();
            let verses = verses
                .into_iter()
                .map(|(number, (name, tokens))| {
                    let v = if count > 1 {
                        format!("{prefix}Verse{}", index_to_alpha(number as usize))
                    } else {
                        format!("{prefix}Lyrics")
                    };
                    (v, name, tokens)
                })
                .collect();
            Line {
                staff,
                voice,
                null_voice,
                verses,
            }
        })
        .collect()
}

/// A NullVoice: one voice's rhythm in absolute pitches, a spacer for each
/// bar it is not in (it is not printed or played; lyrics follow it).
fn null_voice_music(part: &Part, staff: Option<u8>, voice: u8, lang: PitchLanguage) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut meter = Frac::from_integer(1);
    for m in &part.measures {
        if let Some(ts) = m.attributes.as_ref().and_then(|a| a.time.as_ref()) {
            meter = ts.beats_fraction();
        }
        let len = match m.content_length() {
            l if l > Frac::from_integer(0) => l,
            _ => meter,
        };
        let mut at = Frac::from_integer(0);
        let elements = m
            .voices
            .iter()
            .filter(|v| v.number == voice)
            .flat_map(|v| &v.elements)
            .filter(|e| staff.is_none_or(|s| e.staff().max(1) == s));
        for e in elements {
            let d = e.metric_duration();
            if d == Frac::from_integer(0) {
                continue;
            }
            let dur = length_to_ly(&Duration::new(d));
            let pitch = |n: &Note| pitch_to_ly(&n.pitch, lang, None, PitchMode::Absolute);
            let tie = |n: &Note| {
                if n.ties.iter().any(|t| t.tie_type == StartStop::Start) {
                    "~"
                } else {
                    ""
                }
            };
            tokens.push(match e {
                VoiceElement::Note(n) => format!("{}{dur}{}", pitch(n), tie(n)),
                VoiceElement::Chord(c) => format!(
                    "<{}>{dur}{}",
                    c.notes.iter().map(pitch).collect::<Vec<_>>().join(" "),
                    c.notes.first().map_or("", tie)
                ),
                VoiceElement::Rest(_) => format!("s{dur}"),
            });
            at += d;
        }
        if at < len {
            tokens.push(format!("s{}", length_to_ly(&Duration::new(len - at))));
        }
        tokens.push("|".to_string());
    }
    tokens
}

/// Emit a part's lyric variables (one per verse) and its NullVoices.
pub(super) fn emit_lyrics_variable(part: &Part, lang: PitchLanguage, lines_out: &mut Vec<String>) {
    for line in lines(part) {
        if let Some(name) = &line.null_voice {
            lines_out.push(format!("{name} = {{"));
            let music = null_voice_music(part, line.staff, line.voice, lang);
            super::helpers::push_wrapped(&music, "  ", lines_out);
            lines_out.push("}".to_string());
            lines_out.push(String::new());
        }
        for (var, name, tokens) in &line.verses {
            lines_out.push(format!("{var} = \\lyricmode {{"));
            lines_out.push("  \\set ignoreMelismata = ##t".to_string());
            if let Some(name) = name {
                let name = super::helpers::escape_ly_string(name);
                lines_out.push(format!("  \\set stanza = \"{name}\""));
            }
            super::helpers::push_wrapped(tokens, "  ", lines_out);
            lines_out.push("}".to_string());
            lines_out.push(String::new());
        }
    }
}

/// Whether anything on this staff sings (the part's voice or a NullVoice).
pub(super) fn staff_sings(part: &Part, staff: Option<u8>) -> bool {
    lyric_voices(part).iter().any(|(s, _)| *s == staff)
}

/// One syllable: `my~a` for words sung on one note, `Hal --`, `jah __`.
pub(super) fn syllable_to_ly(s: &LyricSyllable) -> String {
    let words: Vec<&str> = s.text.split('\u{203F}').collect();
    let mut out = if words.len() > 1 && words.iter().all(|w| escape_lyric_text(w) == *w) {
        words.join("~")
    } else {
        escape_lyric_text(&s.text)
    };
    if matches!(s.syllabic, SyllabicType::Begin | SyllabicType::Middle) {
        out.push_str(" --");
    }
    if s.extend {
        out.push_str(" __");
    }
    out
}

/// Lyric text for `\lyricmode`, quoted when LilyPond would read it as
/// something else: a duration (any digit: `a1`, `2nd`, `dominant-11th`), a
/// hyphen or extender, a space (`_`), words on one note (`~`), a brace, a
/// comment, Scheme, or several words.
pub(super) fn escape_lyric_text(text: &str) -> String {
    let plain = !text.is_empty()
        && !matches!(text, "--" | "__" | "_")
        && !text.contains(|c: char| {
            c.is_ascii_digit() || c.is_whitespace() || "\"\\{}#$%~_|*=<>\u{203F}".contains(c)
        });
    if plain {
        text.to_string()
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// The score block's lyrics for a staff: its NullVoices, and a
/// `\new Lyrics` for each verse sung on it (`voice_name`: the part's own
/// voice).
pub(super) fn emit_lyrics_refs(
    part: &Part,
    staff: Option<u8>,
    voice_name: &str,
    indent: usize,
    lines_out: &mut Vec<String>,
) {
    let pad = " ".repeat(indent);
    for line in lines(part).into_iter().filter(|l| l.staff == staff) {
        let target = match &line.null_voice {
            Some(name) => {
                lines_out.push(format!("{pad}\\new NullVoice = \"{name}\" \\{name}"));
                name.clone()
            }
            None => super::helpers::escape_ly_string(voice_name),
        };
        for (var, _, _) in &line.verses {
            lines_out.push(format!("{pad}\\new Lyrics \\lyricsto \"{target}\" \\{var}"));
        }
    }
}
