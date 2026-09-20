//! ABC notation → IR adapter (EET1).
//!
//! Parses a single ABC tune into a Layer-1 [`MusicDocument`]. Supports the
//! common subset: the `X/T/C/M/L/K/Q` header fields, notes (accidentals,
//! octave marks, fractional durations), rests, bar lines and repeats, chords
//! `[...]`, and ties `-`. Unknown decorations, chord symbols (`"..."`),
//! grace notes (`{...}`) and inline fields (`[K:...]`) are skipped gracefully
//! rather than erroring.
//!
//! Pitch convention (ABC standard): uppercase `C..B` are the middle-C octave
//! (MIDI 60–71), lowercase `c..b` the octave above; `,` lowers and `'` raises
//! by an octave.

use std::path::Path;

use crate::ir::annotation::Annotation;
use crate::ir::direction::{Barline, BarlineType, RepeatDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::{KeyMode, KeySignature, TimeSignature};
use crate::ir::music::{ContextType, Music, MusicDocument};
use crate::ir::pitch::{Alter, Pitch, PitchStep};
use crate::ir::score::ScoreMetadata;

use super::{AdapterError, Result, ToIrAdapter, ToMusicAdapter};

/// Adapter that reads ABC notation.
#[derive(Default)]
pub struct AbcToIrAdapter;

impl AbcToIrAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl ToMusicAdapter for AbcToIrAdapter {
    fn convert_file_to_music(&self, path: &Path) -> Result<MusicDocument> {
        let text = std::fs::read_to_string(path)?;
        self.convert_str_to_music(&text)
    }

    fn convert_str_to_music(&self, text: &str) -> Result<MusicDocument> {
        parse_tune(text)
    }
}

impl ToIrAdapter for AbcToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<crate::ir::Score> {
        let doc = self.convert_file_to_music(path)?;
        Ok(crate::ir::lower::lower_to_score(&doc))
    }

    fn convert_str(&self, text: &str) -> Result<crate::ir::Score> {
        let doc = self.convert_str_to_music(text)?;
        Ok(crate::ir::lower::lower_to_score(&doc))
    }
}

// ---------------------------------------------------------------------------
// Tune parsing
// ---------------------------------------------------------------------------

struct TuneState {
    unit_length: Frac, // L: default note length, in whole notes
    meter: Option<Frac>,
}

/// One ABC voice (`V:` field) — an independent music stream that plays
/// simultaneously with the others. Maps to one Staff (→ Part on lowering).
struct VoiceStream {
    id: String,
    name: Option<String>,
    events: Vec<Music>,
}

/// Find the voice with `id`, creating it (recording `name` if given) when absent.
/// Returns its index in `voices`.
fn ensure_voice(voices: &mut Vec<VoiceStream>, id: &str, name: Option<String>) -> usize {
    if let Some(i) = voices.iter().position(|v| v.id == id) {
        // A later declaration may supply the name (e.g. header `V:1 name=…`).
        if name.is_some() && voices[i].name.is_none() {
            voices[i].name = name;
        }
        return i;
    }
    voices.push(VoiceStream {
        id: id.to_string(),
        name,
        events: Vec::new(),
    });
    voices.len() - 1
}

/// Parse a `V:` field value into `(id, name)`. The id is the first whitespace-
/// delimited token; `name="…"`/`nm="…"` (quoted or bare) supplies the name.
fn parse_voice_header(value: &str) -> (String, Option<String>) {
    let v = value.trim();
    let id = v.split_whitespace().next().unwrap_or("1").to_string();
    let name = extract_param(v, "name").or_else(|| extract_param(v, "nm"));
    (id, name)
}

/// Pull `key=value` (or `key="quoted value"`) from an ABC field parameter list.
fn extract_param(s: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=");
    let start = s.find(&pat)? + pat.len();
    let rest = &s[start..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        Some(rest.split_whitespace().next().unwrap_or("").to_string())
    }
}

/// If `line` begins with an inline `[V:id]` voice marker, return `(Some(id), rest)`.
fn peel_inline_voice(line: &str) -> (Option<String>, &str) {
    let t = line.trim_start();
    if let Some(after) = t.strip_prefix("[V:") {
        if let Some(end) = after.find(']') {
            let id = after[..end].split_whitespace().next().unwrap_or("1");
            return (Some(id.to_string()), &after[end + 1..]);
        }
    }
    (None, line)
}

fn parse_tune(text: &str) -> Result<MusicDocument> {
    let mut metadata = ScoreMetadata::default();
    let mut state = TuneState {
        unit_length: Frac::new(1, 8),
        meter: None,
    };
    // Shared header signatures (M:/K:), prepended to every voice so each lowers
    // to a Part with the right attributes.
    let mut header_events: Vec<Music> = Vec::new();
    let mut voices: Vec<VoiceStream> = Vec::new();
    let mut current: usize = 0;
    let mut in_body = false;
    let mut explicit_unit_length = false;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.starts_with('%') {
            continue;
        }

        // Header fields are `X:value` with a single uppercase letter key.
        if !in_body && is_header_line(line) {
            let (key, value) = split_field(line);
            match key {
                'T' => {
                    if metadata.title.is_none() {
                        metadata.title = Some(value.to_string());
                    }
                }
                'C' => metadata.composer = Some(value.to_string()),
                'M' => {
                    let m = parse_meter(value);
                    state.meter = m.map(|(n, d)| Frac::new(n as i64, d as i64));
                    if let Some((n, d)) = m {
                        header_events.push(Music::TimeSignature(TimeSignature {
                            beats: n.to_string(),
                            beat_type: d,
                            symbol: meter_symbol(value),
                        }));
                    }
                }
                'L' => {
                    if let Some(f) = parse_fraction(value) {
                        state.unit_length = f;
                        explicit_unit_length = true;
                    }
                }
                'V' => {
                    // Voice declaration in the header (ABC 2.1 §4.1): set up the
                    // voice (and its name) ahead of the body.
                    let (id, name) = parse_voice_header(value);
                    ensure_voice(&mut voices, &id, name);
                }
                'K' => {
                    if let Some(key_sig) = parse_key(value) {
                        header_events.push(Music::KeySignature(key_sig));
                    }
                    // K: ends the header; the rest is the tune body.
                    in_body = true;
                    // Default unit length depends on the meter when L: is absent.
                    if !explicit_unit_length {
                        state.unit_length = default_unit_length(state.meter);
                    }
                }
                'X' => {
                    metadata.extra.insert("X".to_string(), value.to_string());
                }
                other => {
                    metadata.extra.insert(other.to_string(), value.to_string());
                }
            }
            continue;
        }

        if in_body {
            // A `V:` info field on its own line switches the active voice.
            if is_header_line(line) {
                let (key, value) = split_field(line);
                if key == 'V' {
                    let (id, name) = parse_voice_header(value);
                    current = ensure_voice(&mut voices, &id, name);
                    continue;
                }
                // Other inline `K:`/`M:`/`L:` fields apply to the current voice.
                if voices.is_empty() {
                    current = ensure_voice(&mut voices, "1", None);
                }
                apply_inline_field(key, value, &mut state, &mut voices[current].events);
                continue;
            }
            // A line may begin with an inline `[V:id]` marker before its music.
            let (switch, rest) = peel_inline_voice(line);
            if let Some(id) = switch {
                current = ensure_voice(&mut voices, &id, None);
            }
            if voices.is_empty() {
                current = ensure_voice(&mut voices, "1", None);
            }
            parse_body_line(rest, &state, &mut voices[current].events);
        }
    }

    if !in_body {
        return Err(AdapterError::Parse(
            "ABC tune has no K: line (no body)".to_string(),
        ));
    }

    let music = build_music(header_events, voices);
    Ok(MusicDocument { metadata, music })
}

/// Assemble the parsed voices into a Music tree: a single Staff for one voice
/// (back-compatible), or a `Simultaneous` of named Staves for multi-voice tunes.
fn build_music(header_events: Vec<Music>, voices: Vec<VoiceStream>) -> Music {
    if voices.len() <= 1 {
        let mut events = header_events;
        if let Some(v) = voices.into_iter().next() {
            events.extend(v.events);
        }
        return Music::Sequential(events).in_context(ContextType::Staff, None);
    }
    let staves: Vec<Music> = voices
        .into_iter()
        .map(|v| {
            let mut events = header_events.clone();
            events.extend(v.events);
            Music::Sequential(events).in_context(ContextType::Staff, v.name)
        })
        .collect();
    Music::Simultaneous(staves)
}

fn is_header_line(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 2 && b[1] == b':' && (b[0] as char).is_ascii_uppercase()
}

fn split_field(line: &str) -> (char, &str) {
    let key = line.chars().next().unwrap();
    let value = line[2..].trim();
    // Strip trailing inline comment.
    let value = value.split('%').next().unwrap_or(value).trim();
    (key, value)
}

fn apply_inline_field(key: char, value: &str, state: &mut TuneState, events: &mut Vec<Music>) {
    match key {
        'M' => {
            if let Some((n, d)) = parse_meter(value) {
                state.meter = Some(Frac::new(n as i64, d as i64));
                events.push(Music::TimeSignature(TimeSignature {
                    beats: n.to_string(),
                    beat_type: d,
                    symbol: meter_symbol(value),
                }));
            }
        }
        'L' => {
            if let Some(f) = parse_fraction(value) {
                state.unit_length = f;
            }
        }
        'K' => {
            if let Some(k) = parse_key(value) {
                events.push(Music::KeySignature(k));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Header value parsing
// ---------------------------------------------------------------------------

/// Parse `M:` into (numerator, denominator). `C` → 4/4, `C|` → 2/2.
fn parse_meter(value: &str) -> Option<(u8, u8)> {
    let v = value.trim();
    if v == "C" {
        return Some((4, 4));
    }
    if v == "C|" {
        return Some((2, 2));
    }
    let (n, d) = v.split_once('/')?;
    // Compound numerators (e.g. "3+2") sum for the fraction value.
    let num: u32 = n
        .split('+')
        .filter_map(|p| p.trim().parse::<u32>().ok())
        .sum();
    let den: u8 = d.trim().parse().ok()?;
    // Reject a zero numerator or denominator: both are musically meaningless and
    // a 0 denominator panics `Frac::new` downstream (mirrors the guard in
    // `parse_fraction`). A crafted `M:4/0` must not crash the parser.
    if num == 0 || den == 0 {
        return None;
    }
    Some((num as u8, den))
}

fn meter_symbol(value: &str) -> Option<String> {
    match value.trim() {
        "C" => Some("common".to_string()),
        "C|" => Some("cut".to_string()),
        _ => None,
    }
}

/// Parse a fraction like `1/8`, `1/4`, or a bare `1`.
fn parse_fraction(value: &str) -> Option<Frac> {
    let v = value.trim();
    if let Some((n, d)) = v.split_once('/') {
        let num: i64 = n.trim().parse().ok()?;
        let den: i64 = d.trim().parse().ok()?;
        if den == 0 {
            return None;
        }
        Some(Frac::new(num, den))
    } else {
        let num: i64 = v.parse().ok()?;
        Some(Frac::from_integer(num))
    }
}

/// ABC default unit length: 1/16 when the meter is < 0.75, else 1/8.
fn default_unit_length(meter: Option<Frac>) -> Frac {
    match meter {
        Some(m) if m < Frac::new(3, 4) => Frac::new(1, 16),
        _ => Frac::new(1, 8),
    }
}

/// Parse a `K:` value into a key signature (tonic + mode → fifths).
fn parse_key(value: &str) -> Option<KeySignature> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") {
        return None;
    }
    let mut chars = v.chars().peekable();
    let letter = chars.next()?.to_ascii_uppercase();
    if !('A'..='G').contains(&letter) {
        return None;
    }
    // Tonic position on the circle of fifths (as a major key).
    let mut tonic_fifths = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => -1,
        'G' => 1,
        'A' => 3,
        'B' => 5,
        _ => 0,
    };
    if let Some(&c) = chars.peek() {
        if c == '#' {
            tonic_fifths += 7;
            chars.next();
        } else if c == 'b' {
            tonic_fifths -= 7;
            chars.next();
        }
    }
    let rest: String = chars.collect::<String>().trim().to_lowercase();
    let (mode, offset) = mode_from_str(&rest);
    let fifths = clamp_fifths(tonic_fifths + offset);
    Some(KeySignature { fifths, mode })
}

/// Map an ABC mode suffix to (KeyMode, fifths offset from the major key).
fn mode_from_str(s: &str) -> (KeyMode, i8) {
    let s = s.trim();
    let head: String = s.chars().take(3).collect();
    match head.as_str() {
        "" | "maj" | "ion" => (KeyMode::Major, 0),
        "min" | "aeo" => (KeyMode::Minor, -3),
        "m" => (KeyMode::Minor, -3),
        "dor" => (KeyMode::Dorian, -2),
        "phr" => (KeyMode::Phrygian, -4),
        "lyd" => (KeyMode::Lydian, 1),
        "mix" => (KeyMode::Mixolydian, -1),
        "loc" => (KeyMode::Locrian, -5),
        _ if s.starts_with('m') => (KeyMode::Minor, -3),
        _ => (KeyMode::Major, 0),
    }
}

fn clamp_fifths(f: i8) -> i8 {
    f.clamp(-7, 7)
}

// ---------------------------------------------------------------------------
// Body parsing
// ---------------------------------------------------------------------------

fn parse_body_line(line: &str, state: &TuneState, events: &mut Vec<Music>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    // Open tuplet: (actual, normal, sounding events still to collect, start index).
    let mut tuplet: Option<(u8, u8, usize, usize)> = None;
    while i < chars.len() {
        let c = chars[i];
        let before = events.len();
        match c {
            '(' if chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                // Tuplet `(p`, `(p:q`, `(p:q:r`.
                close_tuplet(&mut tuplet, events);
                let ((p, q, r), next) = parse_tuplet_spec(&chars, i + 1);
                tuplet = Some((p, q, r, events.len()));
                i = next;
            }
            ' ' | '\t' => i += 1,
            '%' => break, // rest of line is a comment
            '|' | ':' | '[' if is_barline_at(&chars, i) => {
                let (bar, next) = parse_barline(&chars, i);
                events.push(Music::Barline(bar));
                i = next;
            }
            '"' => {
                // Chord symbol / annotation — skip to closing quote.
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += 1;
                }
                i += 1;
            }
            '!' => {
                // Decoration !...! — skip.
                i += 1;
                while i < chars.len() && chars[i] != '!' {
                    i += 1;
                }
                i += 1;
            }
            '{' => {
                // Grace-note group `{ab}`; a leading `/` marks an acciaccatura.
                let mut j = i + 1;
                let slash = chars.get(j) == Some(&'/');
                if slash {
                    j += 1;
                }
                let end = chars[j..]
                    .iter()
                    .position(|c| *c == '}')
                    .map(|k| j + k)
                    .unwrap_or(chars.len());
                let mut inner = Vec::new();
                let body: String = chars[j..end].iter().collect();
                parse_body_line(&body, state, &mut inner);
                if !inner.is_empty() {
                    events.push(Music::Grace {
                        content: Box::new(Music::Sequential(inner)),
                        slash,
                    });
                }
                i = end + 1;
            }
            '[' => {
                // Chord [CEG].
                let (chord, next) = parse_chord(&chars, i, state);
                if let Some(ch) = chord {
                    events.push(ch);
                }
                i = next;
            }
            'z' | 'x' | 'Z' => {
                let (dur, next) = parse_duration(&chars, i + 1, state, c == 'Z');
                events.push(Music::Rest {
                    duration: dur,
                    is_measure_rest: c == 'Z',
                });
                i = next;
            }
            '^' | '_' | '=' | 'A'..='G' | 'a'..='g' => {
                let (note, next) = parse_note(&chars, i, state);
                if let Some(n) = note {
                    events.push(n);
                }
                i = next;
            }
            '-' => {
                // Tie: attach a TieStart to the previous note.
                attach_tie(events);
                i += 1;
            }
            _ => i += 1, // skip unsupported tokens (slurs, decorations, …)
        }
        if let Some((_, _, rem, _)) = &mut tuplet {
            let n = events[before..].iter().filter(|m| is_sounding(m)).count();
            *rem = rem.saturating_sub(n);
        }
        if matches!(tuplet, Some((_, _, 0, _))) {
            close_tuplet(&mut tuplet, events);
        }
    }
    // ponytail: a tuplet left open at end of line is closed here; ABC allows a
    // tuplet to span a line break, but that is vanishingly rare in real tunes.
    close_tuplet(&mut tuplet, events);
}

/// True for events that consume a tuplet slot (ABC counts notes, not barlines).
fn is_sounding(m: &Music) -> bool {
    matches!(
        m,
        Music::Note { .. } | Music::Chord { .. } | Music::Rest { .. }
    )
}

/// Parse a tuplet spec `p`, `p:q`, `p:q:r` starting at the first digit.
/// Returns ((actual, normal, count), next index).
fn parse_tuplet_spec(chars: &[char], start: usize) -> ((u8, u8, usize), usize) {
    fn num(chars: &[char], i: &mut usize) -> Option<u8> {
        let mut v: u32 = 0;
        let mut saw = false;
        while let Some(d) = chars.get(*i).and_then(|c| c.to_digit(10)) {
            v = (v * 10 + d).min(255);
            saw = true;
            *i += 1;
        }
        saw.then_some(v.max(1) as u8)
    }
    let mut i = start;
    let p = num(chars, &mut i).unwrap_or(3);
    let mut q = None;
    let mut r = None;
    if chars.get(i) == Some(&':') {
        i += 1;
        q = num(chars, &mut i);
        if chars.get(i) == Some(&':') {
            i += 1;
            r = num(chars, &mut i);
        }
    }
    // ABC defaults for a bare `(p`. ponytail: 5/7/9 take q=2 (simple meter);
    // the compound-meter q=3 case needs the un-reduced meter, which the IR
    // time signature does not keep here.
    let q = q.unwrap_or(match p {
        2 | 4 | 8 => 3,
        _ => 2,
    });
    ((p, q, r.unwrap_or(p) as usize), i)
}

/// Close an open tuplet: wrap the events it collected in `Music::Tuplet` and
/// stamp the ratio onto their durations (the rest of the IR reads it there).
fn close_tuplet(tuplet: &mut Option<(u8, u8, usize, usize)>, events: &mut Vec<Music>) {
    let Some((actual, normal, _, start)) = tuplet.take() else {
        return;
    };
    if start >= events.len() {
        return;
    }
    let mut inner = events.split_off(start);
    for m in &mut inner {
        if let Music::Note { duration, .. }
        | Music::Chord { duration, .. }
        | Music::Rest { duration, .. } = m
        {
            duration.tuplet_actual = actual;
            duration.tuplet_normal = normal;
        }
    }
    events.push(Music::Tuplet {
        actual,
        normal,
        content: Box::new(Music::Sequential(inner)),
    });
}

fn is_barline_at(chars: &[char], i: usize) -> bool {
    match chars[i] {
        '|' => true,
        ':' => chars.get(i + 1) == Some(&'|') || chars.get(i + 1) == Some(&':'),
        '[' => chars.get(i + 1) == Some(&'|'),
        _ => false,
    }
}

/// Parse a (possibly repeat) bar line, returning the Barline and next index.
fn parse_barline(chars: &[char], start: usize) -> (Barline, usize) {
    // Collect the run of bar characters.
    let mut i = start;
    let mut run = String::new();
    while i < chars.len() && matches!(chars[i], '|' | ':' | ']' | '[') {
        // Stop if `[` begins a chord/inline field rather than a barline.
        if chars[i] == '[' && chars.get(i + 1) != Some(&'|') {
            break;
        }
        run.push(chars[i]);
        i += 1;
    }
    let mut bar = Barline::default();
    // A leading ':' closes the preceding repeat; a trailing ':' opens the next.
    let closes = run.starts_with(':');
    let opens = run.ends_with(':');
    if closes && opens {
        bar.style = BarlineType::RepeatBoth;
        bar.repeat_direction = Some(RepeatDirection::Backward);
    } else if closes {
        bar.style = BarlineType::RepeatBackward;
        bar.repeat_direction = Some(RepeatDirection::Backward);
    } else if opens {
        bar.style = BarlineType::RepeatForward;
        bar.repeat_direction = Some(RepeatDirection::Forward);
    } else if run == "||" {
        bar.style = BarlineType::Double;
    } else if run == "|]" || run == "[|" {
        bar.style = BarlineType::Final;
    } else {
        bar.style = BarlineType::Regular;
    }
    (bar, i)
}

/// Parse `[CEG]` into a chord (or `[K:...]` inline field, returned as None).
fn parse_chord(chars: &[char], start: usize, state: &TuneState) -> (Option<Music>, usize) {
    // Inline field like [K:G] — skip to ].
    if chars.get(start + 2) == Some(&':') {
        let mut i = start;
        while i < chars.len() && chars[i] != ']' {
            i += 1;
        }
        return (None, i + 1);
    }
    let mut i = start + 1; // past '['
    let mut pitches: Vec<(Pitch, Vec<Annotation>)> = Vec::new();
    while i < chars.len() && chars[i] != ']' {
        if matches!(chars[i], '^' | '_' | '=' | 'A'..='G' | 'a'..='g') {
            if let (Some(Music::Note { pitch, .. }), next) = parse_note(chars, i, state) {
                pitches.push((pitch, Vec::new()));
                i = next;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    i += 1; // past ']'
            // An optional duration follows the chord.
    let (duration, next) = parse_duration(chars, i, state, false);
    if pitches.is_empty() {
        return (None, next);
    }
    (
        Some(Music::Chord {
            pitches,
            duration,
            annotations: Vec::new(),
        }),
        next,
    )
}

/// Parse one note (accidental, letter, octave marks, duration).
fn parse_note(chars: &[char], start: usize, state: &TuneState) -> (Option<Music>, usize) {
    let mut i = start;
    // Accidentals.
    let mut alter = 0i32;
    while i < chars.len() {
        match chars[i] {
            '^' => {
                alter += 1;
                i += 1;
            }
            '_' => {
                alter -= 1;
                i += 1;
            }
            '=' => {
                alter = 0;
                i += 1;
                break;
            }
            _ => break,
        }
    }
    if i >= chars.len() {
        return (None, i);
    }
    let letter = chars[i];
    let (step, base_octave) = match letter {
        'C'..='G' | 'A' | 'B' => (step_from_letter(letter), 4),
        'a'..='g' => (step_from_letter(letter.to_ascii_uppercase()), 5),
        _ => return (None, i + 1),
    };
    i += 1;
    // Octave marks.
    let mut octave = base_octave;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                octave += 1;
                i += 1;
            }
            ',' => {
                octave -= 1;
                i += 1;
            }
            _ => break,
        }
    }
    let (duration, next) = parse_duration(chars, i, state, false);
    let pitch = Pitch::with_alter(step, Alter::from_integer(alter), octave);
    (
        Some(Music::Note {
            pitch,
            duration,
            annotations: Vec::new(),
        }),
        next,
    )
}

fn step_from_letter(letter: char) -> PitchStep {
    match letter {
        'C' => PitchStep::C,
        'D' => PitchStep::D,
        'E' => PitchStep::E,
        'F' => PitchStep::F,
        'G' => PitchStep::G,
        'A' => PitchStep::A,
        'B' => PitchStep::B,
        _ => PitchStep::C,
    }
}

/// Parse a duration suffix `[num][/[den]]` as a multiple of the unit length.
fn parse_duration(
    chars: &[char],
    start: usize,
    state: &TuneState,
    measure_rest: bool,
) -> (Duration, usize) {
    // A duration multiplier never legitimately exceeds a few digits; cap it so
    // a malicious digit run can't overflow i64 (panic in debug, wrap in release)
    // or blow up Frac arithmetic downstream.
    const MAX_DUR: i64 = 1_000_000;
    let mut i = start;
    let mut num: i64 = 0;
    let mut saw_num = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        num = (num.saturating_mul(10)).saturating_add(chars[i].to_digit(10).unwrap() as i64);
        saw_num = true;
        i += 1;
    }
    if !saw_num {
        num = 1;
    }
    num = num.min(MAX_DUR);
    let mut den: i64 = 1;
    if i < chars.len() && chars[i] == '/' {
        let mut slashes = 0;
        while i < chars.len() && chars[i] == '/' {
            slashes += 1;
            i += 1;
        }
        let mut den_num: i64 = 0;
        let mut saw_den = false;
        while i < chars.len() && chars[i].is_ascii_digit() {
            den_num =
                (den_num.saturating_mul(10)).saturating_add(chars[i].to_digit(10).unwrap() as i64);
            saw_den = true;
            i += 1;
        }
        den = if saw_den {
            den_num.clamp(1, MAX_DUR)
        } else {
            // Each bare '/' halves: '/'=2, '//'=4. Cap the shift: 63+ slashes
            // would overflow the i64 shift and panic.
            1i64 << slashes.min(20)
        };
    }
    let mult = Frac::new(num, den.max(1));
    // (A `Z` measure rest counts whole measures, but we lack the bar length
    // here, so it is treated like `z` with is_measure_rest set by the caller.)
    let _ = measure_rest;
    let base = state.unit_length * mult;
    (Duration::new(base), i)
}

/// Attach a tie-start annotation to the most recent note/chord.
fn attach_tie(events: &mut [Music]) {
    if let Some(Music::Note { annotations, .. } | Music::Chord { annotations, .. }) =
        events.last_mut()
    {
        annotations.push(Annotation::TieStart);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> MusicDocument {
        AbcToIrAdapter::new().convert_str_to_music(s).unwrap()
    }

    /// Flatten the staff's sequential events.
    fn events(doc: &MusicDocument) -> Vec<Music> {
        fn inner(m: &Music) -> Vec<Music> {
            match m {
                Music::Context { content, .. } => inner(content),
                Music::Sequential(v) => v.clone(),
                _ => vec![m.clone()],
            }
        }
        inner(&doc.music)
    }

    fn notes(doc: &MusicDocument) -> Vec<Pitch> {
        events(doc)
            .iter()
            .filter_map(|m| match m {
                Music::Note { pitch, .. } => Some(*pitch),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_header_metadata() {
        let doc = parse("X:1\nT:Test Tune\nC:Anon\nM:4/4\nL:1/8\nK:C\nCDEF|\n");
        assert_eq!(doc.metadata.title.as_deref(), Some("Test Tune"));
        assert_eq!(doc.metadata.composer.as_deref(), Some("Anon"));
    }

    #[test]
    fn test_pitch_octave_convention() {
        // C=middle C (MIDI 60), c=72, C,=48, c'=84.
        let doc = parse("X:1\nK:C\nC c C, c'\n");
        let midis: Vec<i32> = notes(&doc).iter().map(|p| p.midi_number()).collect();
        assert_eq!(midis, vec![60, 72, 48, 84]);
    }

    #[test]
    fn test_accidentals() {
        let doc = parse("X:1\nK:C\n^F _B =C ^^G __A\n");
        let ns = notes(&doc);
        assert_eq!(ns[0].midi_number(), 66); // F#4
        assert_eq!(ns[1].midi_number(), 70); // Bb4
        assert_eq!(ns[2].midi_number(), 60); // C natural
        assert_eq!(ns[3].midi_number(), 69); // G##4
        assert_eq!(ns[4].midi_number(), 67); // Abb4 = G4
    }

    #[test]
    fn test_durations() {
        // L:1/8 → C=1/8, C2=1/4, C/2=1/16, C/=1/16, C3/2=3/16.
        let doc = parse("X:1\nL:1/8\nK:C\nC C2 C/2 C/ C3/2\n");
        let durs: Vec<Frac> = events(&doc)
            .iter()
            .filter_map(|m| match m {
                Music::Note { duration, .. } => Some(duration.actual_duration()),
                _ => None,
            })
            .collect();
        assert_eq!(
            durs,
            vec![
                Frac::new(1, 8),
                Frac::new(1, 4),
                Frac::new(1, 16),
                Frac::new(1, 16),
                Frac::new(3, 16),
            ]
        );
    }

    #[test]
    fn test_default_unit_length_from_meter() {
        // M:2/4 (<0.75) → default L = 1/16.
        let doc = parse("X:1\nM:2/4\nK:C\nC\n");
        let dur = events(&doc).iter().find_map(|m| match m {
            Music::Note { duration, .. } => Some(duration.actual_duration()),
            _ => None,
        });
        assert_eq!(dur, Some(Frac::new(1, 16)));
        // M:4/4 (>=0.75) → default 1/8.
        let doc2 = parse("X:1\nM:4/4\nK:C\nC\n");
        let dur2 = events(&doc2).iter().find_map(|m| match m {
            Music::Note { duration, .. } => Some(duration.actual_duration()),
            _ => None,
        });
        assert_eq!(dur2, Some(Frac::new(1, 8)));
    }

    #[test]
    fn test_key_signatures() {
        let cases = [
            ("C", 0, KeyMode::Major),
            ("G", 1, KeyMode::Major),
            ("D", 2, KeyMode::Major),
            ("F", -1, KeyMode::Major),
            ("Bb", -2, KeyMode::Major),
            ("Am", 0, KeyMode::Minor),
            ("Em", 1, KeyMode::Minor),
            ("Dm", -1, KeyMode::Minor),
            ("Ddor", 0, KeyMode::Dorian),
            ("Gmix", 0, KeyMode::Mixolydian),
        ];
        for (k, fifths, mode) in cases {
            let doc = parse(&format!("X:1\nK:{k}\nC\n"));
            let ks = events(&doc).iter().find_map(|m| match m {
                Music::KeySignature(k) => Some(*k),
                _ => None,
            });
            let ks = ks.unwrap_or_else(|| panic!("no key for {k}"));
            assert_eq!(ks.fifths, fifths, "fifths for {k}");
            assert_eq!(ks.mode, mode, "mode for {k}");
        }
    }

    #[test]
    fn test_time_signature() {
        let doc = parse("X:1\nM:6/8\nK:C\nC\n");
        let ts = events(&doc).iter().find_map(|m| match m {
            Music::TimeSignature(t) => Some(t.clone()),
            _ => None,
        });
        let ts = ts.unwrap();
        assert_eq!(ts.beats, "6");
        assert_eq!(ts.beat_type, 8);
    }

    #[test]
    fn test_rests_and_chords() {
        let doc = parse("X:1\nL:1/4\nK:C\nz [CEG] z2\n");
        let evs = events(&doc);
        let rests = evs
            .iter()
            .filter(|m| matches!(m, Music::Rest { .. }))
            .count();
        assert_eq!(rests, 2);
        let chord = evs.iter().find_map(|m| match m {
            Music::Chord { pitches, .. } => Some(pitches.len()),
            _ => None,
        });
        assert_eq!(chord, Some(3));
    }

    #[test]
    fn test_barlines_and_repeats() {
        let doc = parse("X:1\nK:C\n|: CD | EF :|\n");
        let bars: Vec<BarlineType> = events(&doc)
            .iter()
            .filter_map(|m| match m {
                Music::Barline(b) => Some(b.style),
                _ => None,
            })
            .collect();
        assert!(bars.contains(&BarlineType::RepeatForward));
        assert!(bars.contains(&BarlineType::RepeatBackward));
    }

    #[test]
    fn test_ties() {
        let doc = parse("X:1\nK:C\nC-C\n");
        let first_has_tie = events(&doc).iter().any(|m| {
            matches!(m, Music::Note { annotations, .. } if annotations.contains(&Annotation::TieStart))
        });
        assert!(first_has_tie);
    }

    #[test]
    fn test_no_key_errors() {
        let r = AbcToIrAdapter::new().convert_str_to_music("X:1\nT:no body\n");
        assert!(r.is_err());
    }

    #[test]
    fn test_skips_unsupported_tokens() {
        // Chord symbols, decorations, grace notes, slurs must not break parsing.
        let doc = parse("X:1\nK:G\n\"G\"G2 !trill!A {ag}f (Bc)\n");
        assert!(notes(&doc).len() >= 4);
    }

    // ---- Multi-voice (ABC 2.1 V:) ----

    /// Collect each top-level Staff voice's note MIDI numbers, in branch order.
    fn voice_notes(doc: &MusicDocument) -> Vec<Vec<i32>> {
        fn staff_notes(m: &Music) -> Vec<i32> {
            let mut out = Vec::new();
            fn walk(m: &Music, out: &mut Vec<i32>) {
                match m {
                    Music::Sequential(v) => v.iter().for_each(|x| walk(x, out)),
                    Music::Context { content, .. } => walk(content, out),
                    Music::Note { pitch, .. } => out.push(pitch.midi_number()),
                    _ => {}
                }
            }
            walk(m, &mut out);
            out
        }
        match &doc.music {
            Music::Simultaneous(branches) => branches.iter().map(staff_notes).collect(),
            single => vec![staff_notes(single)],
        }
    }

    #[test]
    fn test_multivoice_two_voices() {
        // Two voices declared in the header, bodies switched by line-start V:.
        let doc = parse("X:1\nM:4/4\nL:1/4\nK:C\nV:1\nC D E F|\nV:2\nC, D, E, F,|\n");
        let vs = voice_notes(&doc);
        assert_eq!(vs.len(), 2, "expected two voices, got {}", vs.len());
        assert_eq!(vs[0], vec![60, 62, 64, 65]); // C D E F
        assert_eq!(vs[1], vec![48, 50, 52, 53]); // C, D, E, F,
    }

    #[test]
    fn test_multivoice_interleaved_blocks() {
        // Voice streams accumulate across multiple V: blocks (ABC 2.1 §4.1).
        let doc = parse("X:1\nK:C\nV:1\nCD|\nV:2\nE,F,|\nV:1\nGA|\nV:2\nB,c,|\n");
        let vs = voice_notes(&doc);
        assert_eq!(vs.len(), 2);
        assert_eq!(vs[0], vec![60, 62, 67, 69]); // C D G A
        assert_eq!(vs[1], vec![52, 53, 59, 60]); // E, F, B, c,
    }

    #[test]
    fn test_multivoice_names_become_staff_names() {
        let doc = parse("X:1\nK:C\nV:1 name=\"Soprano\"\nV:2 name=\"Bass\"\nV:1\nC|\nV:2\nC,|\n");
        let names: Vec<Option<String>> = match &doc.music {
            Music::Simultaneous(b) => b
                .iter()
                .map(|m| match m {
                    Music::Context { name, .. } => name.clone(),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        };
        assert_eq!(
            names,
            vec![Some("Soprano".to_string()), Some("Bass".to_string())]
        );
    }

    #[test]
    fn test_multivoice_shared_header_in_each_voice() {
        // The header M:/K: must appear in every voice so each lowers correctly.
        let doc = parse("X:1\nM:3/4\nK:D\nV:1\nDEF|\nV:2\nA,B,C|\n");
        for branch in match &doc.music {
            Music::Simultaneous(b) => b.clone(),
            _ => panic!("expected multi-voice"),
        } {
            let has_time = matches!(&branch, Music::Context { content, .. }
                if matches!(content.as_ref(), Music::Sequential(v)
                    if v.iter().any(|m| matches!(m, Music::TimeSignature(_)))));
            let has_key = matches!(&branch, Music::Context { content, .. }
                if matches!(content.as_ref(), Music::Sequential(v)
                    if v.iter().any(|m| matches!(m, Music::KeySignature(_)))));
            assert!(has_time && has_key, "voice missing shared M:/K:");
        }
    }

    #[test]
    fn test_multivoice_inline_marker() {
        // `[V:id]` inline at line start switches the voice for that line.
        let doc = parse("X:1\nK:C\n[V:1] CD|\n[V:2] E,F,|\n");
        let vs = voice_notes(&doc);
        assert_eq!(vs.len(), 2);
        assert_eq!(vs[0], vec![60, 62]);
        assert_eq!(vs[1], vec![52, 53]); // E, F,
    }

    #[test]
    fn test_single_voice_unchanged() {
        // A tune with no V: is still a single Staff (not a Simultaneous).
        let doc = parse("X:1\nK:C\nCDEF|\n");
        assert!(matches!(doc.music, Music::Context { .. }));
    }
}
#[cfg(test)]
mod boundary_tests {
    use super::*;

    /// Digit runs and slash runs in duration suffixes must not overflow/panic.
    #[test]
    fn malformed_duration_does_not_panic() {
        let adapter = AbcToIrAdapter::new();
        let long_digits = "9".repeat(30);
        let _ = adapter.convert_str(&format!("X:1\nK:C\nC{long_digits}|\n"));
        let _ = adapter.convert_str(&format!("X:1\nK:C\nC/{long_digits}|\n"));
        let slashes = "/".repeat(80);
        let _ = adapter.convert_str(&format!("X:1\nK:C\nC{slashes}|\n"));
    }
}
