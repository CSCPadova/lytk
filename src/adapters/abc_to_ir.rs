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

fn parse_tune(text: &str) -> Result<MusicDocument> {
    let mut metadata = ScoreMetadata::default();
    let mut state = TuneState {
        unit_length: Frac::new(1, 8),
        meter: None,
    };
    let mut events: Vec<Music> = Vec::new();
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
                        explicit_unit_length = true;
                    }
                }
                'K' => {
                    if let Some(key_sig) = parse_key(value) {
                        events.push(Music::KeySignature(key_sig));
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
            // An inline `K:`/`M:`/`L:` field can also appear at line start.
            if is_header_line(line) {
                let (key, value) = split_field(line);
                apply_inline_field(key, value, &mut state, &mut events);
                continue;
            }
            parse_body_line(line, &state, &mut events);
        }
    }

    if !in_body {
        return Err(AdapterError::Parse(
            "ABC tune has no K: line (no body)".to_string(),
        ));
    }

    let music = Music::Sequential(events).in_context(ContextType::Staff, None);
    Ok(MusicDocument { metadata, music })
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
    while i < chars.len() {
        let c = chars[i];
        match c {
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
                // Grace-note group — skip (not represented in v1).
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                i += 1;
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
            _ => i += 1, // skip unsupported tokens (slurs, tuplets, etc.)
        }
    }
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
    let mut i = start;
    let mut num: i64 = 0;
    let mut saw_num = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        num = num * 10 + chars[i].to_digit(10).unwrap() as i64;
        saw_num = true;
        i += 1;
    }
    if !saw_num {
        num = 1;
    }
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
            den_num = den_num * 10 + chars[i].to_digit(10).unwrap() as i64;
            saw_den = true;
            i += 1;
        }
        den = if saw_den {
            den_num.max(1)
        } else {
            // Each bare '/' halves: '/'=2, '//'=4.
            1i64 << slashes
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
                Music::KeySignature(k) => Some(k.clone()),
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
}
