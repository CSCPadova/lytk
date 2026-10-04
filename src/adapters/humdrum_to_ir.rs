//! Humdrum (`**kern`) → IR adapter.
//!
//! Parses a Humdrum file's `**kern` spines into a Layer-1 [`MusicDocument`].
//! Each kern spine becomes one Staff context (spines are ordered low→high in
//! kern, so they are reversed into top-down part order). Supported subset:
//! pitches (letter runs + `#`/`-`/`n` accidentals), recip durations including
//! dots and rational `N%M` values, rests, chords, ties (`[` `_` `]`), slurs,
//! grace notes (`q`/`Q`), fermatas, barlines (incl. repeats), and the tandem
//! interpretations `*clef…`, `*k[…]`, `*X:` (mode), `*M n/d`, `*MM n`, `*I"…`.
//!
//! Not supported (clear error, no silent loss): spine rearrangement
//! (`*^`, `*v`, `*x`). Non-kern spines (`**dynam`, `**text`, …) are skipped.
//! Beam marks (`L`/`J`) and unmapped ornament characters are ignored.

use std::collections::HashMap;
use std::path::Path;

use crate::ir::annotation::Annotation;
use crate::ir::articulation::{Articulation, Fermata, LyricSyllable, Placement, SyllabicType};
use crate::ir::direction::{Barline, BarlineType, RepeatDirection, TempoDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, TimeSignature};
use crate::ir::music::{ContextType, Music, MusicDocument};
use crate::ir::pitch::{Alter, Pitch, PitchStep};
use crate::ir::score::ScoreMetadata;

use super::{AdapterError, Result, ToIrAdapter, ToMusicAdapter};

/// Adapter that reads Humdrum `**kern`.
#[derive(Default)]
pub struct HumdrumToIrAdapter;

impl HumdrumToIrAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl ToMusicAdapter for HumdrumToIrAdapter {
    fn convert_file_to_music(&self, path: &Path) -> Result<MusicDocument> {
        // Older kern corpora (e.g. Palestrina) are Latin-1.
        let text = super::decode_text_bytes(std::fs::read(path)?);
        self.convert_str_to_music(&text)
    }

    fn convert_str_to_music(&self, text: &str) -> Result<MusicDocument> {
        let doc = parse_kern(text)?;
        super::check_music_length(&doc.music)?;
        Ok(doc)
    }
}

impl ToIrAdapter for HumdrumToIrAdapter {
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
// Parsing
// ---------------------------------------------------------------------------

/// One kern spine being accumulated.
struct Spine {
    name: Option<String>,
    events: Vec<Music>,
    /// Beams open (`L` opens one level, `J` closes one).
    beams: usize,
}

fn parse_kern(text: &str) -> Result<MusicDocument> {
    let mut metadata = ScoreMetadata::default();
    // Column index (in the tab-separated rows) → kern spine accumulator.
    // `None` for non-kern spines we skip.
    let mut spines: Vec<Option<Spine>> = Vec::new();
    let mut started = false;
    // Anacrusis detection: the longest kern spine's music before the first
    // barline (one voice may end early), and the first declared bar length.
    let mut lead: Vec<Frac> = Vec::new();
    let mut saw_barline = false;
    let mut bar_len: Option<Frac> = None;
    let mut beamed = false;
    // `**text`/`**silbe` columns: (kern column on their left, verse).
    let mut text_spines: Vec<Option<(usize, u8)>> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        // Reference records and global comments.
        if let Some(rest) = line.strip_prefix("!!!") {
            if let Some((key, value)) = rest.split_once(':') {
                let value = value.trim().to_string();
                // Keys may carry qualifiers like `OTL@@DE`.
                match key.split('@').next().unwrap_or("") {
                    "COM" if metadata.composer.is_none() => metadata.composer = Some(value),
                    "OTL" if metadata.title.is_none() => metadata.title = Some(value),
                    _ => {}
                }
            }
            continue;
        }
        if line.starts_with("!!") {
            continue;
        }

        let tokens: Vec<&str> = line.split('\t').collect();

        // Local comment row.
        if tokens.first().is_some_and(|t| t.starts_with('!')) {
            continue;
        }

        // Interpretation row.
        if tokens.first().is_some_and(|t| t.starts_with('*')) {
            if !started {
                // Exclusive interpretation row establishes the spines.
                if tokens.iter().any(|t| t.starts_with("**")) {
                    // A text spine sings the kern spine on its left, a verse
                    // per text spine.
                    let mut verses: HashMap<usize, u8> = HashMap::new();
                    for (i, t) in tokens.iter().enumerate() {
                        let kern = tokens[..i].iter().rposition(|k| *k == "**kern");
                        text_spines.push(match (*t, kern) {
                            ("**text" | "**silbe", Some(k)) => {
                                let v = verses.entry(k).or_insert(0);
                                *v = v.saturating_add(1);
                                Some((k, *v))
                            }
                            _ => None,
                        });
                    }
                    for t in &tokens {
                        spines.push(if *t == "**kern" {
                            Some(Spine {
                                name: None,
                                events: Vec::new(),
                                beams: 0,
                            })
                        } else {
                            None
                        });
                    }
                    started = true;
                    continue;
                }
                continue; // stray interpretation before **kern — ignore
            }
            for (i, t) in tokens.iter().enumerate() {
                if matches!(*t, "*^" | "*v" | "*x") {
                    return Err(AdapterError::Parse(
                        "Humdrum spine rearrangement (*^/*v/*x) is not supported yet".into(),
                    ));
                }
                let Some(Some(spine)) = spines.get_mut(i) else {
                    continue;
                };
                parse_interpretation(t, spine);
            }
            continue;
        }

        if !started {
            continue; // data before **kern header — ignore
        }

        // Barline row.
        if tokens.first().is_some_and(|t| t.starts_with('=')) {
            saw_barline = true;
            let bar = parse_barline(tokens[0]);
            // `:|!|:` ends one repeat and starts the next.
            let restart = (bar.repeat_direction == Some(RepeatDirection::Backward)
                && tokens[0].contains("|:"))
            .then(|| Barline {
                style: BarlineType::RepeatForward,
                repeat_direction: Some(RepeatDirection::Forward),
                ..Default::default()
            });
            for spine in spines.iter_mut().flatten() {
                spine.events.push(Music::Barline(bar.clone()));
                spine.events.extend(restart.clone().map(Music::Barline));
            }
            continue;
        }

        // Data row.
        if lead.len() < spines.len() {
            lead.resize(spines.len(), Frac::from_integer(0));
        }
        for (i, t) in tokens.iter().enumerate() {
            let Some(Some(spine)) = spines.get_mut(i) else {
                continue;
            };
            if *t == "." || t.is_empty() {
                continue;
            }
            if let Some(mut event) = parse_data_token(t) {
                if !saw_barline {
                    lead[i] += event_whole_notes(&event);
                }
                // Beams: the outermost `L` begins a group, the `J` that
                // closes the last level ends it (on a chord's first note).
                let first = t.split_whitespace().next().unwrap_or("");
                let (opens, closes) = (first.matches('L').count(), first.matches('J').count());
                beamed |= opens + closes > 0;
                let before = spine.beams;
                spine.beams = (before + opens).saturating_sub(closes);
                if let Some(a) = lead_annotations(&mut event) {
                    if before == 0 && opens > 0 {
                        a.push(Annotation::BeamStart);
                    }
                    if before > 0 && spine.beams == 0 {
                        a.push(Annotation::BeamStop);
                    }
                }
                spine.events.push(event);
            }
        }
        // Syllables on the notes their kern spine has on this row.
        for (i, t) in tokens.iter().enumerate() {
            let Some(Some((k, verse))) = text_spines.get(i) else {
                continue;
            };
            if *t == "." || t.is_empty() || tokens.get(*k).is_none_or(|n| *n == ".") {
                continue;
            }
            let Some(Some(spine)) = spines.get_mut(*k) else {
                continue;
            };
            if let Some(a) = spine.events.last_mut().and_then(lead_annotations) {
                a.push(Annotation::Lyric(text_syllable(t, *verse)));
            }
        }
    }
    // A file that beams decides every note's beaming.
    if beamed {
        for e in spines.iter_mut().flatten().flat_map(|s| &mut s.events) {
            if let Some(a) = lead_annotations(e) {
                a.push(Annotation::NoAutoBeam);
            }
        }
    }

    // Anacrusis: content before the first barline shorter than a full bar.
    if saw_barline {
        if bar_len.is_none() {
            bar_len = spines
                .iter()
                .flatten()
                .flat_map(|s| &s.events)
                .find_map(|e| {
                    if let Music::TimeSignature(ts) = e {
                        Some(ts.beats_fraction())
                    } else {
                        None
                    }
                });
        }
        let lead_dur = lead
            .into_iter()
            .max()
            .unwrap_or_else(|| Frac::from_integer(0));
        if let Some(bl) = bar_len {
            if lead_dur > Frac::from_integer(0) && lead_dur < bl {
                metadata.partial_duration = Some(Duration::new(lead_dur));
            }
        }
    }

    let mut staves: Vec<Music> = spines
        .into_iter()
        .flatten()
        .filter(|s| !s.events.is_empty())
        .map(|s| Music::Sequential(s.events).in_context(ContextType::Staff, s.name))
        .collect();
    if staves.is_empty() {
        return Err(AdapterError::Parse(
            "no **kern spine found in Humdrum input".into(),
        ));
    }
    // Kern orders spines low→high (bass leftmost); scores read top-down.
    staves.reverse();
    let music = if staves.len() == 1 {
        staves.pop().unwrap()
    } else {
        Music::Simultaneous(staves)
    };
    let mut doc = MusicDocument::new(music);
    doc.metadata = metadata;
    Ok(doc)
}

/// Metrical length of a parsed event, in whole notes (graces count zero).
fn event_whole_notes(event: &Music) -> Frac {
    match event {
        Music::Note { duration, .. } => duration.actual_duration(),
        Music::Chord { duration, .. } => duration.actual_duration(),
        Music::Rest { duration, .. } | Music::Skip { duration } => duration.actual_duration(),
        _ => Frac::from_integer(0),
    }
}

/// Apply a tandem interpretation token to a spine.
fn parse_interpretation(t: &str, spine: &mut Spine) {
    if let Some(name) = t.strip_prefix("*I\"") {
        spine.name = Some(name.trim().to_string());
    } else if let Some(tr) = t.strip_prefix("*ITrd") {
        // `*ITrd-1c-2`: the spine sounds 1 step, 2 semitones below.
        let parsed = tr.split_once('c').and_then(|(d, c)| {
            let d: i32 = d.parse().ok()?;
            let c: i32 = c.parse().ok()?;
            let o = d / 7;
            Some(crate::ir::measure::Transpose {
                diatonic: i8::try_from(d - 7 * o).ok()?,
                chromatic: i8::try_from(c - 12 * o).ok()?,
                octave_change: i8::try_from(o).ok()?,
            })
        });
        spine.events.extend(parsed.map(Music::Transposition));
    } else if let Some(clef) = t.strip_prefix("*clef") {
        if let Some(c) = parse_clef(clef) {
            spine.events.push(Music::Clef(c));
        }
    } else if let Some(inner) = t.strip_prefix("*k[").and_then(|r| r.strip_suffix(']')) {
        spine.events.push(Music::KeySignature(KeySignature {
            fifths: key_sig_fifths(inner),
            mode: KeyMode::Major,
        }));
    } else if let Some(meter) = t.strip_prefix("*M") {
        if let Some(bpm) = meter.strip_prefix('M').and_then(|n| n.parse::<f64>().ok()) {
            // *MM<n> — metronome marking.
            spine.events.push(Music::Tempo(TempoDirection {
                text: None,
                beat_unit: Some("quarter".to_string()),
                per_minute: Some(bpm),
                dots: 0,
                placement: Placement::Above,
            }));
        } else if let Some((n, d)) = meter.split_once('/') {
            if let (false, Ok(dv)) = (n.is_empty(), d.parse::<u8>()) {
                spine.events.push(Music::TimeSignature(TimeSignature {
                    beats: n.to_string(),
                    beat_type: dv,
                    symbol: None,
                }));
            }
        }
    } else if t.len() > 2 && t.ends_with(':') {
        // Key designation `*G:` / `*g:` — mode for the latest key signature.
        let tonic = &t[1..t.len() - 1];
        let minor = tonic.chars().next().is_some_and(|c| c.is_ascii_lowercase());
        if let Some(Music::KeySignature(k)) = spine
            .events
            .iter_mut()
            .rev()
            .find(|e| matches!(e, Music::KeySignature(_)))
        {
            k.mode = if minor {
                KeyMode::Minor
            } else {
                KeyMode::Major
            };
        }
    }
}

/// `G2`, `F4`, `C3`, `Gv2` (octave down), `G^2` (octave up).
fn parse_clef(s: &str) -> Option<Clef> {
    let mut chars = s.chars();
    let sign = match chars.next()? {
        'G' => ClefSign::G,
        'F' => ClefSign::F,
        'C' => ClefSign::C,
        'X' => ClefSign::Percussion,
        _ => return None,
    };
    let rest: String = chars.collect();
    let octave_change = if rest.contains('v') {
        -1
    } else if rest.contains('^') {
        1
    } else {
        0
    };
    let line: u8 = rest
        .trim_matches(|c| c == 'v' || c == '^')
        .parse()
        .unwrap_or(match sign {
            ClefSign::G => 2,
            ClefSign::F => 4,
            _ => 3,
        });
    Some(Clef {
        sign,
        line,
        octave_change,
    })
}

/// `f#c#` → 2, `b-e-` → -2, `` → 0.
fn key_sig_fifths(inner: &str) -> i8 {
    let sharps = inner.matches('#').count() as i8;
    let flats = inner.matches('-').count() as i8;
    if flats > 0 {
        -flats
    } else {
        sharps
    }
}

fn parse_barline(t: &str) -> Barline {
    let (style, repeat) = if t.contains(":|") {
        (BarlineType::RepeatBackward, Some(RepeatDirection::Backward))
    } else if t.contains("|:") {
        (BarlineType::RepeatForward, Some(RepeatDirection::Forward))
    } else if t.starts_with("==") {
        (BarlineType::Final, None)
    } else if t.contains("||") {
        (BarlineType::Double, None)
    } else {
        (BarlineType::Regular, None)
    };
    Barline {
        style,
        repeat_direction: repeat,
        ..Default::default()
    }
}

/// Characters carrying per-note/token meaning we map; everything else
/// (beams `L`/`J`, editorial marks, unmapped ornaments) is ignored.
struct TokenFlags {
    tie_start: bool,
    tie_stop: bool,
    slur_start: bool,
    slur_stop: bool,
    fermata: bool,
    grace: Option<bool>, // Some(slash?)
    /// `y`: an invisible token (an invisible rest is a spacer).
    hidden: bool,
    /// Articulations, by MusicXML name.
    articulations: Vec<&'static str>,
}

/// A `**text` token: `Hal-` begins a word, `-le-` goes on with it, `-lu`
/// ends it.
fn text_syllable(token: &str, verse: u8) -> LyricSyllable {
    let (continues, rest) = match token.strip_prefix('-') {
        Some(r) if !r.is_empty() => (true, r),
        _ => (false, token),
    };
    let (text, goes_on) = match rest.strip_suffix('-') {
        Some(t) if !t.is_empty() => (t, true),
        _ => (rest, false),
    };
    LyricSyllable {
        text: text.to_string(),
        syllabic: match (continues, goes_on) {
            (false, false) => SyllabicType::Single,
            (false, true) => SyllabicType::Begin,
            (true, true) => SyllabicType::Middle,
            (true, false) => SyllabicType::End,
        },
        number: verse,
        ..LyricSyllable::default()
    }
}

/// The annotations a note's beams go on: a note's, a chord's first note's
/// (a grace's too).
fn lead_annotations(m: &mut Music) -> Option<&mut Vec<Annotation>> {
    match m {
        Music::Note { annotations, .. } => Some(annotations),
        Music::Chord { pitches, .. } => pitches.first_mut().map(|(_, a)| a),
        Music::Grace { content, .. } => lead_annotations(content),
        _ => None,
    }
}

fn parse_data_token(token: &str) -> Option<Music> {
    // A chord is space-separated subtokens sharing one time slot.
    let subtokens: Vec<&str> = token.split_whitespace().collect();
    if subtokens.is_empty() {
        return None;
    }

    let mut notes: Vec<(Pitch, Duration, TokenFlags)> = Vec::new();
    let mut rest: Option<(Duration, bool)> = None;
    for sub in &subtokens {
        let (dur, flags, body) = split_subtoken(sub);
        if body.contains('r') {
            rest = Some((dur, flags.hidden));
            continue;
        }
        let pitch = parse_kern_pitch(&body)?;
        notes.push((pitch, dur, flags));
    }

    if notes.is_empty() {
        return Some(match rest? {
            (duration, true) => Music::Skip { duration },
            (duration, false) => Music::Rest {
                duration,
                is_measure_rest: false,
            },
        });
    }

    let annotations = |f: &TokenFlags| {
        let mut a = Vec::new();
        if f.tie_stop {
            a.push(Annotation::TieStop);
        }
        if f.tie_start {
            a.push(Annotation::TieStart);
        }
        if f.slur_start {
            a.push(Annotation::SlurStart {
                number: 1,
                placement: Placement::Unspecified,
            });
        }
        if f.slur_stop {
            a.push(Annotation::SlurStop { number: 1 });
        }
        for name in &f.articulations {
            a.push(Annotation::Articulation(Articulation {
                name: name.to_string(),
                placement: Placement::Unspecified,
            }));
        }
        if f.fermata {
            a.push(Annotation::Fermata(Fermata {
                shape: String::new(),
                inverted: false,
            }));
        }
        a
    };

    let grace = notes[0].2.grace;
    let music = if notes.len() == 1 {
        let (pitch, duration, flags) = &notes[0];
        Music::Note {
            pitch: *pitch,
            duration: duration.clone(),
            annotations: annotations(flags),
        }
    } else {
        let duration = notes[0].1.clone();
        Music::Chord {
            pitches: notes.iter().map(|(p, _, f)| (*p, annotations(f))).collect(),
            duration,
            annotations: Vec::new(),
        }
    };

    Some(match grace {
        Some(slash) => Music::Grace {
            content: Box::new(music),
            slash,
        },
        None => music,
    })
}

/// Split a kern subtoken into (duration, flags, pitch/rest body).
fn split_subtoken(sub: &str) -> (Duration, TokenFlags, String) {
    let mut flags = TokenFlags {
        tie_start: false,
        tie_stop: false,
        slur_start: false,
        slur_stop: false,
        fermata: false,
        grace: None,
        hidden: false,
        articulations: Vec::new(),
    };
    let mut digits = String::new();
    let mut dots = 0u8;
    let mut rational: Option<(String, String)> = None;
    let mut body = String::new();

    for c in sub.chars() {
        match c {
            '[' => flags.tie_start = true,
            ']' => flags.tie_stop = true,
            '_' => {
                flags.tie_start = true;
                flags.tie_stop = true;
            }
            '(' => flags.slur_start = true,
            ')' => flags.slur_stop = true,
            ';' => flags.fermata = true,
            'q' => flags.grace = Some(true),
            'Q' => flags.grace = Some(false),
            'y' => flags.hidden = true,
            '\'' => flags.articulations.push("staccato"),
            '`' => flags.articulations.push("staccatissimo"),
            '~' => flags.articulations.push("tenuto"),
            // `^` an accent, `^^` a heavy one.
            '^' => match flags.articulations.last_mut() {
                Some(a) if *a == "accent" => *a = "strong-accent",
                _ => flags.articulations.push("accent"),
            },
            '0'..='9' => {
                if let Some((_, den)) = &mut rational {
                    den.push(c);
                } else {
                    digits.push(c);
                }
            }
            '%' => rational = Some((std::mem::take(&mut digits), String::new())),
            '.' => dots = dots.saturating_add(1),
            'a'..='g' | 'A'..='G' | '#' | '-' | 'n' | 'r' => body.push(c),
            _ => {} // beams (L/J), ornaments, editorial marks — ignored
        }
    }

    // Recip value X → duration 1/X whole notes. `0` = breve.
    // ponytail: cap recip parts at 4 digits; kern never needs more.
    let dur = if let Some((num, den)) = rational {
        let n: i64 = num
            .get(..4.min(num.len()))
            .unwrap_or("1")
            .parse()
            .unwrap_or(1);
        let d: i64 = den
            .get(..4.min(den.len()))
            .unwrap_or("1")
            .parse()
            .unwrap_or(1);
        duration_from_recip(Frac::new(n.max(1), d.max(1)), dots)
    } else if digits.is_empty() {
        // No recip (bare grace or malformed): default to an eighth.
        duration_from_recip(Frac::from_integer(8), dots)
    } else {
        let n: i64 = digits.get(..4).unwrap_or(&digits).parse().unwrap_or(4);
        if n == 0 {
            // breve
            let mut d = Duration::new(Frac::from_integer(2));
            d.dots = dots;
            d
        } else {
            duration_from_recip(Frac::from_integer(n), dots)
        }
    };

    (dur, flags, body)
}

/// Build a Duration from a recip value X (duration = 1/X whole notes),
/// decomposing non-power-of-two X into a power-of-two base + tuplet ratio so
/// downstream emitters see ordinary tuplets (recip 12 → eighth in a 3:2).
fn duration_from_recip(recip: Frac, dots: u8) -> Duration {
    let mut dur = Duration::new(Frac::new(*recip.denom(), *recip.numer()));
    dur.dots = dots;
    let base = dur.base;
    if *base.numer() == 1 {
        let n = *base.denom();
        if n > 0 && (n as u64).count_ones() != 1 {
            let p2 = 1i64 << (63 - n.leading_zeros()); // largest power of two ≤ n
            let ratio = Frac::new(n, p2);
            let (actual, normal) = (*ratio.numer(), *ratio.denom());
            if (1..=u8::MAX as i64).contains(&actual) && (1..=u8::MAX as i64).contains(&normal) {
                dur.base = Frac::new(1, p2);
                dur.tuplet_actual = actual as u8;
                dur.tuplet_normal = normal as u8;
            }
        }
    }
    dur
}

/// Kern pitch: letter run (`c`=C4, `cc`=C5, `C`=C3, `CC`=C2) + accidentals.
fn parse_kern_pitch(body: &str) -> Option<Pitch> {
    let letters: Vec<char> = body
        .chars()
        .filter(|c| c.is_ascii_alphabetic() && *c != 'n' && *c != 'r')
        .collect();
    let first = *letters.first()?;
    if !letters.iter().all(|c| *c == first) {
        return None; // mixed letters — not a pitch
    }
    let step = PitchStep::from_name(&first.to_ascii_uppercase().to_string())?;
    let count = letters.len() as i32;
    let octave = if first.is_ascii_lowercase() {
        3 + count
    } else {
        4 - count
    };
    let sharps = body.matches('#').count() as i32;
    let flats = body.matches('-').count() as i32;
    Some(Pitch::with_alter(
        step,
        Alter::from_integer(sharps - flats),
        octave,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::note::VoiceElement;

    fn parse(text: &str) -> MusicDocument {
        HumdrumToIrAdapter::new()
            .convert_str_to_music(text)
            .unwrap()
    }

    fn notes_of(doc: &MusicDocument) -> Vec<(PitchStep, i32, i32)> {
        fn walk(m: &Music, out: &mut Vec<(PitchStep, i32, i32)>) {
            match m {
                Music::Note { pitch, .. } => {
                    out.push((pitch.step, pitch.alter.to_integer(), pitch.octave))
                }
                Music::Chord { pitches, .. } => {
                    for (p, _) in pitches {
                        out.push((p.step, p.alter.to_integer(), p.octave));
                    }
                }
                Music::Sequential(c) | Music::Simultaneous(c) => {
                    c.iter().for_each(|m| walk(m, out))
                }
                Music::Context { content, .. }
                | Music::Grace { content, .. }
                | Music::Tuplet { content, .. } => walk(content, out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(&doc.music, &mut out);
        out
    }

    #[test]
    fn pitches_octaves_accidentals() {
        let doc = parse("**kern\n4c\n4cc\n4C\n4CC\n4f#\n4B-\n*-\n");
        assert_eq!(
            notes_of(&doc),
            vec![
                (PitchStep::C, 0, 4),
                (PitchStep::C, 0, 5),
                (PitchStep::C, 0, 3),
                (PitchStep::C, 0, 2),
                (PitchStep::F, 1, 4),
                (PitchStep::B, -1, 3),
            ]
        );
    }

    #[test]
    fn durations_dots_and_triplets() {
        let doc = parse("**kern\n2.d\n12e\n3%2f\n*-\n");
        let mut durs = Vec::new();
        fn walk(m: &Music, out: &mut Vec<Duration>) {
            match m {
                Music::Note { duration, .. } => out.push(duration.clone()),
                Music::Sequential(c) => c.iter().for_each(|m| walk(m, out)),
                Music::Context { content, .. } => walk(content, out),
                _ => {}
            }
        }
        walk(&doc.music, &mut durs);
        assert_eq!(durs[0].base, Frac::new(1, 2));
        assert_eq!(durs[0].dots, 1);
        // recip 12 = eighth-note triplet
        assert_eq!(durs[1].base, Frac::new(1, 8));
        assert_eq!((durs[1].tuplet_actual, durs[1].tuplet_normal), (3, 2));
        // rational recip 3%2 = 2/3 whole note
        assert_eq!(durs[2].actual_duration(), Frac::new(2, 3));
    }

    #[test]
    fn chords_rests_and_ties() {
        let doc = parse("**kern\n4c 4e 4g\n4r\n[2d\n2d]\n*-\n");
        let score = crate::ir::lower::lower_to_score(&doc);
        let elems: Vec<_> = score.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();
        assert!(matches!(elems[0], VoiceElement::Chord(c) if c.notes.len() == 3));
        assert!(matches!(elems[1], VoiceElement::Rest(_)));
    }

    #[test]
    fn spines_become_parts_top_down() {
        let doc = parse("**kern\t**kern\n*I\"Bass\t*I\"Soprano\n4C\t4g\n*-\t*-\n");
        let score = crate::ir::lower::lower_to_score(&doc);
        let parts = score.parts();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "Soprano", "rightmost spine is the top part");
        assert_eq!(parts[1].name, "Bass");
    }

    #[test]
    fn interpretations_key_meter_clef_metadata() {
        let doc = parse(
            "!!!COM: Bach, Johann Sebastian\n!!!OTL: Test\n**kern\n*clefG2\n*k[f#]\n*G:\n*M3/4\n4g\n*-\n",
        );
        assert_eq!(
            doc.metadata.composer.as_deref(),
            Some("Bach, Johann Sebastian")
        );
        assert_eq!(doc.metadata.title.as_deref(), Some("Test"));
        let score = crate::ir::lower::lower_to_score(&doc);
        let attrs = score.parts()[0].measures[0].attributes.as_ref().unwrap();
        assert_eq!(attrs.key.unwrap().fifths, 1);
        assert_eq!(attrs.time.as_ref().unwrap().beats, "3");
        assert_eq!(attrs.time.as_ref().unwrap().beat_type, 4);
    }

    #[test]
    fn spine_split_is_a_clear_error() {
        let err = HumdrumToIrAdapter::new()
            .convert_str_to_music("**kern\n*^\n4c\t4e\n*v\t*v\n*-\n")
            .unwrap_err();
        assert!(err.to_string().contains("spine"), "{err}");
    }

    #[test]
    fn malformed_input_does_not_panic() {
        let a = HumdrumToIrAdapter::new();
        for bad in [
            "",
            "**kern\n",
            "**kern\n999999999999999999999c\n*-\n",
            "**kern\n4c 4x zz\n*-\n",
            "**kern\n0%0q\n*-\n",
            "!!!COM\n**kern\n=\n*-\n",
        ] {
            let _ = a.convert_str_to_music(bad);
        }
    }
}
