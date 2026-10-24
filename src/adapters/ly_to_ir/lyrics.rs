//! Lyrics: LilyPond's lyric words (`lily/lexer.ll`, lyric mode) and the
//! notes they are sung on (`\lyricsto`, `\addlyrics`), by LilyPond's melisma
//! rules (`melismaBusyProperties`).

use std::collections::HashMap;

use tree_sitter::Node;

use crate::ir::articulation::{BeamValue, LyricSyllable, StartStop, SyllabicType};
use crate::ir::duration::Frac;
use crate::ir::note::VoiceElement;
use crate::ir::score::Score;

use super::consume::{extract_string_value, skip_markup};
use super::state::WalkState;

/// What a lyric line says, in order.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum LyricToken {
    /// The next note's syllable, and its length when written (`la4`).
    Syllable(LyricSyllable, Option<Frac>),
    /// A note sung without a syllable (`_`, `\skip 4`).
    Skip(Option<Frac>),
    /// `\set ignoreMelismata`: every note takes a token (`true`), or a
    /// melisma's notes take none.
    IgnoreMelismata(bool),
    /// `\set stanza = "1."`: the verse's name.
    Stanza(String),
}

/// What a lyric line is sung on.
#[derive(Debug, Clone)]
pub(super) enum LyricTarget {
    /// `\lyricsto "name"`: that Voice, wherever it is.
    Voice(String),
    /// `\addlyrics`: the voice of the music before it (a voice tag).
    Tag(u32),
    /// A Lyrics context without `\lyricsto`: its syllables at the times
    /// their lengths give from `start` (whole notes from the movement's
    /// start), on the notes of the voice before it.
    Timed { tag: u32, start: Frac },
}

/// One lyric line and what it follows, in source order.
#[derive(Debug, Clone)]
pub(super) struct LyricJob {
    pub(super) target: LyricTarget,
    pub(super) tokens: Vec<LyricToken>,
}

/// Most tokens one `\repeat unfold` may make.
const MAX_UNFOLDED: usize = 100_000;

/// The tokens of a lyric block (`{ … }`).
pub(super) fn parse_lyric_block(state: &WalkState, block: Node) -> Vec<LyricToken> {
    // `go. on` parses as a property (`Staff.x`): its tokens are words.
    fn flatten<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
        if node.kind() == "property_expression" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                flatten(child, out);
            }
        } else {
            out.push(node);
        }
    }
    let mut cursor = block.walk();
    let mut children: Vec<Node> = Vec::new();
    for child in block.children(&mut cursor) {
        flatten(child, &mut children);
    }
    let mut out = Vec::new();
    let mut hyphen = false;
    let mut i = 0;
    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "{" | "}" | "comment" | "embedded_scheme" => i += 1,
            // A quoted syllable is taken as written.
            "string" => {
                syllable(
                    &mut out,
                    &mut hyphen,
                    extract_string_value(state, node),
                    false,
                );
                i += 1;
            }
            "expression_block" => {
                out.extend(parse_lyric_block(state, node));
                i += 1;
            }
            "assignment_lhs" => i = skip_assignment(state, &children, i).0,
            "escaped_word" => i = command(state, &children, i, &mut out),
            _ => {
                // A word: the tokens written together (`don't` is three).
                let (start, mut end) = (node.start_byte(), node.end_byte());
                i += 1;
                while let Some(n) = children
                    .get(i)
                    .filter(|n| n.start_byte() == end && is_word_part(n))
                {
                    end = n.end_byte();
                    i += 1;
                }
                word(&mut out, &mut hyphen, &state.source[start..end]);
            }
        }
    }
    out
}

fn is_word_part(n: &Node) -> bool {
    !matches!(
        n.kind(),
        "{" | "}"
            | "comment"
            | "string"
            | "escaped_word"
            | "expression_block"
            | "assignment_lhs"
            | "embedded_scheme"
    )
}

/// One unquoted lyric word (LilyPond's lyric mode): `--` joins the
/// syllables around it, `__` extends the one before, `_` alone sings a note
/// without a syllable; in a word `~` joins words sung on one note and `_` is
/// a space. A trailing duration (`la4`) is no part of the text.
fn word(out: &mut Vec<LyricToken>, hyphen: &mut bool, raw: &str) {
    match raw {
        "--" => {
            if let Some(s) = last_syllable(out) {
                s.syllabic = match s.syllabic {
                    SyllabicType::End | SyllabicType::Middle => SyllabicType::Middle,
                    _ => SyllabicType::Begin,
                };
            }
            *hyphen = true;
        }
        "__" => {
            if let Some(s) = last_syllable(out) {
                s.extend = true;
            }
        }
        "_" => out.push(LyricToken::Skip(None)),
        _ => {
            let (text, len) = split_duration(raw);
            syllable(out, hyphen, text.to_string(), true);
            if let Some(LyricToken::Syllable(_, l)) = out.last_mut() {
                *l = len;
            }
        }
    }
}

fn syllable(out: &mut Vec<LyricToken>, hyphen: &mut bool, text: String, lex: bool) {
    let (text, elision) = if lex {
        let words: Vec<String> = text.split('~').map(|w| w.replace('_', " ")).collect();
        (words.join("\u{203F}"), words.len() > 1)
    } else {
        let elided = text.contains('\u{203F}');
        (text, elided)
    };
    let syllabic = if std::mem::take(hyphen) {
        SyllabicType::End
    } else {
        SyllabicType::Single
    };
    out.push(LyricToken::Syllable(
        LyricSyllable {
            text,
            syllabic,
            number: 1,
            extend: false,
            elision,
            name: None,
        },
        None,
    ));
}

fn last_syllable(out: &mut [LyricToken]) -> Option<&mut LyricSyllable> {
    out.iter_mut().rev().find_map(|t| match t {
        LyricToken::Syllable(s, _) => Some(s),
        _ => None,
    })
}

/// `la4.` → (`la`, 3/8): digits and dots ending a word that has more are
/// its length.
fn split_duration(raw: &str) -> (&str, Option<Frac>) {
    let body = raw.trim_end_matches('.');
    let digits = body.len() - body.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits == body.len() {
        return (raw, None);
    }
    let text = &body[..body.len() - digits];
    (text, length(&raw[text.len()..]))
}

/// A written length, `4`, `2.`, `16..`: `None` if it is none.
fn length(s: &str) -> Option<Frac> {
    let dots = s.len() - s.trim_end_matches('.').len();
    let n: i64 = s.trim_end_matches('.').parse().ok()?;
    if !(n as u64).is_power_of_two() || n > 128 || dots > 4 {
        return None;
    }
    let base = Frac::new(1, n);
    Some(base * (Frac::from_integer(2) - Frac::new(1, 1 << dots)))
}

/// `\set prop = value` and `\override …` from the `assignment_lhs` at
/// `children[i]`: the index past the value, the property's last name and the
/// value's text.
fn skip_assignment(state: &WalkState, children: &[Node], mut i: usize) -> (usize, String, String) {
    let prop = children.get(i).map_or("", |n| state.text(*n));
    let prop = prop.rsplit('.').next().unwrap_or("").trim().to_string();
    i += 1;
    let mut value = String::new();
    if children.get(i).is_some_and(|n| state.text(*n) == "=") {
        i += 1;
        if children.get(i).is_some_and(|n| state.text(*n) == "-") {
            i += 1;
        }
        if let Some(v) = children.get(i) {
            value = if v.kind() == "string" {
                extract_string_value(state, *v)
            } else {
                state.text(*v).to_string()
            };
            i += 1;
        }
    }
    (i, prop, value)
}

/// A command in a lyric block, at `children[i]`; the next index.
fn command(state: &WalkState, children: &[Node], i: usize, out: &mut Vec<LyricToken>) -> usize {
    let name = state.text(children[i]);
    let mut i = i + 1;
    match name {
        // `\skip 4`: a note without a syllable.
        "\\skip" => {
            let mut len = None;
            if let Some(n) = children.get(i).filter(|n| n.kind() == "unsigned_integer") {
                let (start, mut end) = (n.start_byte(), n.end_byte());
                i += 1;
                while let Some(d) = children
                    .get(i)
                    .filter(|d| d.start_byte() == end && state.text(**d) == ".")
                {
                    end = d.end_byte();
                    i += 1;
                }
                len = length(&state.source[start..end]);
            }
            out.push(LyricToken::Skip(len));
        }
        // `\repeat unfold N { … }` sings the block N times; other repeats
        // once.
        "\\repeat" => {
            let unfold = children.get(i).is_some_and(|n| state.text(*n) == "unfold");
            i += 1;
            let times = children
                .get(i)
                .and_then(|n| state.text(*n).parse::<usize>().ok())
                .unwrap_or(1);
            i += 1;
            if let Some(block) = children.get(i).filter(|n| n.kind() == "expression_block") {
                let body = parse_lyric_block(state, *block);
                let times = if unfold {
                    times.min(MAX_UNFOLDED / body.len().max(1))
                } else {
                    1
                };
                for _ in 0..times {
                    out.extend(body.iter().cloned());
                }
                i += 1;
            }
        }
        "\\set" | "\\unset" => {
            if children
                .get(i)
                .is_some_and(|n| n.kind() == "assignment_lhs")
            {
                let (next, prop, value) = skip_assignment(state, children, i);
                i = next;
                let set = name == "\\set";
                match prop.as_str() {
                    "stanza" if set => out.push(LyricToken::Stanza(value)),
                    "ignoreMelismata" => {
                        out.push(LyricToken::IgnoreMelismata(set && value.contains("#t")))
                    }
                    _ => {}
                }
            }
        }
        "\\lyricsto" => {
            if lyricsto_name(state, children, i).is_some() {
                i += 1;
            }
        }
        "\\markup" | "\\markuplist" => i = skip_markup(state, children, i),
        word => {
            if let Some(tokens) = state.lyric_definitions.get(word.trim_start_matches('\\')) {
                out.extend(tokens.iter().cloned());
            }
        }
    }
    i
}

/// `MUSIC \addlyrics LYRICS`: a lyric line sung on the voice of the last
/// note written. Whether `node` was lyrics (a block or a lyric variable).
pub(super) fn add_lyrics(state: &mut WalkState, node: Node) -> bool {
    let tokens = match node.kind() {
        "expression_block" => parse_lyric_block(state, node),
        "escaped_word" => {
            match state
                .lyric_definitions
                .get(state.text(node).trim_start_matches('\\'))
            {
                Some(tokens) => tokens.clone(),
                None => return false,
            }
        }
        _ => return false,
    };
    state.pending_lyrics.push(LyricJob {
        target: LyricTarget::Tag(state.last_tag),
        tokens,
    });
    true
}

/// The voice `\lyricsto` names at `children[i]`: a string or a bare word.
pub(super) fn lyricsto_name(state: &WalkState, children: &[Node], i: usize) -> Option<String> {
    let n = children.get(i)?;
    match n.kind() {
        "string" => Some(extract_string_value(state, *n)),
        "symbol" => Some(state.text(*n).to_string()),
        _ => None,
    }
}

/// The voice a `\lyricsto` inside a lyric block names.
pub(super) fn extract_lyricsto_voice(state: &WalkState, block: Node) -> Option<String> {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let at = children
        .iter()
        .position(|n| n.kind() == "escaped_word" && state.text(*n) == "\\lyricsto")?;
    lyricsto_name(state, &children, at + 1)
}

/// Sing every lyric line on its voice, verses numbered per voice in source
/// order; then forget the voice tags. `shadows`: the notes of each
/// NullVoice (by tag), which are not in the score.
pub(super) fn attach_lyrics(
    score: &mut Score,
    jobs: Vec<LyricJob>,
    voices: &HashMap<String, u32>,
    shadows: &HashMap<u32, Vec<(Frac, VoiceElement)>>,
) {
    let mut verses: HashMap<u32, u8> = HashMap::new();
    for job in jobs {
        let tag = match &job.target {
            LyricTarget::Voice(name) => voices.get(name).copied(),
            LyricTarget::Tag(t) | LyricTarget::Timed { tag: t, .. } => Some(*t),
        };
        let Some(tag) = tag.filter(|&t| t != 0) else {
            continue;
        };
        let verse = verses.entry(tag).or_insert(0);
        *verse = verse.saturating_add(1);
        let verse = *verse;
        match (job.target, shadows.get(&tag)) {
            (LyricTarget::Timed { start, .. }, _) => {
                sing_timed(score, tag, verse, start, &job.tokens)
            }
            (_, Some(shadow)) => sing_shadow(score, shadow, verse, &job.tokens),
            _ => sing(score, tag, verse, &job.tokens),
        }
    }
    for part in score.parts_mut() {
        for e in part
            .measures
            .iter_mut()
            .flat_map(|m| &mut m.voices)
            .flat_map(|v| &mut v.elements)
        {
            for n in e.notes_mut() {
                n.lyric_voice = 0;
            }
        }
    }
}

/// Which of these elements (a voice's, in time order) sing which syllable
/// of a line, by LilyPond's rules: a note continuing a tie, one after a
/// slur's start up to its end, one inside a manual beam under
/// `\autoBeamOff` and one inside `\melisma` take none, unless the line
/// ignores melismata; grace notes and rests take none.
fn assign(
    elements: &[&VoiceElement],
    verse: u8,
    tokens: &[LyricToken],
) -> Vec<(usize, LyricSyllable)> {
    let mut out = Vec::new();
    let mut tokens = tokens.iter().peekable();
    let (mut ignore, mut stanza) = (false, None::<String>);
    let (mut slurs, mut beam, mut tie) = (0usize, false, false);
    for (k, e) in elements.iter().enumerate() {
        let Some(lead) = e.notes().first().filter(|n| !n.is_grace) else {
            continue;
        };
        while let Some(t) =
            tokens.next_if(|t| matches!(t, LyricToken::IgnoreMelismata(_) | LyricToken::Stanza(_)))
        {
            match t {
                LyricToken::IgnoreMelismata(on) => ignore = *on,
                LyricToken::Stanza(s) => stanza = Some(s.clone()),
                _ => {}
            }
        }
        let tied = std::mem::replace(
            &mut tie,
            e.notes()
                .iter()
                .any(|n| n.ties.iter().any(|t| t.tie_type == StartStop::Start)),
        ) || e
            .notes()
            .iter()
            .any(|n| n.ties.iter().any(|t| t.tie_type == StartStop::Stop));
        let busy = tied || slurs > 0 || (lead.no_auto_beam && beam) || lead.in_melisma;
        let count = |k: StartStop| lead.slurs.iter().filter(|s| s.slur_type == k).count();
        slurs = (slurs + count(StartStop::Start)).saturating_sub(count(StartStop::Stop));
        match lead
            .beams
            .iter()
            .find(|b| b.number == 1)
            .map(|b| b.beam_type)
        {
            Some(BeamValue::Begin) => beam = true,
            Some(BeamValue::End) => beam = false,
            _ => {}
        }
        if busy && !ignore {
            continue;
        }
        match tokens.next() {
            Some(LyricToken::Syllable(s, _)) => {
                let mut s = s.clone();
                s.number = verse;
                s.name = stanza.clone();
                out.push((k, s));
            }
            Some(_) => {}
            None => break,
        }
    }
    out
}

/// Where each element of the score sits: (start in whole notes from the
/// movement's start, part, bar, voice, element), bars laid end to end.
fn onsets(score: &Score) -> Vec<(Frac, usize, usize, usize, usize)> {
    let mut out = Vec::new();
    for (pi, part) in score.parts().iter().enumerate() {
        let mut bar = Frac::from_integer(0);
        let mut meter = Frac::from_integer(1);
        for (mi, m) in part.measures.iter().enumerate() {
            if let Some(ts) = m.attributes.as_ref().and_then(|a| a.time.as_ref()) {
                meter = ts.beats_fraction();
            }
            for (vi, v) in m.voices.iter().enumerate() {
                let mut on = bar;
                for (ei, e) in v.elements.iter().enumerate() {
                    out.push((on, pi, mi, vi, ei));
                    on += e.metric_duration();
                }
            }
            bar += match m.content_length() {
                len if len > Frac::from_integer(0) => len,
                _ => meter,
            };
        }
    }
    out
}

/// One line on the notes of voice `tag`, in time order.
fn sing(score: &mut Score, tag: u32, verse: u8, tokens: &[LyricToken]) {
    let mut order: Vec<(usize, usize, Frac, usize, usize)> = onsets(score)
        .into_iter()
        .map(|(on, pi, mi, vi, ei)| (pi, mi, on, vi, ei))
        .collect();
    let parts = score.parts();
    order.retain(|&(pi, mi, _, vi, ei)| {
        parts[pi].measures[mi].voices[vi].elements[ei]
            .notes()
            .first()
            .is_some_and(|n| n.lyric_voice == tag)
    });
    order.sort();
    let elements: Vec<&VoiceElement> = order
        .iter()
        .map(|&(pi, mi, _, vi, ei)| &parts[pi].measures[mi].voices[vi].elements[ei])
        .collect();
    let sung = assign(&elements, verse, tokens);
    let mut parts = score.parts_mut();
    for (k, s) in sung {
        let (pi, mi, _, vi, ei) = order[k];
        parts[pi].measures[mi].voices[vi].elements[ei].notes_mut()[0]
            .lyrics
            .push(s);
    }
}

/// A line sung on a NullVoice, whose notes are not in the score: each
/// syllable on the score's note that starts with its note, the one with the
/// same pitch if there is one.
fn sing_shadow(
    score: &mut Score,
    shadow: &[(Frac, VoiceElement)],
    verse: u8,
    tokens: &[LyricToken],
) {
    let elements: Vec<&VoiceElement> = shadow.iter().map(|(_, e)| e).collect();
    let sung = assign(&elements, verse, tokens);
    let at = onsets(score);
    let mut parts = score.parts_mut();
    for (k, s) in sung {
        let (on, e) = &shadow[k];
        let pitches: Vec<_> = e.notes().iter().map(|n| n.pitch.midi_number()).collect();
        let starting: Vec<_> = at
            .iter()
            .filter(|c| c.0 == *on)
            .filter(|&&(_, pi, mi, vi, ei)| {
                parts[pi].measures[mi].voices[vi].elements[ei]
                    .notes()
                    .first()
                    .is_some_and(|n| !n.is_grace)
            })
            .collect();
        let same_pitch = starting.iter().find(|&&&(_, pi, mi, vi, ei)| {
            parts[pi].measures[mi].voices[vi].elements[ei]
                .notes()
                .iter()
                .any(|n| pitches.contains(&n.pitch.midi_number()))
        });
        if let Some(&&(_, pi, mi, vi, ei)) = same_pitch.or(starting.first()) {
            parts[pi].measures[mi].voices[vi].elements[ei].notes_mut()[0]
                .lyrics
                .push(s);
        }
    }
}

/// A line without `\lyricsto`: each syllable on the voice's note that
/// starts where the line's lengths put it (a length carries on, as in
/// music; a quarter to begin with).
fn sing_timed(score: &mut Score, tag: u32, verse: u8, start: Frac, tokens: &[LyricToken]) {
    let mut at: HashMap<Frac, (usize, usize, usize, usize)> = HashMap::new();
    {
        let parts = score.parts();
        for (on, pi, mi, vi, ei) in onsets(score) {
            let e = &parts[pi].measures[mi].voices[vi].elements[ei];
            if e.notes()
                .first()
                .is_some_and(|n| n.lyric_voice == tag && !n.is_grace)
            {
                at.entry(on).or_insert((pi, mi, vi, ei));
            }
        }
    }
    let mut parts = score.parts_mut();
    let (mut t, mut last) = (start, Frac::new(1, 4));
    let mut stanza = None;
    for token in tokens {
        let len = match token {
            LyricToken::Syllable(s, len) => {
                if let Some(&(pi, mi, vi, ei)) = at.get(&t) {
                    let mut s = s.clone();
                    s.number = verse;
                    s.name = stanza.clone();
                    parts[pi].measures[mi].voices[vi].elements[ei].notes_mut()[0]
                        .lyrics
                        .push(s);
                }
                *len
            }
            LyricToken::Skip(len) => *len,
            LyricToken::Stanza(s) => {
                stanza = Some(s.clone());
                continue;
            }
            LyricToken::IgnoreMelismata(_) => continue,
        };
        last = len.unwrap_or(last);
        t += last;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(raw: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        let mut hyphen = false;
        for w in raw {
            word(&mut out, &mut hyphen, w);
        }
        out.iter()
            .map(|t| match t {
                LyricToken::Syllable(s, _) => format!(
                    "{}{}{}",
                    s.text,
                    match s.syllabic {
                        SyllabicType::Begin => "-",
                        SyllabicType::Middle => "=",
                        SyllabicType::End => "+",
                        SyllabicType::Single => "",
                    },
                    if s.extend { "_" } else { "" }
                ),
                _ => "skip".to_string(),
            })
            .collect()
    }

    #[test]
    fn words_follow_lilypond() {
        assert_eq!(
            texts(&["Hal", "--", "le", "--", "lu", "__", "_", "jah4."]),
            ["Hal-", "le=", "lu+_", "skip", "jah"]
        );
        assert_eq!(
            texts(&["a~b", "a_b", "don't", "-", "1."]),
            ["a‿b", "a b", "don't", "-", "1."]
        );
    }
}
