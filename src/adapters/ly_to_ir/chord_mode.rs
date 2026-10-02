//! `\chordmode` parsing → Harmony IR.
//!
//! Mirrors the figured-bass approach (`figured_bass.rs`): parse a chordmode
//! block into a flat stream of entries carrying durations, then place the
//! harmonies one after another on the part's timeline.
//!
//! Parsing is text-based rather than tree-sitter-node based: chord quality
//! tokens (`maj7`, `m7.5-`, `sus4`) and bass syntax (`/e`, `/+e`) plus optional
//! `_"..."` / `^"..."` markup are far simpler to handle from the raw token text
//! than by reassembling individual grammar nodes.

use tree_sitter::Node;

use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::{ChordPitch, Harmony};
use crate::ir::language::{parse_pitch_name, PitchLanguage};

use super::state::WalkState;
use crate::ir::timeline::{Event, Timeline};

/// Divisions per quarter note used when computing harmony offsets.
/// Must match `FIGURED_BASS_DIVISIONS` / `DEFAULT_DIVISIONS`.
pub(super) const HARMONY_DIVISIONS: i64 = crate::ir::timeline::OFFSET_DIVISIONS;

/// A chordmode entry with its duration, used during distribution.
#[derive(Debug, Clone)]
pub(super) enum HarmonyEntry {
    /// A parsed chord symbol with its (carry-forward-resolved) duration.
    Chord(Harmony, Duration),
    /// A skip/spacer (`s`) with a duration (no harmony produced).
    Skip(Duration),
}

/// A LilyPond chord's modifiers (the text after `:`, e.g. `m`, `maj7`,
/// `m7.5-`, `7.9-`) as a MusicXML kind and degrees; `other` when they can't
/// be read.
pub(super) fn ly_quality(modifiers: &str) -> (String, Vec<crate::ir::harmony::ChordDegree>) {
    match crate::ir::harmony::ly_chord_steps(modifiers) {
        Some(steps) => {
            let (kind, degrees) = crate::ir::harmony::kind_and_degrees(&steps);
            (kind.to_string(), degrees)
        }
        None => ("other".to_string(), Vec::new()),
    }
}

/// Parse a pitch-name string (e.g. `c`, `cis`, `bes`, German `h`) into a
/// `ChordPitch`. Returns `None` if the name is not a valid pitch in `lang`.
fn parse_chord_pitch(name: &str, lang: PitchLanguage) -> Option<ChordPitch> {
    let (step, alter) = parse_pitch_name(name, lang)?;
    let alter_f = *alter.numer() as f64 / *alter.denom() as f64;
    Some(ChordPitch {
        step: step.name().to_string(),
        alter: alter_f,
    })
}

/// Strip `_"..."` / `^"..."` markup segments from a chordmode token so the
/// chord core can be tokenized. Handles only simple (non-nested) quoted markup,
/// which is what chordmode uses in practice.
fn strip_markup(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut chars = token.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '_' || c == '^') && chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            for d in chars.by_ref() {
                if d == '"' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a single chordmode token (e.g. `f4:maj7/e`) into a `Harmony` plus the
/// LilyPond duration text (digits + dots) that was embedded, if any.
fn parse_chord_token(token: &str, lang: PitchLanguage) -> Option<(Harmony, Option<Duration>)> {
    let token = strip_markup(token);
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    // Split off the bass (after a '/' before a note name: `1*3/4` is a
    // length), the quality (after the first ':') from the head (root +
    // octave + duration).
    let bass_at = token.char_indices().find(|&(i, c)| {
        c == '/' && token[i + 1..].starts_with(|n: char| n.is_ascii_alphabetic() || n == '+')
    });
    let (main, bass_str) = match bass_at {
        Some((i, _)) => (&token[..i], Some(&token[i + 1..])),
        None => (token, None),
    };
    let (head, quality) = match main.split_once(':') {
        Some((h, q)) => (h, q),
        None => (main, ""),
    };

    // Head = pitch-name letters + optional octave marks + optional duration.
    let bytes = head.as_bytes();
    let mut p = 0;
    while p < bytes.len() && bytes[p].is_ascii_alphabetic() {
        p += 1;
    }
    if p == 0 {
        return None;
    }
    let root = parse_chord_pitch(&head[..p], lang)?;

    // Skip octave marks (irrelevant to the chord symbol).
    while p < bytes.len() && (bytes[p] == b'\'' || bytes[p] == b',') {
        p += 1;
    }

    // Remaining = duration digits + dots.
    let dur = parse_duration_text(&head[p..]);

    // Bass: strip a leading '+' (added-bass vs inversion — same IR field).
    let bass = bass_str.and_then(|b| {
        let b = b.trim_start_matches('+');
        let blen = b
            .as_bytes()
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        parse_chord_pitch(&b[..blen], lang)
    });

    let (kind, degrees) = ly_quality(quality.trim());
    let harmony = Harmony {
        root,
        kind,
        bass,
        degrees,
        offset: 0,
        function: None,
    };
    Some((harmony, dur))
}

/// Parse a duration substring like `4`, `2.`, `16..`, `1*3/4` into a `Duration`.
fn parse_duration_text(s: &str) -> Option<Duration> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let val: u32 = digits.parse().ok()?;
    if !matches!(val, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128) {
        return None;
    }
    let rest = &s[digits.len()..];
    let dots = rest.chars().take_while(|&c| c == '.').count();
    let mut dur = Duration::from_lilypond_number(val, 0)?;
    dur.dots = dots as u8;
    // A scale factor `*N` or `*N/M` multiplies the length.
    if let Some(factor) = rest[dots..].strip_prefix('*') {
        let (n, d) = factor.split_once('/').unwrap_or((factor, "1"));
        let (n, d): (i64, i64) = (n.parse().ok()?, d.parse().ok()?);
        if n <= 0 || d <= 0 {
            return None;
        }
        dur = Duration::new(dur.actual_duration() * Frac::new(n, d));
    }
    Some(dur)
}

/// Parse a `\chordmode { ... }` block into a flat stream of harmony entries.
pub(super) fn parse_chordmode_block(state: &WalkState, block: Node) -> Vec<HarmonyEntry> {
    let lang = state.language;
    let text = state.text(block);
    // Strip the surrounding braces.
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');

    let mut entries = Vec::new();
    let mut last_dur = Duration::quarter();

    for raw in inner.split_whitespace() {
        let tok = raw.trim();
        if tok.is_empty() || tok == "|" {
            continue;
        }
        // Skip commands and markup-only tokens.
        if tok.starts_with('\\') {
            continue;
        }
        // Spacer: `s`, `s2`, `s4.`, `s1*3/4` etc.
        if tok == "s"
            || (tok.starts_with('s')
                && tok[1..]
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | '*' | '/')))
        {
            let dur = parse_duration_text(&tok[1..]).unwrap_or_else(|| last_dur.clone());
            last_dur = dur.clone();
            entries.push(HarmonyEntry::Skip(dur));
            continue;
        }

        // A rest is a no-chord (ChordNames prints N.C.).
        if let Some(d) = tok.strip_prefix(['r', 'R']).filter(|d| {
            d.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | '*' | '/'))
        }) {
            let dur = parse_duration_text(d).unwrap_or_else(|| last_dur.clone());
            last_dur = dur.clone();
            entries.push(HarmonyEntry::Chord(no_chord(), dur));
            continue;
        }
        if let Some((harmony, dur)) = parse_chord_token(tok, lang) {
            let d = dur.unwrap_or_else(|| last_dur.clone());
            last_dur = d.clone();
            entries.push(HarmonyEntry::Chord(harmony, d));
        }
    }

    entries
}

/// The no-chord of a chord-mode rest.
fn no_chord() -> Harmony {
    Harmony {
        root: ChordPitch {
            step: "C".to_string(),
            alter: 0.0,
        },
        kind: "none".to_string(),
        bass: None,
        degrees: Vec::new(),
        offset: 0,
        function: None,
    }
}

/// Place a flat stream of harmony entries one after another from `start`.
pub(super) fn place_harmonies(tl: &mut Timeline, start: Frac, entries: &[HarmonyEntry]) {
    let mut at = start;
    for entry in entries {
        match entry {
            HarmonyEntry::Chord(h, dur) => {
                tl.add(at, Event::Harmony(h.clone()));
                at += dur.actual_duration();
            }
            HarmonyEntry::Skip(dur) => at += dur.actual_duration(),
        }
    }
}
