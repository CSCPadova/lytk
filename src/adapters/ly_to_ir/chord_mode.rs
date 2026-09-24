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
use super::timeline::{Event, Timeline};

/// Divisions per quarter note used when computing harmony offsets.
/// Must match `FIGURED_BASS_DIVISIONS` / `DEFAULT_DIVISIONS`.
pub(super) const HARMONY_DIVISIONS: i64 = 4;

/// A chordmode entry with its duration, used during distribution.
#[derive(Debug, Clone)]
pub(super) enum HarmonyEntry {
    /// A parsed chord symbol with its (carry-forward-resolved) duration.
    Chord(Harmony, Duration),
    /// A skip/spacer (`s`) with a duration (no harmony produced).
    Skip(Duration),
}

/// Map a LilyPond chord quality suffix (the text after `:`, e.g. `m`, `maj7`,
/// `m7.5-`) to the IR harmony kind string. The vocabulary mirrors
/// `harmony_kind_to_ly` in `ir_to_ly/maps.rs` so chord symbols round-trip.
pub(super) fn ly_quality_to_kind(suffix: &str) -> String {
    let kind = match suffix {
        "" => "major",
        "m" | "min" => "minor",
        "7" => "dominant",
        "maj7" | "maj" | "major7" => "major-seventh",
        "m7" | "min7" => "minor-seventh",
        "dim" => "diminished",
        "dim7" => "diminished-seventh",
        "aug" => "augmented",
        "m7.5-" | "m7-5" | "dim5m7" => "half-diminished",
        "6" => "major-sixth",
        "m6" | "min6" => "minor-sixth",
        "9" => "dominant-ninth",
        "maj9" => "major-ninth",
        "m9" | "min9" => "minor-ninth",
        "11" => "dominant-11th",
        "13" => "dominant-13th",
        "sus2" => "suspended-second",
        "sus4" | "sus" => "suspended-fourth",
        "5" => "power",
        _ => "major",
    };
    kind.to_string()
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

    // Split off the bass (after the first '/'), the quality (after the first ':')
    // from the head (root + octave + duration).
    let (main, bass_str) = match token.split_once('/') {
        Some((m, b)) => (m, Some(b)),
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

    let harmony = Harmony {
        root,
        kind: ly_quality_to_kind(quality.trim()),
        bass,
        degrees: Vec::new(),
        offset: 0,
        function: None,
    };
    Some((harmony, dur))
}

/// Parse a duration substring like `4`, `2.`, `16..` into a `Duration`.
fn parse_duration_text(s: &str) -> Option<Duration> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let val: u32 = digits.parse().ok()?;
    if !matches!(val, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128) {
        return None;
    }
    let dots = s[digits.len()..].chars().take_while(|&c| c == '.').count() as u8;
    let mut dur = Duration::from_lilypond_number(val, 0)?;
    dur.dots = dots;
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
        // Spacer: `s`, `s2`, `s4.` etc.
        if tok == "s"
            || (tok.starts_with('s') && tok[1..].chars().all(|c| c.is_ascii_digit() || c == '.'))
        {
            let dur = parse_duration_text(&tok[1..]).unwrap_or_else(|| last_dur.clone());
            last_dur = dur.clone();
            entries.push(HarmonyEntry::Skip(dur));
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
