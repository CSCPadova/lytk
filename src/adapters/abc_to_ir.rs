//! ABC notation → IR adapter (EET1).
//!
//! Parses the first tune of an ABC file into a Layer-1 [`MusicDocument`],
//! following ABC 2.1: the `X/T/C/M/L/K/V` fields, notes (accidentals, octave
//! marks, fractional durations), rests, bar lines and repeats, chords `[...]`,
//! ties `-`, tuplets, grace notes and inline `[K:]`/`[M:]`/`[L:]` fields.
//!
//! Pitch follows the standard: an unmarked note takes the key signature (with
//! its mode and any explicit accidentals, `K:D Phr ^f`), and an accidental
//! carries to the end of the bar — on notes of the same letter in the same
//! octave by default (what abcm2ps and abc2svg do), or in every octave under
//! `%%propagate-accidentals pitch`. Uppercase `C..B` are the middle-C octave
//! (MIDI 60–71), lowercase `c..b` the octave above; `,` lowers and `'` raises
//! by an octave, and `octave=` on `K:`/`V:` shifts a voice.
//!
//! Lyric lines (`w:`, `W:`) and other field lines are not music; decorations
//! (`!...!`, legacy `+...+`) and chord symbols (`"..."`) are skipped.

use std::collections::HashMap;
use std::path::Path;

use crate::ir::annotation::Annotation;
use crate::ir::articulation::{
    Articulation, ArticulationType, DynamicMark, Fermata, FermataShape, Ornament, OrnamentType,
    Placement, Technical, TechnicalType, Wedge, WedgeType,
};
use crate::ir::direction::{
    Barline, BarlineType, Direction, RepeatDirection, TempoDirection, TextDirection,
};
use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::{parse_chord_suffix, ChordKind, ChordPitch, Harmony};
use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, TimeSignature};
use crate::ir::music::{ContextType, Music, MusicDocument};
use crate::ir::pitch::{Alter, Pitch, PitchStep};
use crate::ir::score::ScoreMetadata;

use super::{AdapterError, Result, ToIrAdapter, ToMusicAdapter};
use crate::ir::direction::{BarlineLocation, EndingType};
use crate::ir::duration::NoteType;
use crate::ir::measure::TimeSymbol;

/// Adapter that reads ABC notation.
#[derive(Default)]
pub struct AbcToIrAdapter;

impl AbcToIrAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Every tune of an ABC file, one score each (the other readers take the
    /// first). The file header — whatever precedes the first `X:` — applies
    /// to every tune; a text without `X:` is one tune.
    pub fn convert_str_tunes(&self, text: &str) -> Result<Vec<crate::ir::Score>> {
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        let starts: Vec<usize> = text
            .match_indices("X:")
            .map(|(i, _)| i)
            .filter(|&i| i == 0 || text.as_bytes()[i - 1] == b'\n')
            .collect();
        let Some(&first) = starts.first() else {
            return Ok(vec![self.convert_str(text)?]);
        };
        let header = &text[..first];
        let ends = starts.iter().skip(1).copied().chain([text.len()]);
        // A tune that doesn't read (a draft without `K:`) is skipped; the file
        // fails only when none does.
        let (tunes, errors): (Vec<_>, Vec<_>) = starts
            .iter()
            .zip(ends)
            .map(|(&a, b)| self.convert_str(&format!("{header}{}", &text[a..b])))
            .partition(|r| r.is_ok());
        match (tunes.is_empty(), errors.into_iter().next()) {
            (true, Some(Err(e))) => Err(e),
            _ => Ok(tunes.into_iter().flatten().collect()),
        }
    }

    pub fn convert_file_tunes(&self, path: &Path) -> Result<Vec<crate::ir::Score>> {
        self.convert_str_tunes(&super::decode_text_bytes(std::fs::read(path)?))
    }
}

impl ToMusicAdapter for AbcToIrAdapter {
    fn convert_file_to_music(&self, path: &Path) -> Result<MusicDocument> {
        // Older ABC collections are Latin-1.
        let text = super::decode_text_bytes(std::fs::read(path)?);
        self.convert_str_to_music(&text)
    }

    fn convert_str_to_music(&self, text: &str) -> Result<MusicDocument> {
        let doc = parse_tune(text)?;
        super::check_music_length(&doc.music)?;
        Ok(doc)
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
    /// The header's `L:` and `M:` (bar length, compound): where every voice
    /// starts. A body `L:` or `M:` changes only its voice.
    unit_length: Frac, // in whole notes
    meter: Option<(Frac, bool)>,
    /// How far an accidental carries within a bar (`%%propagate-accidentals`).
    propagate: Propagate,
    /// The header's key, octave shift and clef: where every voice starts.
    default_pitch: PitchState,
}

/// `%%propagate-accidentals`: how far an accidental carries within a bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Propagate {
    /// Only the note it is written on.
    Not,
    /// Same letter, same octave — what abcm2ps and abc2svg do, and what
    /// engraving practice expects. lytk's default.
    Octave,
    /// Same letter in every octave (the ABC 2.1 text's default).
    Pitch,
}

/// The pitch context of one voice: the key signature as an alteration per
/// letter (C..B), the `octave=` shift, and the accidentals met so far in the
/// bar, keyed by (letter, octave) — octave 0 under [`Propagate::Pitch`].
#[derive(Clone, Debug, Default)]
struct PitchState {
    key: [i32; 7],
    octave_shift: i32,
    bar: HashMap<(usize, i32), i32>,
    /// (letter, written octave, alteration) of the notes tied into the next
    /// one: a tied note keeps its pitch across the bar line.
    tied: Vec<(usize, i32, i32)>,
}

impl PitchState {
    /// The sounding alteration of `letter` in `octave`. A written accidental
    /// sets it (and, unless `grace`, carries through the bar); an unmarked note
    /// takes the bar's accidental or else the key.
    fn alteration(
        &mut self,
        letter: usize,
        octave: i32,
        written: Option<i32>,
        propagate: Propagate,
        grace: bool,
    ) -> i32 {
        let slot = (
            letter,
            if propagate == Propagate::Pitch {
                0
            } else {
                octave
            },
        );
        match written {
            Some(a) => {
                if !grace && propagate != Propagate::Not {
                    self.bar.insert(slot, a);
                }
                a
            }
            None => {
                if let Some(t) = self.tied.iter().find(|t| t.0 == letter && t.1 == octave) {
                    return t.2;
                }
                self.bar.get(&slot).copied().unwrap_or(self.key[letter])
            }
        }
    }

    /// Remember the pitches just tied into the next note.
    fn tie(&mut self, pitches: &[Pitch]) {
        self.tied = pitches
            .iter()
            .map(|p| {
                let letter = "CDEFGAB".find(p.step.name()).unwrap_or(0);
                (
                    letter,
                    p.octave - self.octave_shift,
                    *p.alter.numer() / *p.alter.denom(),
                )
            })
            .collect();
    }
}

/// One ABC voice (`V:` field) — an independent music stream that plays
/// simultaneously with the others. Maps to one Staff (→ Part on lowering).
struct VoiceStream {
    id: String,
    name: Option<String>,
    events: Vec<Music>,
    pitch: PitchState,
    /// The ending (`[1`, `|2`) being read, closed by the next `:|`, `||`,
    /// `|]`, `|:` or ending.
    open_ending: Option<u8>,
    /// This voice's `L:` and `M:` (bar length, compound meter).
    unit: Frac,
    meter: Option<(Frac, bool)>,
    /// The finished layers of a bar with `&` overlays (ABC 2.1 §7.4); the
    /// layer being read is at the end of `events`.
    overlay: Vec<Vec<Music>>,
    /// Decorations and slur starts waiting for the next note or chord.
    pending: Vec<Annotation>,
    /// Slurs open (numbers for nested ones).
    slurs: u8,
    /// Where the next `w:` line's syllables start (an index in `events`),
    /// and the start and verse number of the `w:` line just read, which a
    /// following `w:` line sings again as the next verse.
    lyric_from: usize,
    verse: Option<(usize, u8)>,
    /// Verses whose last `w:` line ended inside a word (`haj-`).
    open_words: Vec<u8>,
    /// The last note of the beam being read (an index in `events`): the
    /// next note written right after it joins it.
    beam_from: Option<usize>,
}

/// Find the voice with `id`, creating it (recording `name` if given) when absent.
/// A new voice starts from the header's key, unit and meter. Returns its index
/// in `voices`.
fn ensure_voice(
    voices: &mut Vec<VoiceStream>,
    id: &str,
    name: Option<String>,
    state: &TuneState,
) -> usize {
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
        pitch: state.default_pitch.clone(),
        open_ending: None,
        unit: state.unit_length,
        meter: state.meter,
        overlay: Vec::new(),
        pending: Vec::new(),
        slurs: 0,
        lyric_from: 0,
        verse: None,
        open_words: Vec::new(),
        beam_from: None,
    });
    voices.len() - 1
}

/// A `V:` field: the id (first token), `name=`/`nm=`, and the `clef=`/
/// `octave=` settings it carries (bare clef names count too).
struct VoiceField {
    id: String,
    name: Option<String>,
    clef: Option<Clef>,
    octave: Option<i32>,
    /// `transpose=N`: the voice sounds `N` semitones from its written notes.
    transpose: Option<i32>,
}

fn parse_voice_field(value: &str) -> VoiceField {
    let v = value.trim();
    let mut toks = v.split_whitespace();
    let id = toks.next().unwrap_or("1").to_string();
    let name = extract_param(v, "name").or_else(|| extract_param(v, "nm"));
    let mut clef = None;
    let mut octave = None;
    let mut transpose = None;
    for t in toks {
        if let Some(c) = t.strip_prefix("clef=").and_then(parse_clef) {
            clef = Some(c);
        } else if let Some(n) = t.strip_prefix("transpose=") {
            transpose = parse_transpose(n);
        } else if let Some(o) = t.strip_prefix("octave=") {
            octave = parse_octave(o);
        } else if !t.contains('=') {
            if let Some(c) = parse_clef(t) {
                clef = Some(c);
            }
        }
    }
    // `treble-8`: "the player will transpose the notes one octave lower"
    // (ABC 2.1 §4.6), unless `octave=` says otherwise.
    if octave.is_none() {
        octave = clef.map(|c| c.octave_change as i32).filter(|o| *o != 0);
    }
    VoiceField {
        id,
        name,
        clef,
        octave,
        transpose,
    }
}

/// `transpose=N`, bounded (an instrument transposes a few octaves at most).
fn parse_transpose(v: &str) -> Option<i32> {
    v.parse::<i32>().ok().map(|n| n.clamp(-48, 48))
}

/// The events a voice's `clef=` and `transpose=` settings put in its music.
fn voice_settings(clef: Option<Clef>, transpose: Option<i32>) -> Vec<Music> {
    let mut out: Vec<Music> = clef.into_iter().map(Music::Clef).collect();
    out.extend(
        transpose.map(|n| Music::Transposition(crate::ir::measure::Transpose::from_semitones(n))),
    );
    out
}

/// Pull `key=value` (or `key="quoted value"`) from an ABC field parameter list.
/// The key must start a token, so `name=` doesn't match `subname=`.
fn extract_param(s: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=");
    let start = s
        .match_indices(&pat)
        .map(|(i, _)| i)
        .find(|&i| i == 0 || s[..i].ends_with(char::is_whitespace))?
        + pat.len();
    let rest = &s[start..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        Some(rest.split_whitespace().next().unwrap_or("").to_string())
    }
}

/// Split a music line at its inline `[V:…]` markers: the music before the
/// first marker (voice unchanged), then each marker's value and its music.
fn split_inline_voices(line: &str) -> Vec<(Option<&str>, &str)> {
    // Markers in quoted text or after a `%` comment don't count.
    let mut marks = Vec::new();
    let mut quoted = false;
    for (k, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '%' if !quoted => break,
            '[' if !quoted && line[k..].starts_with("[V:") => {
                if let Some(end) = line[k..].find(']') {
                    marks.push((k, k + end));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let (mut switch, mut from) = (None, 0);
    for (k, end) in marks {
        if k < from {
            continue;
        }
        out.push((switch, &line[from..k]));
        switch = Some(&line[k + 3..end]);
        from = end + 1;
    }
    out.push((switch, &line[from..]));
    out
}

/// A `V:` field in the body (own line or inline): switch to that voice,
/// applying its octave and clef. Returns the voice's index.
fn switch_voice(voices: &mut Vec<VoiceStream>, value: &str, state: &TuneState) -> usize {
    let vf = parse_voice_field(value);
    let i = ensure_voice(voices, &vf.id, vf.name, state);
    if let Some(o) = vf.octave {
        voices[i].pitch.octave_shift = o;
    }
    voices[i]
        .events
        .extend(voice_settings(vf.clef, vf.transpose));
    i
}

fn parse_tune(text: &str) -> Result<MusicDocument> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut metadata = ScoreMetadata::default();
    let mut state = TuneState {
        unit_length: Frac::new(1, 8),
        meter: None,
        propagate: Propagate::Octave,
        default_pitch: PitchState::default(),
    };
    // Shared header signatures (M:/K:), prepended to every voice so each lowers
    // to a Part with the right attributes.
    let mut header_events: Vec<Music> = Vec::new();
    let mut voices: Vec<VoiceStream> = Vec::new();
    // Clefs and octave shifts declared by header `V:` lines, applied once the
    // header's `K:` has set the key each voice starts from.
    let mut header_voice_settings: Vec<VoiceField> = Vec::new();
    let mut current: usize = 0;
    let mut in_body = false;
    let mut seen_x = false;
    let mut explicit_unit_length = false;
    let mut header_tempo: Option<TempoDirection> = None;
    // The last body line was a `w:` line (the next one is another verse).
    let mut after_w = false;
    // The last music line ended with `\`: this one goes on with it.
    let mut continued = false;

    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(directive) = line.strip_prefix("%%") {
            apply_directive(directive, &mut state);
            continue;
        }
        if line.trim().is_empty() {
            // An empty line ends the tune body (ABC 2.1 §2.2.1); free text may
            // follow before the next tune.
            if in_body {
                break;
            }
            continue;
        }
        if line.starts_with('%') {
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
                    state.meter = m.as_ref().map(meter_length);
                    header_events.extend(m.map(Music::TimeSignature));
                }
                'L' => {
                    if let Some(f) = parse_fraction(value).filter(|f| *f > Frac::from_integer(0)) {
                        state.unit_length = f;
                        explicit_unit_length = true;
                    }
                }
                'V' => {
                    // Voice declaration in the header (ABC 2.1 §4.1): set up the
                    // voice (and its name) ahead of the body.
                    let vf = parse_voice_field(value);
                    ensure_voice(&mut voices, &vf.id, vf.name.clone(), &state);
                    header_voice_settings.push(vf);
                }
                'K' => {
                    let k = parse_key_field(value);
                    if let Some(sig) = k.signature {
                        header_events.push(Music::KeySignature(sig));
                    }
                    header_events.extend(voice_settings(k.clef, k.transpose));
                    apply_key(&mut state.default_pitch, &k);
                    // Default unit length depends on the meter when L: is absent.
                    if !explicit_unit_length {
                        state.unit_length = default_unit_length(state.meter.map(|m| m.0));
                    }
                    // Voices declared in the header start from this key, unit
                    // and meter, plus their own clef and octave.
                    for v in voices.iter_mut() {
                        v.pitch = state.default_pitch.clone();
                        v.unit = state.unit_length;
                        v.meter = state.meter;
                        if let Some(vf) = header_voice_settings.iter().find(|vf| vf.id == v.id) {
                            if let Some(o) = vf.octave {
                                v.pitch.octave_shift = o;
                            }
                            v.events.extend(voice_settings(vf.clef, vf.transpose));
                        }
                    }
                    // K: ends the header; the rest is the tune body.
                    in_body = true;
                    seen_x = true;
                }
                'X' => {
                    seen_x = true;
                    metadata.extra.insert("X".to_string(), value.to_string());
                }
                'I' => apply_directive(value, &mut state),
                // The tempo opens the first voice (once, not in every part).
                'Q' => header_tempo = parse_tempo(value, state.unit_length),
                other => {
                    metadata.extra.insert(other.to_string(), value.to_string());
                }
            }
            continue;
        }

        if in_body {
            if is_field_line(line) {
                let (key, value) = split_field(line);
                match key {
                    // A second tune starts: this reader returns the first.
                    'X' if seen_x => break,
                    // A `V:` info field on its own line switches the active voice.
                    'V' => current = switch_voice(&mut voices, value, &state),
                    'I' => apply_directive(value, &mut state),
                    // Other `K:`/`M:`/`L:`/`Q:` fields apply to the current voice.
                    'K' | 'M' | 'L' | 'Q' => {
                        if voices.is_empty() {
                            current = ensure_voice(&mut voices, "1", None, &state);
                        }
                        apply_inline_field(key, value, &mut state, &mut voices[current]);
                    }
                    // Lyrics under the notes just read; a second `w:` line
                    // right after is the next verse.
                    'w' if !voices.is_empty() => {
                        align_lyrics(&mut voices[current], value, after_w);
                        after_w = true;
                        continue;
                    }
                    // Words printed after the tune.
                    'W' => {
                        let words = metadata.extra.entry("W".to_string()).or_default();
                        if !words.is_empty() {
                            words.push('\n');
                        }
                        words.push_str(value);
                    }
                    // Symbol lines, remarks, parts, notes, continuations: not
                    // music.
                    _ => {}
                }
                after_w = false;
                continue;
            }
            after_w = false;
            // A `w:` line is sung on the music line above it (ABC 2.1 §5.1),
            // with the lines a `\` joins to it.
            let mut started: Vec<usize> = Vec::new();
            // Inline `[V:id]` markers switch voices anywhere in the line.
            for (switch, music) in split_inline_voices(line) {
                if let Some(value) = switch {
                    current = switch_voice(&mut voices, value, &state);
                }
                if voices.is_empty() {
                    // Nothing before the line's first `[V:]` switch.
                    if music.trim().is_empty() {
                        continue;
                    }
                    current = ensure_voice(&mut voices, "1", None, &state);
                }
                if !continued && !started.contains(&current) {
                    voices[current].lyric_from = voices[current].events.len();
                    started.push(current);
                }
                parse_body_line(music, &mut state, &mut voices[current]);
            }
            continued = line
                .split('%')
                .next()
                .unwrap_or("")
                .trim_end()
                .ends_with('\\');
        }
    }
    for v in &mut voices {
        end_overlay(v);
        // An ending still open when the tune ends stops there.
        if v.open_ending.is_some()
            && !matches!(v.events.last(), Some(Music::Barline(b)) if b.ending_type.is_none())
        {
            v.events.push(Music::Barline(Barline::default()));
        }
        close_ending(v, None);
        mark_irregular_bars(&mut v.events, state.meter.map(|m| m.0));
    }

    if !in_body {
        return Err(AdapterError::Parse(
            "ABC tune has no K: line (no body)".to_string(),
        ));
    }

    // A first bar shorter than the meter is a pickup.
    if let (Some((meter, _)), Some(first)) = (state.meter, first_bar_length(&voices)) {
        if first < meter {
            metadata.partial_duration = Some(Duration::new(first));
        }
    }
    if let (Some(t), Some(first)) = (header_tempo, voices.first_mut()) {
        first.events.insert(0, Music::Tempo(t));
    }
    let music = build_music(header_events, voices);
    Ok(MusicDocument { metadata, music })
}

/// `%%propagate-accidentals not|octave|pitch` (also written
/// `I:propagate-accidentals …`); other directives are layout.
fn apply_directive(directive: &str, state: &mut TuneState) {
    let mut t = directive.split_whitespace();
    if t.next() == Some("propagate-accidentals") {
        state.propagate = match t.next() {
            Some("not") => Propagate::Not,
            Some("pitch") => Propagate::Pitch,
            _ => Propagate::Octave,
        };
    }
}

/// How long the first bar of the first voice with music lasts (up to its
/// first bar line; a bar line before any music doesn't count). `None` without
/// a bar line.
fn first_bar_length(voices: &[VoiceStream]) -> Option<Frac> {
    let voice = voices.iter().find(|v| {
        v.events.iter().any(|m| {
            matches!(
                m,
                Music::Note { .. }
                    | Music::Chord { .. }
                    | Music::Tuplet { .. }
                    | Music::Simultaneous(_)
            )
        })
    })?;
    let mut total = Frac::from_integer(0);
    for m in &voice.events {
        match m {
            Music::Barline(_) if total > Frac::from_integer(0) => return Some(total),
            _ => total += m.written_length(),
        }
    }
    None
}

/// ABC bar lines are real bar lines (a LilyPond bar check only checks): a
/// bar longer or shorter than its meter starts with a `Music::Partial` of its
/// length, so the lowering keeps it whole. A short first bar is the pickup
/// (`partial_duration`); a bar with a meter change inside is cut there anyway;
/// the last bar ends with the music. Without a meter (`M:none`, no `M:`)
/// every bar is its own length, the last one too.
fn mark_irregular_bars(events: &mut Vec<Music>, mut meter: Option<Frac>) {
    let zero = Frac::from_integer(0);
    let mut marks = Vec::new(); // (index, length)
    let (mut start, mut len, mut first, mut steady) = (0, zero, true, true);
    for (k, m) in events.iter().enumerate() {
        match m {
            Music::Barline(_) => {
                if len > zero {
                    let irregular = match meter {
                        Some(bar) => len != bar && !(first && len < bar),
                        None => true,
                    };
                    if steady && irregular {
                        marks.push((start, len));
                    }
                    first = false;
                }
                (start, len, steady) = (k + 1, zero, true);
            }
            Music::TimeSignature(ts) => {
                meter = Some(ts.beats_fraction());
                steady &= len == zero;
            }
            _ => len += m.written_length(),
        }
    }
    if meter.is_none() && steady && len > zero {
        marks.push((start, len));
    }
    for (k, d) in marks.into_iter().rev() {
        events.insert(k, Music::Partial(Duration::new(d)));
    }
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

/// In the tune body, a letter (either case) or `+` followed by `:` starts a
/// field line: `w:` lyrics, `s:` symbols, `+:` continuations, `K:`, `V:`, ….
fn is_field_line(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 2 && b[1] == b':' && (b[0].is_ascii_alphabetic() || b[0] == b'+')
}

fn split_field(line: &str) -> (char, &str) {
    let key = line.chars().next().unwrap();
    let value = line[2..].trim();
    // Strip trailing inline comment (`\%` is a percent sign).
    let cut = value
        .char_indices()
        .find(|&(i, c)| c == '%' && !value[..i].ends_with('\\'))
        .map_or(value.len(), |(i, _)| i);
    (key, value[..cut].trim())
}

/// Apply a `K:`/`M:`/`L:` field met in the body (on its own line or inline as
/// `[K:…]`) to the current voice.
fn apply_inline_field(key: char, value: &str, state: &mut TuneState, voice: &mut VoiceStream) {
    match key {
        'M' => {
            if let Some(ts) = parse_meter(value) {
                voice.meter = Some(meter_length(&ts));
                voice.events.push(Music::TimeSignature(ts));
            }
        }
        'L' => {
            if let Some(f) = parse_fraction(value).filter(|f| *f > Frac::from_integer(0)) {
                voice.unit = f;
            }
        }
        'K' => {
            let k = parse_key_field(value);
            if let Some(sig) = k.signature {
                voice.events.push(Music::KeySignature(sig));
            }
            voice.events.extend(voice_settings(k.clef, k.transpose));
            apply_key(&mut voice.pitch, &k);
        }
        'Q' => voice
            .events
            .extend(parse_tempo(value, voice.unit).map(Music::Tempo)),
        'I' => apply_directive(value, state),
        _ => {}
    }
}

/// A `Q:` field: `1/4=120`, `"Allegro" 3/8=80`, `"Andante"`, or the old
/// `120` (in `L:` units).
fn parse_tempo(value: &str, unit: Frac) -> Option<TempoDirection> {
    let mut text = None;
    let mut rest = value.trim().to_string();
    if let Some(open) = rest.find('"') {
        if let Some(len) = rest[open + 1..].find('"') {
            text = Some(rest[open + 1..open + 1 + len].to_string());
            rest = format!("{}{}", &rest[..open], &rest[open + 2 + len..]);
        }
    }
    let rest = rest.trim();
    let (beat, bpm) = match rest.split_once('=') {
        Some((b, n)) => (
            b.split_whitespace()
                .filter_map(parse_fraction)
                .sum::<Frac>(),
            n.trim().parse::<f64>().ok(),
        ),
        None => (unit, rest.parse::<f64>().ok()),
    };
    if text.is_none() && bpm.is_none() {
        return None;
    }
    // The beat as a note value, dotted when it is 3/2 of one.
    let (base, dots) = if (*beat.numer() as u64).is_power_of_two() || *beat.numer() == 1 {
        (beat, 0)
    } else {
        (beat * Frac::new(2, 3), 1)
    };
    let name = NoteType::from_length(base);
    Some(TempoDirection {
        text,
        beat_unit: bpm.and(name),
        per_minute: bpm,
        dots: if bpm.is_some() { dots } else { 0 },
        placement: Placement::Above,
    })
}

/// A `w:` line's syllables on the voice's notes from where the last one
/// stopped (ABC 2.1 §5.1): `-` splits a word, `_` holds the last syllable over
/// a note, `*` skips one, `~` joins words under one note, `\-` is a hyphen,
/// `|` goes on to the next bar. `again`: the line before was a `w:` line too,
/// so this is its next verse, under the same notes.
fn align_lyrics(voice: &mut VoiceStream, line: &str, again: bool) {
    use crate::ir::articulation::{LyricSyllable, SyllabicType};
    enum Tok {
        Syl(String, bool),
        Hold,
        Skip,
        Bar,
    }
    let (from, verse) = match (again, voice.verse) {
        (true, Some((f, v))) => (f, v.saturating_add(1)),
        _ => (voice.lyric_from, 1),
    };
    let from = from.min(voice.events.len());
    let mut toks = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, toks: &mut Vec<Tok>, hyphen: bool| {
        if !cur.is_empty() {
            toks.push(Tok::Syl(std::mem::take(cur), hyphen));
        } else if hyphen {
            toks.push(Tok::Skip);
        }
    };
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if !cur.is_empty() {
                    flush(&mut cur, &mut toks, false);
                }
            }
            '-' => flush(&mut cur, &mut toks, true),
            '_' | '*' | '|' => {
                if !cur.is_empty() {
                    flush(&mut cur, &mut toks, false);
                }
                toks.push(match c {
                    '_' => Tok::Hold,
                    '*' => Tok::Skip,
                    _ => Tok::Bar,
                });
            }
            '~' => cur.push(' '),
            // `\-`, `\_`, `\*`, …: the sign itself.
            '\\' => cur.extend(chars.next()),
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut toks, false);

    // Notes and bar lines from `from`, in order: `true` for a note.
    fn slots(events: &mut [Music], f: &mut dyn FnMut(Option<&mut Vec<Annotation>>)) {
        for m in events {
            match m {
                Music::Note { annotations, .. } | Music::Chord { annotations, .. } => {
                    f(Some(annotations))
                }
                Music::Barline(_) => f(None),
                Music::Tuplet { content, .. } => {
                    if let Music::Sequential(inner) = content.as_mut() {
                        slots(inner, f);
                    }
                }
                Music::Simultaneous(layers) => {
                    if let Some(Music::Sequential(first)) = layers.first_mut() {
                        slots(first, f);
                    }
                }
                _ => {}
            }
        }
    }
    let mut kinds: Vec<bool> = Vec::new();
    slots(&mut voice.events[from..], &mut |a| kinds.push(a.is_some()));
    let notes = kinds.iter().filter(|k| **k).count();
    let mut sung: Vec<Option<LyricSyllable>> = vec![None; notes];
    let (mut slot, mut note) = (0, 0);
    // A word a verse's last line left open goes on here.
    let mut hyphen = voice.open_words.contains(&verse);
    let mut last: Option<usize> = None;
    for tok in toks {
        if let Tok::Bar = tok {
            // On to the note after the next bar line.
            while slot < kinds.len() && kinds[slot] {
                slot += 1;
                note += 1;
            }
            slot += 1;
            continue;
        }
        while slot < kinds.len() && !kinds[slot] {
            slot += 1;
        }
        if slot >= kinds.len() {
            break;
        }
        match tok {
            Tok::Syl(text, h) => {
                let syllabic = match (hyphen, h) {
                    (false, false) => SyllabicType::Single,
                    (false, true) => SyllabicType::Begin,
                    (true, true) => SyllabicType::Middle,
                    (true, false) => SyllabicType::End,
                };
                sung[note] = Some(LyricSyllable {
                    elision: text.contains('\u{203F}'),
                    text,
                    syllabic,
                    number: verse,
                    extend: false,
                    name: None,
                });
                (hyphen, last) = (h, Some(note));
            }
            Tok::Hold => {
                if let Some(s) = last.and_then(|l| sung[l].as_mut()) {
                    s.extend = true;
                }
            }
            Tok::Skip | Tok::Bar => {}
        }
        slot += 1;
        note += 1;
    }
    let mut k = 0;
    slots(&mut voice.events[from..], &mut |a| {
        if let Some(anns) = a {
            if let Some(s) = sung.get_mut(k).and_then(Option::take) {
                anns.push(Annotation::Lyric(s));
            }
            k += 1;
        }
    });
    voice.verse = Some((from, verse));
    voice.lyric_from = voice.events.len();
    voice.open_words.retain(|v| *v != verse);
    if hyphen {
        voice.open_words.push(verse);
    }
}

// ---------------------------------------------------------------------------
// Header value parsing
// ---------------------------------------------------------------------------

/// Parse `M:` into a time signature. `C` → 4/4, `C|` → 2/2; an additive
/// numerator (`2+3+2/8`, `(2+3+2)/8`) keeps its groups.
fn parse_meter(value: &str) -> Option<TimeSignature> {
    let v = value.trim();
    let (n, d) = match v {
        "C" => ("4", "4"),
        "C|" => ("2", "2"),
        _ => v.split_once('/')?,
    };
    let groups: Vec<u32> = n
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split('+')
        .map(|p| p.trim().parse::<u32>().ok())
        .collect::<Option<_>>()?;
    let num: u32 = groups.iter().try_fold(0u32, |a, &g| a.checked_add(g))?;
    let den: u8 = d.trim().parse().ok()?;
    // Reject a zero numerator or denominator: both are musically meaningless and
    // a 0 denominator panics `Frac::new` downstream (mirrors the guard in
    // `parse_fraction`). A crafted `M:4/0` must not crash the parser, and a
    // numerator past `u8` (`M:256/4`) must not wrap to a zero-length bar.
    if num == 0 || den == 0 || num > u8::MAX as u32 {
        return None;
    }
    let beats: Vec<String> = groups.iter().map(u32::to_string).collect();
    Some(TimeSignature {
        beats: beats.join("+"),
        beat_type: den,
        symbol: meter_symbol(v),
    })
}

/// A meter's bar length, and whether it is compound (6/8, 9/8, 12/8: a
/// bare `(5` then means 5 in the time of 3).
fn meter_length(ts: &TimeSignature) -> (Frac, bool) {
    (ts.beats_fraction(), ts.is_compound())
}

fn meter_symbol(value: &str) -> Option<TimeSymbol> {
    match value.trim() {
        "C" => Some(TimeSymbol::Common),
        "C|" => Some(TimeSymbol::Cut),
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

/// A `K:` field: the signature to show, the alteration of each letter (C..B)
/// it implies — explicit accidentals included — and any octave shift or clef.
/// `alters` is `None` when the field sets no key (`K:clef=bass`, `K:bass`):
/// the key in force stays.
#[derive(Debug, Default)]
struct KeyField {
    signature: Option<KeySignature>,
    alters: Option<[i32; 7]>,
    octave: Option<i32>,
    clef: Option<Clef>,
    transpose: Option<i32>,
}

/// Parse a `K:` value: `G`, `F#m`, `Bb mix`, `D Phr ^f`, `D exp _b _e ^f`,
/// `none`, `HP`/`Hp`, plus `clef=`, `octave=` and the other voice settings.
fn parse_key_field(value: &str) -> KeyField {
    let mut out = KeyField::default();
    let mut toks = value.split_whitespace().peekable();
    if let Some(&first) = toks.peek() {
        if first.eq_ignore_ascii_case("none") {
            toks.next();
            out.alters = Some([0; 7]);
            out.signature = Some(KeySignature::default());
        } else if first == "HP" || first == "Hp" {
            // Highland pipes: HP shows no signature, Hp marks F♯ and C♯.
            toks.next();
            let mut a = [0; 7];
            let mut sig = KeySignature::default();
            if first == "Hp" {
                a[0] = 1;
                a[3] = 1;
                sig.fifths = 2;
            }
            out.alters = Some(a);
            out.signature = Some(sig);
        } else if let Some(sig) = parse_tonic(first, &mut toks) {
            out.alters = Some(signature_alters(sig.fifths as i32));
            out.signature = Some(sig);
        }
    }
    for t in toks {
        if t == "exp" {
            out.alters = Some([0; 7]);
        } else if let Some(o) = t.strip_prefix("octave=") {
            out.octave = parse_octave(o);
        } else if let Some(c) = t.strip_prefix("clef=") {
            out.clef = parse_clef(c).or(out.clef);
        } else if let Some(n) = t.strip_prefix("transpose=") {
            out.transpose = parse_transpose(n);
        } else if t.contains('=') && !t.starts_with('=') {
            // middle=, stafflines=, …: layout only.
        } else if let Some(c) = parse_clef(t) {
            out.clef = Some(c);
        } else if let Some((letter, alter)) = parse_key_accidental(t) {
            out.alters.get_or_insert(signature_alters(
                out.signature.map_or(0, |k| k.fifths as i32),
            ))[letter] = alter;
        }
    }
    // A `-8`/`+8` clef transposes what is played (ABC 2.1 §4.6).
    if out.octave.is_none() {
        out.octave = out.clef.map(|c| c.octave_change as i32).filter(|o| *o != 0);
    }
    out
}

/// `octave=N`, bounded (a real voice moves a few octaves at most).
fn parse_octave(v: &str) -> Option<i32> {
    v.parse::<i32>().ok().map(|o| o.clamp(-4, 4))
}

/// The tonic and mode (`G`, `F#m`, `Bbmix`, or `D` followed by `Phr`).
fn parse_tonic<'a>(
    first: &str,
    toks: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
) -> Option<KeySignature> {
    if first.contains('=') {
        return None;
    }
    let mut chars = first.chars().peekable();
    let letter = *chars.peek()?;
    if !('A'..='G').contains(&letter) {
        return None;
    }
    chars.next();
    // Tonic position on the circle of fifths (as a major key).
    let mut tonic_fifths: i8 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => -1,
        'G' => 1,
        'A' => 3,
        _ => 5, // B
    };
    match chars.peek() {
        Some('#') => {
            tonic_fifths += 7;
            chars.next();
        }
        Some('b') => {
            tonic_fifths -= 7;
            chars.next();
        }
        _ => {}
    }
    toks.next();
    let mut rest: String = chars.collect::<String>().to_lowercase();
    if rest.is_empty() {
        // The mode may be a separate word: `K:D Phr`.
        if let Some(&next) = toks.peek() {
            let m = next.to_lowercase();
            if mode_word(&m).is_some() {
                rest = m;
                toks.next();
            }
        }
    }
    let (mode, offset) = mode_from_str(&rest);
    Some(KeySignature {
        fifths: clamp_fifths(tonic_fifths + offset),
        mode,
    })
}

/// A mode word (`maj`, `m`, `min`, `dorian`, …), matched on its first three
/// letters as ABC 2.1 allows.
fn mode_word(s: &str) -> Option<(KeyMode, i8)> {
    let head: String = s.chars().take(3).collect();
    Some(match (s, head.as_str()) {
        ("m", _) => (KeyMode::Minor, -3),
        (_, "maj" | "ion") => (KeyMode::Major, 0),
        (_, "min" | "aeo") => (KeyMode::Minor, -3),
        (_, "dor") => (KeyMode::Dorian, -2),
        (_, "phr") => (KeyMode::Phrygian, -4),
        (_, "lyd") => (KeyMode::Lydian, 1),
        (_, "mix") => (KeyMode::Mixolydian, -1),
        (_, "loc") => (KeyMode::Locrian, -5),
        _ => return None,
    })
}

/// An explicit key accidental: `^f`, `_b`, `=c`, `^^g`, `__e`.
fn parse_key_accidental(t: &str) -> Option<(usize, i32)> {
    let acc_len = t
        .chars()
        .take_while(|c| matches!(c, '^' | '_' | '='))
        .count();
    let alter = match &t[..acc_len] {
        "^" => 1,
        "^^" => 2,
        "_" => -1,
        "__" => -2,
        "=" => 0,
        _ => return None,
    };
    let mut rest = t[acc_len..].chars();
    let letter = letter_index(rest.next()?)?;
    rest.next().is_none().then_some((letter, alter))
}

/// Alteration of each letter C..B under a key with `fifths` sharps (negative:
/// flats).
fn signature_alters(fifths: i32) -> [i32; 7] {
    const SHARPS: [usize; 7] = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B
    let mut a = [0; 7];
    for k in 0..fifths.unsigned_abs() as usize {
        if fifths > 0 {
            a[SHARPS[k % 7]] += 1;
        } else {
            a[SHARPS[6 - k % 7]] -= 1;
        }
    }
    a
}

fn letter_index(c: char) -> Option<usize> {
    "CDEFGAB".find(c.to_ascii_uppercase())
}

/// Apply a parsed `K:` field to a voice's pitch context.
fn apply_key(pitch: &mut PitchState, k: &KeyField) {
    if let Some(a) = k.alters {
        pitch.key = a;
    }
    if let Some(o) = k.octave {
        pitch.octave_shift = o;
    }
}

/// A clef name: `treble`, `bass`, `alto`, `tenor`, `perc`, optionally with a
/// line number (`bass3`) or octave suffix (`treble-8`, `bass+8`).
fn parse_clef(name: &str) -> Option<Clef> {
    let (name, octave_change) = if let Some(n) = name.strip_suffix("-8") {
        (n, -1)
    } else if let Some(n) = name.strip_suffix("+8") {
        (n, 1)
    } else {
        (name, 0)
    };
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let line = name[base.len()..].parse::<u8>().ok();
    let (sign, default_line) = match base {
        "treble" => (ClefSign::G, 2),
        "bass" => (ClefSign::F, 4),
        "alto" => (ClefSign::C, 3),
        "tenor" => (ClefSign::C, 4),
        "perc" => (ClefSign::Percussion, 3),
        _ => return None,
    };
    Some(Clef {
        sign,
        line: line.unwrap_or(default_line),
        octave_change,
    })
}

/// Map an ABC mode suffix to (KeyMode, fifths offset from the major key).
fn mode_from_str(s: &str) -> (KeyMode, i8) {
    let s = s.trim();
    if s.is_empty() {
        return (KeyMode::Major, 0);
    }
    mode_word(s).unwrap_or(if s.starts_with('m') {
        (KeyMode::Minor, -3)
    } else {
        (KeyMode::Major, 0)
    })
}

fn clamp_fifths(f: i8) -> i8 {
    f.clamp(-7, 7)
}

// ---------------------------------------------------------------------------
// Body parsing
// ---------------------------------------------------------------------------

fn parse_body_line(line: &str, state: &mut TuneState, voice: &mut VoiceStream) {
    parse_music(line, state, voice, false);
}

/// Parse one line of music into `voice`. Inside a grace group (`grace`),
/// accidentals don't carry through the bar.
fn parse_music(line: &str, state: &mut TuneState, voice: &mut VoiceStream, grace: bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    // A line's end ends a beam.
    voice.beam_from = None;
    // Open tuplet: (actual, normal, sounding events still to collect, start index).
    let mut tuplet: Option<(u8, u8, usize, usize)> = None;
    // Broken rhythm (`A>B`): the factor the next note, chord or rest takes.
    let mut broken: Option<Frac> = None;
    while i < chars.len() {
        let c = chars[i];
        let before = voice.events.len();
        match c {
            '(' if chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                // Tuplet `(p`, `(p:q`, `(p:q:r`.
                close_tuplet(&mut tuplet, &mut voice.events);
                let compound = voice.meter.is_some_and(|m| m.1);
                let ((p, q, r), next) = parse_tuplet_spec(&chars, i + 1, compound);
                tuplet = Some((p, q, r, voice.events.len()));
                i = next;
            }
            ' ' | '\t' => i += 1,
            '%' => break, // rest of line is a comment
            '[' if chars.get(i + 2) == Some(&':')
                && chars.get(i + 1).is_some_and(|k| k.is_ascii_alphabetic()) =>
            {
                // Inline field `[K:…]`, `[M:…]`, `[L:…]`.
                let end = chars[i..]
                    .iter()
                    .position(|c| *c == ']')
                    .map_or(chars.len(), |k| i + k);
                let value: String = chars[(i + 3).min(end)..end].iter().collect();
                apply_inline_field(chars[i + 1], value.trim(), state, voice);
                i = end + 1;
            }
            '|' | ':' | '[' if is_barline_at(&chars, i) => {
                let (mut bar, next) = parse_barline(&chars, i);
                // A repeat sign, a thick bar or a new repeat closes an open
                // ending.
                if bar.style != BarlineType::Regular {
                    close_ending(voice, Some(&mut bar));
                }
                end_overlay(voice);
                voice.events.push(Music::Barline(bar));
                // An accidental lasts to the end of its bar.
                voice.pitch.bar.clear();
                i = next;
                // `|1`, `:|2`: an ending starts after the bar line.
                if chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
                    open_ending(voice, &chars, &mut i);
                }
            }
            '[' if chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                // `[1`, `[2`, `[1,3`, `[1-3`: an ending.
                i += 1;
                open_ending(voice, &chars, &mut i);
            }
            '"' => {
                // A chord symbol (`"Am7"`, `"G/B"`) or an annotation
                // (`"^dolce"`: above, `_` below, `<` `>` `@` placed freely).
                let end = chars[i + 1..]
                    .iter()
                    .position(|x| *x == '"')
                    .map_or(chars.len(), |k| i + 1 + k);
                let text: String = chars[i + 1..end].iter().collect();
                if !grace {
                    voice.events.extend(quoted(&text));
                }
                i = end + 1;
            }
            '!' | '+' => {
                // Decoration `!trill!` (legacy `+trill+`). In ABC 2.0 a lone
                // `!` was a line break: without a plausible decoration name
                // before the next `!`, skip just the mark.
                match chars[i + 1..].iter().position(|x| *x == c) {
                    Some(k) if is_decoration_name(&chars[i + 1..i + 1 + k]) => {
                        let name: String = chars[i + 1..i + 1 + k].iter().collect();
                        voice.pending.extend(decoration(&name));
                        i += k + 2;
                    }
                    _ => i += 1,
                }
            }
            '.' | '~' | 'H' | 'L' | 'M' | 'O' | 'P' | 'S' | 'T' | 'u' | 'v' => {
                // One-letter decorations (ABC 2.1 §4.14).
                voice.pending.extend(decoration(&c.to_string()));
                i += 1;
            }
            '(' if chars.get(i + 1) != Some(&'&') => {
                voice.slurs = voice.slurs.saturating_add(1);
                voice.pending.push(Annotation::SlurStart {
                    number: voice.slurs,
                    placement: Placement::Unspecified,
                });
                i += 1;
            }
            ')' => {
                if voice.slurs > 0 {
                    attach(
                        &mut voice.events,
                        Annotation::SlurStop {
                            number: voice.slurs,
                        },
                    );
                    voice.slurs -= 1;
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
                let body: String = chars[j..end].iter().collect();
                let outer = std::mem::take(&mut voice.events);
                parse_music(&body, state, voice, true);
                let inner = std::mem::replace(&mut voice.events, outer);
                if !inner.is_empty() {
                    voice.events.push(Music::Grace {
                        content: Box::new(Music::Sequential(inner)),
                        slash,
                    });
                }
                i = end + 1;
            }
            '[' => {
                // Chord [CEG].
                let (chord, next, tied) = parse_chord(&chars, i, state, voice, grace);
                if let Some(mut ch) = chord {
                    if !grace {
                        take_pending(voice, &mut ch);
                    }
                    voice.events.push(ch);
                    if !grace {
                        voice.pitch.tie(&tied);
                    }
                }
                i = next;
            }
            'z' | 'x' => {
                // `x` is an invisible rest: a skip.
                voice.pitch.tied.clear();
                let (duration, next) = parse_duration(&chars, i + 1, voice.unit);
                voice.events.push(if c == 'x' {
                    Music::Skip { duration }
                } else {
                    Music::Rest {
                        duration,
                        is_measure_rest: false,
                    }
                });
                i = next;
            }
            'Z' | 'X' => {
                // `Z`, `Z4`: rests of whole bars; `X`: invisible ones.
                voice.pitch.tied.clear();
                i += 1;
                let mut bars: u32 = 0;
                while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                    bars = (bars * 10 + d).min(MAX_BAR_RESTS);
                    i += 1;
                }
                // Without a meter (`M:none`) a bar is taken as a whole note.
                let bar = Duration::new(voice.meter.map_or(Frac::from_integer(1), |m| m.0));
                for k in 0..bars.max(1) {
                    // Each its own bar.
                    if k > 0 {
                        voice.events.push(Music::Barline(Barline::default()));
                    }
                    voice.events.push(if c == 'X' {
                        Music::Skip {
                            duration: bar.clone(),
                        }
                    } else {
                        Music::Rest {
                            duration: bar.clone(),
                            is_measure_rest: true,
                        }
                    });
                }
            }
            '>' | '<' => {
                // Broken rhythm: `>` dots the note before and halves the one
                // after, `>>` double-dots and quarters, `<` the other way.
                let mut n = 0;
                while chars.get(i) == Some(&c) {
                    n += 1;
                    i += 1;
                }
                let short = Frac::new(1, 1 << n.min(6));
                let long = Frac::from_integer(2) - short;
                let (prev, next) = if c == '>' {
                    (long, short)
                } else {
                    (short, long)
                };
                if let Some(m) = last_timed(&mut voice.events) {
                    scale_duration(m, prev);
                }
                broken = Some(next);
            }
            '&' => {
                // Overlay: what follows sounds from the start of the bar.
                close_tuplet(&mut tuplet, &mut voice.events);
                let start = bar_start(&voice.events);
                let layer = voice.events.split_off(start);
                voice.overlay.push(layer);
                voice.pitch.tied.clear();
                i += 1;
            }
            '^' | '_' | '=' | 'A'..='G' | 'a'..='g' => {
                let (note, next) = parse_note(&chars, i, state, voice, grace);
                if let Some(mut n) = note {
                    if !grace {
                        take_pending(voice, &mut n);
                    }
                    voice.events.push(n);
                    if !grace {
                        voice.pitch.tied.clear();
                    }
                }
                i = next;
            }
            '-' => {
                // Tie: attach a TieStart to the previous note (every note of a
                // chord; the last note of a tuplet).
                let tied = attach_tie(&mut voice.events);
                voice.pitch.tie(&tied);
                i += 1;
            }
            _ => i += 1, // skip unsupported tokens
        }
        if broken.is_some() {
            let start = before.min(voice.events.len());
            if let Some(m) = voice.events[start..].iter_mut().find(|m| is_sounding(m)) {
                scale_duration(m, broken.take().unwrap_or(Frac::from_integer(1)));
            }
        }
        if let Some((_, _, rem, _)) = &mut tuplet {
            let n = voice.events[before.min(voice.events.len())..]
                .iter()
                .filter(|m| is_sounding(m))
                .count();
            *rem = rem.saturating_sub(n);
        }
        if !grace {
            beam_by_spacing(voice, before, c);
        }
        if matches!(tuplet, Some((_, _, 0, _))) {
            close_tuplet(&mut tuplet, &mut voice.events);
            // The tuplet's notes moved into it: a beam goes no further.
            voice.beam_from = None;
        }
    }
    // ponytail: a tuplet left open at end of line is closed here; ABC allows a
    // tuplet to span a line break, but that is vanishingly rare in real tunes.
    close_tuplet(&mut tuplet, &mut voice.events);
}

/// What a decoration puts on the next note (ABC 2.1 §4.14); `None` for the
/// ones the IR has no place for (bowings, segno, coda, …).
fn decoration(name: &str) -> Option<Annotation> {
    let art = |name| {
        Some(Annotation::Articulation(Articulation {
            name,
            placement: Placement::default(),
        }))
    };
    let orn = |name| {
        Some(Annotation::Ornament(Ornament {
            name,
            placement: Placement::default(),
        }))
    };
    let wedge = |wedge_type| {
        Some(Annotation::Wedge(Wedge {
            wedge_type,
            placement: Placement::default(),
        }))
    };
    let technical = |name, value: &str| {
        Some(Annotation::Technical(Technical {
            name,
            value: value.to_string(),
        }))
    };
    let fermata = |inverted| {
        Some(Annotation::Fermata(Fermata {
            shape: FermataShape::Normal,
            inverted,
        }))
    };
    match name {
        "pppp" | "ppp" | "pp" | "p" | "mp" | "mf" | "f" | "ff" | "fff" | "ffff" | "sfz" | "sf"
        | "sffz" | "fp" | "fz" | "rfz" => Some(Annotation::Dynamic(DynamicMark {
            sign: name.into(),
            placement: Placement::default(),
        })),
        "<(" | "crescendo(" => wedge(WedgeType::Crescendo),
        ">(" | "diminuendo(" | "decrescendo(" => wedge(WedgeType::Diminuendo),
        "<)" | ">)" | "crescendo)" | "diminuendo)" | "decrescendo)" => wedge(WedgeType::Stop),
        "." | "staccato" => art(ArticulationType::Staccato),
        ">" | "accent" | "emphasis" | "L" => art(ArticulationType::Accent),
        "^" | "marcato" => art(ArticulationType::StrongAccent),
        "tenuto" => art(ArticulationType::Tenuto),
        "wedge" | "staccatissimo" => art(ArticulationType::Staccatissimo),
        "breath" => art(ArticulationType::BreathMark),
        "trill" | "T" => orn(OrnamentType::TrillMark),
        "lowermordent" | "mordent" | "M" => orn(OrnamentType::Mordent),
        "uppermordent" | "pralltriller" | "P" => orn(OrnamentType::InvertedMordent),
        "turn" | "roll" | "~" => orn(OrnamentType::Turn),
        "invertedturn" => orn(OrnamentType::InvertedTurn),
        "fermata" | "H" => fermata(false),
        "invertedfermata" => fermata(true),
        "upbow" | "u" => technical(TechnicalType::UpBow, ""),
        "downbow" | "v" => technical(TechnicalType::DownBow, ""),
        "0" | "1" | "2" | "3" | "4" | "5" => technical(TechnicalType::Fingering, name),
        _ => None,
    }
}

/// A quoted string: a chord symbol (`Am7`, `F#m7b5`, `G/B`, `C6/9`) when it
/// reads as one, `N.C.` for none, else an annotation — `^` above, `_` below,
/// the other placements (`<`, `>`, `@`) and a plain string (`"Fine"`) above.
fn quoted(text: &str) -> Option<Music> {
    let first = text.chars().next()?;
    let annotation = |words: &str| -> Option<Music> {
        (!words.trim().is_empty()).then(|| {
            Music::Direction(Box::new(Direction {
                text: Some(TextDirection {
                    text: words.trim().to_string(),
                    placement: if first == '_' {
                        Placement::Below
                    } else {
                        Placement::Above
                    },
                    font_style: None,
                    font_weight: None,
                }),
                ..Direction::default()
            }))
        })
    };
    if let Some(words) = text.strip_prefix(['^', '_', '<', '>', '@']) {
        return annotation(words);
    }
    let pitch = |s: &str| -> Option<(ChordPitch, usize)> {
        let step = s
            .get(..1)
            .filter(|c| ("A"..="G").contains(c))
            .and_then(PitchStep::from_name)?;
        // Up to two of one accidental (`Fbb`, `C##`).
        let sign = |c: char| match c {
            '#' | '♯' => 1.0,
            'b' | '♭' => -1.0,
            _ => 0.0,
        };
        let first = s[1..].chars().next().map_or(0.0, sign);
        let signs: Vec<char> = s[1..]
            .chars()
            .take_while(|&c| first != 0.0 && sign(c) == first)
            .take(2)
            .collect();
        let alter = first * signs.len() as f64;
        let n = 1 + signs.iter().map(|c| c.len_utf8()).sum::<usize>();
        Some((ChordPitch { step, alter }, n))
    };
    if matches!(text.trim(), "N.C." | "NC" | "N.C") {
        return Some(Music::Harmony(Harmony {
            root: ChordPitch {
                step: PitchStep::C,
                alter: 0.0,
            },
            kind: ChordKind::NoChord,
            bass: None,
            degrees: Vec::new(),
            offset: Frac::from_integer(0),
            function: None,
        }));
    }
    let Some((root, n)) = pitch(text) else {
        return annotation(text);
    };
    // A bass after the last `/` (`G/B`); `/9` in `C6/9` is the suffix's.
    let (quality, bass) = match text[n..].rsplit_once('/') {
        Some((q, b)) if pitch(b.trim()).is_some_and(|(_, len)| len == b.trim().len()) => {
            (q, pitch(b.trim()).map(|p| p.0))
        }
        _ => (&text[n..], None),
    };
    match parse_chord_suffix(quality.trim()) {
        Some((kind, degrees)) => Some(Music::Harmony(Harmony {
            root,
            kind,
            bass,
            degrees,
            offset: Frac::from_integer(0),
            function: None,
        })),
        // `"Fine"`, `"D.C. al Fine"`: words, not a chord.
        None => annotation(text),
    }
}

/// Put an annotation on the last note or chord (the last of a tuplet).
fn attach(events: &mut [Music], ann: Annotation) {
    match events.iter_mut().rev().find(|m| {
        matches!(
            m,
            Music::Note { .. } | Music::Chord { .. } | Music::Tuplet { .. }
        )
    }) {
        Some(Music::Note { annotations, .. } | Music::Chord { annotations, .. }) => {
            annotations.push(ann)
        }
        Some(Music::Tuplet { content, .. }) => {
            if let Music::Sequential(inner) = content.as_mut() {
                attach(inner, ann);
            }
        }
        _ => {}
    }
}

/// The decorations and slur starts written before a note go on it.
fn take_pending(voice: &mut VoiceStream, m: &mut Music) {
    if let Music::Note { annotations, .. } | Music::Chord { annotations, .. } = m {
        annotations.append(&mut voice.pending);
    }
}

/// Whether the text between two `!` (or `+`) marks is a decoration name like
/// `trill`, `crescendo(`, `D.C.` or `<(` — not music with bar lines or spaces.
fn is_decoration_name(s: &[char]) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.iter()
            .all(|c| !c.is_whitespace() && !matches!(c, '|' | '[' | ']' | '{' | '}' | '"'))
}

/// Start an ending at `i` (its number list: `1`, `1,3`, `1-3`; the IR keeps the
/// first), closing the one before it at the last bar line.
fn open_ending(voice: &mut VoiceStream, chars: &[char], i: &mut usize) {
    let start = *i;
    while *i < chars.len() && (chars[*i].is_ascii_digit() || matches!(chars[*i], ',' | '-')) {
        *i += 1;
    }
    let number: u8 = chars[start..*i]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(1);
    close_ending(voice, None);
    voice.events.push(Music::Barline(Barline {
        location: BarlineLocation::Left,
        ending_number: Some(number),
        ending_type: Some(EndingType::Start),
        ..Barline::default()
    }));
    voice.open_ending = Some(number);
}

/// Close the open ending, on `bar` (the bar line being read) or else on the
/// last bar line of the voice.
fn close_ending(voice: &mut VoiceStream, bar: Option<&mut Barline>) {
    let Some(number) = voice.open_ending.take() else {
        return;
    };
    let target = match bar {
        Some(b) => Some(b),
        None => voice.events.iter_mut().rev().find_map(|m| match m {
            Music::Barline(b) if b.ending_type.is_none() => Some(b),
            _ => None,
        }),
    };
    if let Some(b) = target {
        b.ending_number = Some(number);
        b.ending_type = Some(EndingType::Stop);
    }
}

/// True for events that consume a tuplet slot (ABC counts notes, not barlines).
fn is_sounding(m: &Music) -> bool {
    matches!(
        m,
        Music::Note { .. } | Music::Chord { .. } | Music::Rest { .. } | Music::Skip { .. }
    )
}

/// Most bars a single `Z`/`X` rest may span (a guard against `Z99999999`).
const MAX_BAR_RESTS: u32 = 1000;

/// Multiply a note's, chord's or rest's length by `f` (broken rhythm).
/// (A tuplet's ratio stays: the written value is what scales.)
fn scale_duration(m: &mut Music, f: Frac) {
    if let Music::Note { duration, .. }
    | Music::Chord { duration, .. }
    | Music::Rest { duration, .. }
    | Music::Skip { duration } = m
    {
        let written = duration.base * crate::ir::duration::dot_multiplier(duration.dots);
        duration.base = written * f;
        duration.dots = 0;
    }
}

/// The note, chord or rest a broken rhythm lengthens: the last one before
/// it, inside a tuplet just closed too (`(3ABc>d`).
fn last_timed(events: &mut [Music]) -> Option<&mut Music> {
    let m = events.iter_mut().rev().find(|m| {
        !matches!(
            m,
            Music::Grace { .. }
                | Music::Clef(_)
                | Music::KeySignature(_)
                | Music::Direction(_)
                | Music::Harmony(_)
                | Music::Tempo(_)
        )
    })?;
    match m {
        Music::Tuplet { content, .. } => match content.as_mut() {
            Music::Sequential(inner) => last_timed(inner),
            _ => None,
        },
        other => Some(other),
    }
}

/// Where the bar being read starts in `events`: after the last bar line.
fn bar_start(events: &[Music]) -> usize {
    events
        .iter()
        .rposition(|m| matches!(m, Music::Barline(_)))
        .map_or(0, |k| k + 1)
}

/// Close a bar with `&` overlays: its layers become one `Simultaneous`, each
/// layer a voice of its own.
fn end_overlay(voice: &mut VoiceStream) {
    if voice.overlay.is_empty() {
        return;
    }
    let start = bar_start(&voice.events);
    let last = voice.events.split_off(start);
    voice.overlay.push(last);
    let layers = voice.overlay.drain(..).map(Music::Sequential).collect();
    voice.events.push(Music::Simultaneous(layers));
}

/// Parse a tuplet spec `p`, `p:q`, `p:q:r` starting at the first digit.
/// Returns ((actual, normal, count), next index).
fn parse_tuplet_spec(chars: &[char], start: usize, compound: bool) -> ((u8, u8, usize), usize) {
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
    // ABC defaults for a bare `(p`: 5, 7 and 9 go in the time of 3 in a
    // compound meter, of 2 otherwise.
    let q = q.unwrap_or(match p {
        2 | 4 | 8 => 3,
        3 | 6 => 2,
        _ if compound => 3,
        _ => 2,
    });
    ((p, q, r.unwrap_or(p) as usize), i)
}

/// Close an open tuplet: wrap the events it collected in `Music::Tuplet` and
/// stamp the ratio onto their durations (the rest of the IR reads it there).
/// ABC beams by spacing (§4.7): eighths and shorter written together are
/// beamed; a space, a rest or a bar line ends the beam. Every note's beaming
/// is the source's (`NoAutoBeam`). `c` is the character just read, the
/// events from `before` what it added.
fn beam_by_spacing(voice: &mut VoiceStream, before: usize, c: char) {
    if matches!(c, ' ' | '\t') {
        voice.beam_from = None;
        return;
    }
    fn lead(m: &mut Music) -> Option<&mut Vec<Annotation>> {
        match m {
            Music::Note { annotations, .. } => Some(annotations),
            Music::Chord { pitches, .. } => pitches.first_mut().map(|(_, a)| a),
            _ => None,
        }
    }
    for k in before.min(voice.events.len())..voice.events.len() {
        let beamable = match &voice.events[k] {
            Music::Note { duration, .. } | Music::Chord { duration, .. } => {
                duration.base < Frac::new(1, 4)
            }
            Music::Harmony(_) | Music::Direction(_) | Music::Grace { .. } => continue,
            _ => {
                voice.beam_from = None;
                continue;
            }
        };
        if let Some(a) = lead(&mut voice.events[k]) {
            a.push(Annotation::NoAutoBeam);
        }
        if !beamable {
            voice.beam_from = None;
            continue;
        }
        if let Some(j) = voice.beam_from {
            if let Some(a) = lead(&mut voice.events[j]) {
                match a.iter().position(|x| *x == Annotation::BeamStop) {
                    Some(p) => {
                        a.remove(p);
                    }
                    None => a.push(Annotation::BeamStart),
                }
            }
            if let Some(a) = lead(&mut voice.events[k]) {
                a.push(Annotation::BeamStop);
            }
        }
        voice.beam_from = Some(k);
    }
}

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
        | Music::Rest { duration, .. }
        | Music::Skip { duration } = m
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

/// Parse `[CEG]` into a chord. The chord lasts as long as its first note
/// (`[C2E2G2]` is a quarter at `L:1/8`) times the length after `]`; a tie on
/// a member (`[C-E]`) ties that note only. Returns the chord, the next index
/// and the pitches tied into the next chord.
fn parse_chord(
    chars: &[char],
    start: usize,
    state: &TuneState,
    voice: &mut VoiceStream,
    grace: bool,
) -> (Option<Music>, usize, Vec<Pitch>) {
    let mut i = start + 1; // past '['
    let mut pitches: Vec<(Pitch, Vec<Annotation>)> = Vec::new();
    let mut first_len: Option<Frac> = None;
    while i < chars.len() && chars[i] != ']' {
        match chars[i] {
            '^' | '_' | '=' | 'A'..='G' | 'a'..='g' => {
                if let (
                    Some(Music::Note {
                        pitch, duration, ..
                    }),
                    next,
                ) = parse_note(chars, i, state, voice, grace)
                {
                    first_len.get_or_insert(duration.actual_duration());
                    let mut anns = Vec::new();
                    i = next;
                    if chars.get(i) == Some(&'-') {
                        anns.push(Annotation::TieStart);
                        i += 1;
                    }
                    pitches.push((pitch, anns));
                } else {
                    i += 1;
                }
            }
            // A decoration or annotation inside the chord is not a pitch.
            q @ ('!' | '"') => {
                i += 1;
                while i < chars.len() && chars[i] != q {
                    i += 1;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    i += 1; // past ']'
    let unit = voice.unit;
    let (outer, next) = parse_duration(chars, i, unit);
    if pitches.is_empty() {
        return (None, next, Vec::new());
    }
    let len = first_len.unwrap_or(unit) * (outer.actual_duration() / unit);
    let tied: Vec<Pitch> = pitches
        .iter()
        .filter(|(_, a)| a.contains(&Annotation::TieStart))
        .map(|(p, _)| *p)
        .collect();
    (
        Some(Music::Chord {
            pitches,
            duration: Duration::new(len),
            annotations: Vec::new(),
        }),
        next,
        tied,
    )
}

/// Parse one note (accidental, letter, octave marks, duration).
fn parse_note(
    chars: &[char],
    start: usize,
    state: &TuneState,
    voice: &mut VoiceStream,
    grace: bool,
) -> (Option<Music>, usize) {
    let mut i = start;
    // Written accidental, if any: `^`, `^^`, `_`, `__` or `=`.
    let mut written: Option<i32> = None;
    while i < chars.len() {
        match chars[i] {
            '^' => {
                written = Some(written.unwrap_or(0) + 1);
                i += 1;
            }
            '_' => {
                written = Some(written.unwrap_or(0) - 1);
                i += 1;
            }
            '=' => {
                written = Some(0);
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
    let (duration, next) = parse_duration(chars, i, voice.unit);
    let letter_idx = letter_index(letter).unwrap_or(0);
    let pitch_state = &mut voice.pitch;
    let alter = pitch_state.alteration(letter_idx, octave, written, state.propagate, grace);
    let octave = octave + pitch_state.octave_shift;
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
fn parse_duration(chars: &[char], start: usize, unit: Frac) -> (Duration, usize) {
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
    (Duration::new(unit * mult), i)
}

/// Attach a tie-start annotation to the most recent note/chord.
fn attach_tie(events: &mut [Music]) -> Vec<Pitch> {
    match events.last_mut() {
        Some(Music::Note {
            pitch, annotations, ..
        }) => {
            annotations.push(Annotation::TieStart);
            vec![*pitch]
        }
        // On each note: lowering reads ties per note.
        Some(Music::Chord { pitches, .. }) => {
            for (_, a) in pitches.iter_mut() {
                if !a.contains(&Annotation::TieStart) {
                    a.push(Annotation::TieStart);
                }
            }
            pitches.iter().map(|(p, _)| *p).collect()
        }
        Some(Music::Tuplet { content, .. }) => match content.as_mut() {
            Music::Sequential(inner) => attach_tie(inner),
            _ => Vec::new(),
        },
        _ => Vec::new(),
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

    /// Each note's beam marks: `[` begins, `]` ends, `.` none.
    fn beam_marks(doc: &MusicDocument) -> String {
        events(doc)
            .iter()
            .filter_map(|m| match m {
                Music::Note { annotations, .. } => Some(annotations),
                _ => None,
            })
            .map(|a| {
                assert!(a.contains(&Annotation::NoAutoBeam), "ABC decides beaming");
                if a.contains(&Annotation::BeamStart) {
                    '['
                } else if a.contains(&Annotation::BeamStop) {
                    ']'
                } else {
                    '.'
                }
            })
            .collect()
    }

    #[test]
    fn notes_written_together_are_beamed() {
        let doc = parse("X:1\nL:1/8\nK:C\ncdef g2 a b|c\"G\"d!p!e z f\n");
        assert_eq!(beam_marks(&doc), "[..]...[.].");
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
