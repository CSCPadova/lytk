//! IR → ABC notation adapter (EET2).
//!
//! Emits a Layer-1 [`MusicDocument`] as an ABC tune: an `X/T/C/M/L/K` header
//! followed by the note/rest/chord/barline body. The unit note length `L:` is
//! fixed at `1/8`; every duration is rendered as a multiple of it.
//!
//! Pitches keep the score's spelling. An accidental is printed whenever a
//! standard reader would otherwise sound something else — under either ABC
//! accidental rule: carried in the same octave (abcm2ps, abc2svg) or in every
//! octave (the ABC 2.1 text's default). So `=F` after `^F` in the bar, `=f`
//! after `^F` too, and `^F` again after the bar line; nothing where the key
//! and the bar already give the right note.

use std::collections::HashMap;

use crate::ir::annotation::Annotation;
use crate::ir::articulation::{Placement, SyllabicType};
use crate::ir::direction::{Barline, BarlineType, RepeatDirection, TempoDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::{suffix_of_kind, Harmony};
use crate::ir::measure::{KeyMode, KeySignature, TimeSignature};
use crate::ir::music::{ContextType, Music, MusicDocument, RepeatType};
use crate::ir::pitch::{respell, Pitch};

use super::{FromMusicAdapter, Result};

/// Bars per output line — ABC convention, and it keeps lines readable.
const BARS_PER_LINE: usize = 4;

/// The unit note length used for emission (`L:1/8`).
const UNIT_LENGTH: Frac = Frac::new_raw(1, 8);

/// Adapter that emits ABC notation.
#[derive(Default)]
pub struct IrToAbcAdapter;

impl IrToAbcAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl FromMusicAdapter for IrToAbcAdapter {
    fn convert_music(&self, doc: &MusicDocument) -> Result<String> {
        Ok(emit_tune(doc))
    }
}

/// One emitted ABC voice: an optional name and its flat event stream.
struct OutVoice {
    name: Option<String>,
    events: Vec<Music>,
}

fn emit_tune(doc: &MusicDocument) -> String {
    // Split the tree into top-level voices (parts / staves). A single voice keeps
    // the original single-line ABC; ≥2 voices emit `V:` blocks (ABC 2.1 §4.1).
    let voices: Vec<OutVoice> = top_level_voices(&doc.music)
        .into_iter()
        .map(|(name, events)| OutVoice { name, events })
        .filter(|v| has_audible(&v.events))
        .collect();

    // Header: pull the first time/key signature from any voice.
    let first_time = voices
        .iter()
        .flat_map(|v| v.events.iter())
        .find_map(|m| match m {
            Music::TimeSignature(t) => Some(t.clone()),
            _ => None,
        });
    let first_key = voices
        .iter()
        .flat_map(|v| v.events.iter())
        .find_map(|m| match m {
            Music::KeySignature(k) => Some(*k),
            _ => None,
        });
    // A tempo opening the first voice goes in the header.
    let first_tempo = voices.first().and_then(|v| {
        v.events
            .iter()
            .take_while(|m| sounding_duration(m).is_none())
            .find_map(|m| match m {
                Music::Tempo(t) => Some(t.clone()),
                _ => None,
            })
    });
    // Bar length in whole notes, for deriving the regular bar lines.
    let init_bar = first_time.as_ref().map(|t| t.beats_fraction());
    // An opening pickup: the first bar is that short.
    let pickup = doc
        .metadata
        .partial_duration
        .as_ref()
        .map(|d| d.actual_duration());

    let mut out = String::new();
    out.push_str("X:1\n");
    if let Some(title) = &doc.metadata.title {
        out.push_str(&format!("T:{title}\n"));
    }
    if let Some(composer) = &doc.metadata.composer {
        out.push_str(&format!("C:{composer}\n"));
    }
    if let Some(ts) = &first_time {
        out.push_str(&format!("M:{}\n", meter_to_abc(ts)));
    }
    out.push_str("L:1/8\n");
    if let Some(q) = first_tempo.as_ref().and_then(tempo_to_abc) {
        out.push_str(&format!("Q:{q}\n"));
    }
    out.push_str(&format!(
        "K:{}\n",
        first_key
            .as_ref()
            .map(key_to_abc)
            .unwrap_or_else(|| "C".to_string())
    ));

    match voices.len() {
        0 => {
            out.push('\n');
        }
        1 => {
            let body = emit_body(
                &voices[0].events,
                first_time.as_ref(),
                first_key,
                first_tempo.as_ref(),
                init_bar,
                pickup,
            );
            // No blank line: it would end the tune for every ABC reader.
            out.push_str(body.trim_end());
            out.push('\n');
        }
        _ => {
            // Each voice is its own `V:n` block; the shared M:/K: live in the
            // header, so a body repeats its leading time/key only if it differs.
            for (i, v) in voices.iter().enumerate() {
                let id = i + 1;
                match &v.name {
                    Some(n) => out.push_str(&format!("V:{id} name=\"{n}\"\n")),
                    None => out.push_str(&format!("V:{id}\n")),
                }
                let body = emit_body(
                    &v.events,
                    first_time.as_ref(),
                    first_key,
                    first_tempo.as_ref(),
                    init_bar,
                    pickup,
                );
                out.push_str(body.trim_end());
                out.push('\n');
            }
        }
    }
    out
}

/// Emit one voice's body as a space-joined token string. The voice's first
/// time/key signature is dropped when it matches the header's `M:`/`K:`.
fn emit_body(
    events: &[Music],
    header_time: Option<&TimeSignature>,
    header_key: Option<KeySignature>,
    header_tempo: Option<&TempoDirection>,
    init_bar: Option<Frac>,
    mut pickup: Option<Frac>,
) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut time_used = false;
    let mut tempo_used = false;
    // Marks a direction puts on the next note (a dynamic, a hairpin).
    let mut carried = String::new();
    let mut lyrics = Lyrics::default();
    let mut key_used = false;
    let mut acc = Accidentals::new(header_key.map_or(0, |k| k.fifths as i32));
    // Bar accounting: the IR only carries explicit `Music::Barline` events for
    // *non-default* barlines, so regular bar lines have to be derived from the
    // running meter — without them the output is one giant ABC measure.
    // Without a meter a reader keeps only the bar lines written (ABC 2.1), so
    // write them where a meterless score has them: every whole note.
    let mut bar_len = init_bar.or(Some(Frac::from_integer(1)));
    // The length of the bar being written when it isn't the meter's
    // (`Music::Partial`: ABC bar lines are real, a bar can be irregular).
    let mut this_bar: Option<Frac> = None;
    let mut filled = Frac::new(0, 1);
    let mut bars_on_line = 0usize;
    // Sounding events still inside the open tuplet run (0 = not in a tuplet).
    let mut tuplet_left = 0usize;
    let mut queue: std::collections::VecDeque<Music> = events.iter().cloned().collect();
    // The other voices of the bar being written, as `&` layers (ABC 2.1
    // §7.4) just before its bar line.
    let mut layers: Vec<Vec<Music>> = Vec::new();
    while let Some(mut ev) = queue.pop_front() {
        // A pickup: the first bar holds only its length.
        if sounding_duration(&ev).is_some() || matches!(ev, Music::Simultaneous(_)) {
            if let (Some(p), Some(len)) = (pickup.take(), bar_len) {
                if p > Frac::new(0, 1) && p < len && filled == Frac::new(0, 1) {
                    filled = len - p;
                }
            }
        }
        let room = this_bar.or(bar_len).map(|len| len - filled);
        if let Music::Simultaneous(branches) = &ev {
            // Several voices: when they fill no more than the bar they start,
            // the first here and the others as `&` layers; else the richest.
            let voices: Vec<Vec<Music>> = branches
                .iter()
                .map(|b| match b {
                    Music::Sequential(v) => v.clone(),
                    other => vec![other.clone()],
                })
                .collect();
            let longest = branches
                .iter()
                .map(Music::written_length)
                .max()
                .unwrap_or_default();
            let fits = filled == Frac::new(0, 1)
                && room.is_some_and(|r| longest <= r)
                && layers.is_empty();
            let mut voices = voices.into_iter();
            let first = if fits {
                let mut first = voices.next().unwrap_or_default();
                layers.extend(voices.filter(|v| !v.is_empty()));
                // The bar lasts as long as its longest voice.
                let short = longest - first.iter().map(Music::written_length).sum::<Frac>();
                if short > Frac::new(0, 1) {
                    first.push(Music::Skip {
                        duration: Duration::new(short),
                    });
                }
                first
            } else {
                voices.max_by_key(|v| v.len()).unwrap_or_default()
            };
            for m in first.into_iter().rev() {
                queue.push_front(m);
            }
            continue;
        }
        // A note longer than what is left of the bar is tied over its line.
        if let (Some(r), Some(d)) = (room, sounding_duration(&ev)) {
            if d > r && r > Frac::new(0, 1) && tuplet_ratio(&ev).is_none() {
                let (head, tail) = split_music(&ev, r);
                queue.push_front(tail);
                ev = head;
            }
        }
        match tuplet_ratio(&ev) {
            Some(ratio) => {
                if tuplet_left == 0 {
                    // One group per `p` notes: always fits inside a bar, so a
                    // run never straddles a barline or a wrapped line.
                    let run = std::iter::once(&ev)
                        .chain(queue.iter())
                        .take_while(|m| tuplet_ratio(m) == Some(ratio))
                        .count()
                        .min(ratio.0.max(1) as usize);
                    tokens.push(format!("({}:{}:{}", ratio.0, ratio.1, run));
                    tuplet_left = run;
                }
                tuplet_left -= 1;
            }
            None => tuplet_left = 0,
        }
        match &ev {
            Music::TimeSignature(t) => {
                bar_len = Some(t.beats_fraction());
                filled = Frac::new(0, 1);
                if time_used || header_time != Some(t) {
                    tokens.push(format!("[M:{}]", meter_to_abc(t)));
                }
                time_used = true;
            }
            Music::KeySignature(k) => {
                acc.set_key(k.fifths as i32);
                if key_used || header_key != Some(*k) {
                    tokens.push(format!("[K:{}]", key_to_abc(k)));
                }
                key_used = true;
            }
            Music::Note { .. } | Music::Chord { .. } | Music::Rest { .. } | Music::Skip { .. } => {
                if let Some(tok) = sounding_token(&ev, &mut acc, false) {
                    let (pre, post) = decorations(&ev);
                    tokens.push(format!("{}{pre}{tok}{post}", std::mem::take(&mut carried)));
                    lyrics.note(&ev);
                }
            }
            Music::Harmony(h) => tokens.push(format!("\"{}\"", chord_symbol(h))),
            Music::Direction(d) => {
                if let Some(t) = d.text.as_ref().filter(|t| !t.text.contains('"')) {
                    let at = if t.placement == Placement::Below {
                        '_'
                    } else {
                        '^'
                    };
                    tokens.push(format!("\"{at}{}\"", t.text));
                }
                // A dynamic or hairpin written as a direction marks the next note.
                if let Some(dm) = &d.dynamic {
                    carried.push_str(&format!("!{}!", dm.sign));
                }
                if let Some(w) = &d.wedge {
                    carried.push_str(wedge_sign(&w.wedge_type));
                }
            }
            Music::Tempo(t) => {
                if tempo_used || header_tempo != Some(t) {
                    if let Some(q) = tempo_to_abc(t) {
                        tokens.push(format!("[Q:{q}]"));
                    }
                }
                tempo_used = true;
            }
            // Grace group: `{ab}`, or `{/a}` for an acciaccatura (ABC 2.1 §4.10).
            // Graces carry no metrical time, so they never move the bar clock.
            Music::Grace { content, slash } => {
                tokens.extend(grace_token(content, *slash, &mut acc))
            }
            Music::Partial(d) => {
                this_bar = Some(d.actual_duration());
                // Before the first note it sizes the first bar: it is the pickup.
                pickup = None;
            }
            Music::Barline(b) => {
                let tok = barline_to_abc(b);
                let ending = tok.starts_with('[');
                if !ending {
                    push_layers(&mut tokens, &mut layers, &mut acc);
                }
                let last = tokens.iter().rposition(|t| t != "\n");
                if tok.starts_with('[') {
                    // An ending opens after the bar line.
                    tokens.push(tok);
                } else if filled == Frac::new(0, 1) && last.is_some_and(|i| tokens[i] == "|") {
                    // The meter already closed the bar here: this bar line
                    // replaces that `|` rather than adding an empty bar.
                    tokens[last.unwrap()] = tok;
                } else {
                    tokens.push(tok);
                    bars_on_line += 1;
                }
                acc.bar();
                filled = Frac::new(0, 1);
                if !ending {
                    this_bar = None;
                }
            }
            _ => {}
        }
        // Regular bar line: close the bar as soon as the meter's worth of time
        // has been emitted (explicit barlines above reset the count themselves).
        if let (Some(len), Some(d)) = (this_bar.or(bar_len), sounding_duration(&ev)) {
            if len > Frac::new(0, 1) {
                filled += d;
                if filled >= len {
                    push_layers(&mut tokens, &mut layers, &mut acc);
                    tokens.push("|".to_string());
                    acc.bar();
                    filled = Frac::new(0, 1);
                    this_bar = None;
                    bars_on_line += 1;
                }
            }
        }
        if bars_on_line >= BARS_PER_LINE {
            bars_on_line = 0;
            tokens.push("\n".to_string());
            // The line's words go under it.
            tokens.extend(lyrics.lines());
        }
    }
    push_layers(&mut tokens, &mut layers, &mut acc);
    let words = lyrics.lines();
    if !words.is_empty() {
        tokens.push("\n".to_string());
        tokens.extend(words);
    }
    // Join on spaces, but keep the line breaks we inserted as real newlines.
    tokens.join(" ").replace(" \n ", "\n").replace(" \n", "\n")
}

/// Write the bar's other voices, each after a `&`, with the accidentals the
/// bar's first voice left (a reader goes through the layers in order).
fn push_layers(tokens: &mut Vec<String>, layers: &mut Vec<Vec<Music>>, acc: &mut Accidentals) {
    for layer in layers.drain(..) {
        tokens.push("&".to_string());
        let mut left = 0usize;
        for (k, m) in layer.iter().enumerate() {
            match tuplet_ratio(m) {
                Some(r) => {
                    if left == 0 {
                        let run = layer[k..]
                            .iter()
                            .take_while(|x| tuplet_ratio(x) == Some(r))
                            .count()
                            .min(r.0.max(1) as usize);
                        tokens.push(format!("({}:{}:{}", r.0, r.1, run));
                        left = run;
                    }
                    left -= 1;
                }
                None => left = 0,
            }
            match m {
                Music::Grace { content, slash } => tokens.extend(grace_token(content, *slash, acc)),
                _ => {
                    if let Some(tok) = sounding_token(m, acc, false) {
                        let (pre, post) = decorations(m);
                        tokens.push(format!("{pre}{tok}{post}"));
                    }
                }
            }
        }
    }
}

/// A grace group: `{ab}`, or `{/a}` for an acciaccatura (ABC 2.1 §4.10).
fn grace_token(content: &Music, slash: bool, acc: &mut Accidentals) -> Option<String> {
    let mut inner = Vec::new();
    walk(content, &mut inner);
    let body: String = inner
        .iter()
        .filter_map(|m| sounding_token(m, acc, true))
        .collect::<Vec<_>>()
        .join("");
    (!body.is_empty()).then(|| format!("{{{}{}}}", if slash { "/" } else { "" }, body))
}

/// A note, chord, rest or skip cut after `first`: the head tied to the tail
/// (which keeps the onward tie and the slur end; the marks, words and slur
/// start stay on the head).
fn split_music(m: &Music, first: Frac) -> (Music, Music) {
    let head_anns = |a: &[Annotation]| {
        let mut v: Vec<Annotation> = a
            .iter()
            .filter(|x| !matches!(x, Annotation::TieStart | Annotation::SlurStop { .. }))
            .cloned()
            .collect();
        v.push(Annotation::TieStart);
        v
    };
    let tail_anns = |a: &[Annotation]| -> Vec<Annotation> {
        a.iter()
            .filter(|x| matches!(x, Annotation::TieStart | Annotation::SlurStop { .. }))
            .cloned()
            .collect()
    };
    let rest = |d: &Duration| Duration::new(d.actual_duration() - first);
    match m {
        Music::Note {
            pitch,
            duration,
            annotations,
        } => (
            Music::Note {
                pitch: *pitch,
                duration: Duration::new(first),
                annotations: head_anns(annotations),
            },
            Music::Note {
                pitch: *pitch,
                duration: rest(duration),
                annotations: tail_anns(annotations),
            },
        ),
        Music::Chord {
            pitches,
            duration,
            annotations,
        } => (
            Music::Chord {
                pitches: pitches.iter().map(|(p, a)| (*p, head_anns(a))).collect(),
                duration: Duration::new(first),
                annotations: head_anns(annotations),
            },
            Music::Chord {
                pitches: pitches.iter().map(|(p, a)| (*p, tail_anns(a))).collect(),
                duration: rest(duration),
                annotations: tail_anns(annotations),
            },
        ),
        Music::Rest {
            duration,
            is_measure_rest,
        } => (
            Music::Rest {
                duration: Duration::new(first),
                is_measure_rest: *is_measure_rest,
            },
            Music::Rest {
                duration: rest(duration),
                is_measure_rest: *is_measure_rest,
            },
        ),
        Music::Skip { duration } => (
            Music::Skip {
                duration: Duration::new(first),
            },
            Music::Skip {
                duration: rest(duration),
            },
        ),
        other => (other.clone(), Music::Sequential(Vec::new())),
    }
}

/// The syllables of the notes of the music line being written, a list per
/// verse: `*` under a note without one, `_` while one is held.
#[derive(Default)]
struct Lyrics {
    verses: Vec<Vec<String>>,
    notes: usize,
    held: Vec<bool>,
}

impl Lyrics {
    fn note(&mut self, m: &Music) {
        let anns: Vec<&Annotation> = match m {
            Music::Note { annotations, .. } => annotations.iter().collect(),
            Music::Chord {
                pitches,
                annotations,
                ..
            } => annotations
                .iter()
                .chain(pitches.iter().flat_map(|(_, a)| a.iter()))
                .collect(),
            _ => return,
        };
        for a in anns {
            if let Annotation::Lyric(l) = a {
                let v = (l.number.max(1) - 1) as usize;
                if self.verses.len() <= v {
                    self.verses.resize(v + 1, Vec::new());
                    self.held.resize(v + 1, false);
                }
                if self.verses[v].len() <= self.notes {
                    let text = l.text.replace(' ', "~").replace('-', "\\-");
                    let hyphen = matches!(l.syllabic, SyllabicType::Begin | SyllabicType::Middle);
                    self.verses[v].push(format!("{text}{}", if hyphen { "-" } else { "" }));
                    self.held[v] = l.extend;
                }
            }
        }
        self.notes += 1;
        for (v, words) in self.verses.iter_mut().enumerate() {
            while words.len() < self.notes {
                words.push(if self.held[v] { "_" } else { "*" }.to_string());
            }
        }
    }

    /// The `w:` lines for the notes so far, then a fresh line.
    fn lines(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for words in self.verses.iter_mut() {
            if words.iter().any(|w| w != "*" && w != "_") {
                out.push(format!("w: {}", words.join(" ")));
                out.push("\n".to_string());
            }
            words.clear();
        }
        self.notes = 0;
        out
    }
}

/// Decorations and slur starts written before a note, slur ends after it
/// (ABC 2.1 §4.14).
fn decorations(m: &Music) -> (String, String) {
    let anns: Vec<&Annotation> = match m {
        Music::Note { annotations, .. } => annotations.iter().collect(),
        Music::Chord {
            pitches,
            annotations,
            ..
        } => annotations
            .iter()
            .chain(pitches.iter().flat_map(|(_, a)| a.iter()))
            .collect(),
        _ => return (String::new(), String::new()),
    };
    let (mut pre, mut slurs, mut post) = (String::new(), String::new(), String::new());
    let mut seen: Vec<String> = Vec::new();
    for a in anns {
        let sign = match a {
            Annotation::Dynamic(d) => format!("!{}!", d.sign),
            Annotation::Wedge(w) => wedge_sign(&w.wedge_type).to_string(),
            Annotation::Articulation(a) => match a.name.as_str() {
                "staccato" => ".",
                "accent" => "!>!",
                "strong-accent" => "!marcato!",
                "tenuto" => "!tenuto!",
                "staccatissimo" => "!wedge!",
                "breath-mark" => "!breath!",
                _ => "",
            }
            .to_string(),
            Annotation::Ornament(o) => match o.name.as_str() {
                "trill-mark" => "!trill!",
                "mordent" => "!mordent!",
                "inverted-mordent" => "!uppermordent!",
                "turn" => "!turn!",
                "inverted-turn" => "!invertedturn!",
                _ => "",
            }
            .to_string(),
            Annotation::Fermata(f) => if f.inverted {
                "!invertedfermata!"
            } else {
                "!fermata!"
            }
            .to_string(),
            Annotation::SlurStart { .. } => {
                slurs.push('(');
                continue;
            }
            Annotation::SlurStop { .. } => {
                post.push(')');
                continue;
            }
            _ => continue,
        };
        // A chord's notes each carry the chord's marks: write them once.
        if !sign.is_empty() && !seen.contains(&sign) {
            pre.push_str(&sign);
            seen.push(sign);
        }
    }
    pre.push_str(&slurs);
    (pre, post)
}

fn wedge_sign(kind: &str) -> &'static str {
    match kind {
        "crescendo" => "!<(!",
        "diminuendo" | "decrescendo" => "!>(!",
        _ => "!<)!",
    }
}

/// `Am7`, `F#m7b5`, `G/B`.
fn chord_symbol(h: &Harmony) -> String {
    let alter = |a: f64| match a.round() as i32 {
        1 => "#",
        -1 => "b",
        _ => "",
    };
    let mut s = format!(
        "{}{}{}",
        h.root.step,
        alter(h.root.alter),
        suffix_of_kind(&h.kind)
    );
    if let Some(b) = &h.bass {
        s.push_str(&format!("/{}{}", b.step, alter(b.alter)));
    }
    s
}

/// A `Q:` value: `"Allegro" 1/4=120`, `3/8=80`.
fn tempo_to_abc(t: &TempoDirection) -> Option<String> {
    let unit = t.beat_unit.as_deref().and_then(|u| match u {
        "whole" => Some(Frac::new(1, 1)),
        "half" => Some(Frac::new(1, 2)),
        "quarter" => Some(Frac::new(1, 4)),
        "eighth" => Some(Frac::new(1, 8)),
        "16th" => Some(Frac::new(1, 16)),
        "32nd" => Some(Frac::new(1, 32)),
        "breve" => Some(Frac::new(2, 1)),
        _ => None,
    });
    let beat = unit.map(|u| u * crate::ir::duration::dot_multiplier(t.dots));
    let mut parts = Vec::new();
    if let Some(text) = t.text.as_ref().filter(|s| !s.contains('"')) {
        parts.push(format!("\"{text}\""));
    }
    if let (Some(b), Some(bpm)) = (beat, t.per_minute) {
        parts.push(format!(
            "{}/{}={}",
            b.numer(),
            b.denom(),
            bpm.round() as i64
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Sounding length of an event (notes/chords/rests advance the bar clock).
fn sounding_duration(m: &Music) -> Option<Frac> {
    match m {
        Music::Note { duration, .. }
        | Music::Chord { duration, .. }
        | Music::Rest { duration, .. }
        | Music::Skip { duration } => Some(duration.actual_duration()),
        _ => None,
    }
}

/// Split the tree into top-level voices: each part / staff becomes one ABC
/// voice. Grouping contexts (PianoStaff, StaffGroup, …) are descended into; a
/// Staff/Voice context is a leaf voice whose events are flattened (inner
/// per-measure polyphony collapses to its richest branch, as in v1).
fn top_level_voices(music: &Music) -> Vec<(Option<String>, Vec<Music>)> {
    match music {
        Music::Context {
            context_type,
            name,
            content,
        } => match context_type {
            ContextType::Staff
            | ContextType::Voice
            | ContextType::TabStaff
            | ContextType::TabVoice => {
                let mut events = Vec::new();
                walk(content, &mut events);
                vec![(name.clone(), events)]
            }
            // Grouping context: descend; the inner staves carry the voice names.
            _ => top_level_voices(content),
        },
        Music::Simultaneous(branches) => branches.iter().flat_map(top_level_voices).collect(),
        other => {
            let mut events = Vec::new();
            walk(other, &mut events);
            vec![(None, events)]
        }
    }
}

/// True if a flat event stream contains any sounding event (note / chord / rest).
fn has_audible(events: &[Music]) -> bool {
    events.iter().any(|m| {
        matches!(
            m,
            Music::Note { .. } | Music::Chord { .. } | Music::Rest { .. }
        )
    })
}

fn walk(music: &Music, out: &mut Vec<Music>) {
    match music {
        Music::Sequential(items) => {
            for m in items {
                walk(m, out);
            }
        }
        Music::Context { content, .. }
        | Music::Variable { content, .. }
        | Music::Tuplet { content, .. } => walk(content, out),
        // Voices sounding together: each flattened; the body writes a bar's
        // worth as `&` layers, a longer one as its richest voice.
        Music::Simultaneous(items) => {
            let branches: Vec<Music> = items
                .iter()
                .map(|b| {
                    let mut tmp = Vec::new();
                    walk(b, &mut tmp);
                    Music::Sequential(tmp)
                })
                .filter(|b| !b.is_empty())
                .collect();
            match branches.len() {
                0 => {}
                1 => out.extend(match branches.into_iter().next() {
                    Some(Music::Sequential(v)) => v,
                    _ => Vec::new(),
                }),
                _ => out.push(Music::Simultaneous(branches)),
            }
        }
        // A volta repeat is written as one: `|: … :|`, with `[1`/`[2` endings;
        // the passes past what ABC's signs play (a third time through) are
        // written out before it. Any other repeat is written out.
        Music::Repeat {
            repeat_type,
            count,
            body,
            alternatives,
        } => {
            if *repeat_type != RepeatType::Volta {
                for i in 0..(*count).max(1) as usize {
                    walk(body, out);
                    if let Some(alt) = alternatives.get(i.min(alternatives.len().saturating_sub(1)))
                    {
                        walk(alt, out);
                    }
                }
                return;
            }
            let extra = (*count as usize).saturating_sub(alternatives.len().max(2));
            for _ in 0..extra {
                walk(body, out);
                if let Some(first) = alternatives.first() {
                    walk(first, out);
                }
            }
            out.push(Music::Barline(Barline {
                style: BarlineType::RepeatForward,
                location: "left".to_string(),
                repeat_direction: Some(RepeatDirection::Forward),
                ..Barline::default()
            }));
            walk(body, out);
            let layout = crate::ir::music::volta_layout(alternatives);
            if let Some(tail) = layout.tail {
                walk(tail, out);
            }
            let alternatives = layout.endings;
            let backward = || Barline {
                style: BarlineType::RepeatBackward,
                repeat_direction: Some(RepeatDirection::Backward),
                ..Barline::default()
            };
            if alternatives.is_empty() {
                out.push(Music::Barline(backward()));
            }
            for (k, alt) in alternatives.iter().enumerate() {
                let number = u8::try_from(k + 1).unwrap_or(u8::MAX);
                out.push(Music::Barline(Barline {
                    location: "left".to_string(),
                    ending_number: Some(number),
                    ending_type: Some("start".to_string()),
                    ..Barline::default()
                }));
                walk(alt, out);
                // The last ending ends at a bar line that ends endings
                // (ABC 2.1 §4.9): a plain `|` would carry it on.
                let mut close = if k + 1 < alternatives.len() || layout.last_repeats {
                    backward()
                } else {
                    Barline {
                        style: BarlineType::Double,
                        ..Barline::default()
                    }
                };
                close.ending_number = Some(number);
                close.ending_type = Some("stop".to_string());
                out.push(Music::Barline(close));
            }
        }
        other => out.push(other.clone()),
    }
}

fn has_tie(annotations: &[Annotation]) -> bool {
    annotations.contains(&Annotation::TieStart)
}

/// What a standard ABC reader sounds for an unmarked note of a voice: the key
/// signature, overridden by the accidentals already met in the bar — tracked
/// under both rules, per (letter, octave) and per letter.
struct Accidentals {
    fifths: i32,
    key: [i32; 7],
    by_octave: HashMap<(usize, i32), i32>,
    by_letter: HashMap<usize, i32>,
    /// A note has been written in this bar.
    noted: bool,
    /// The key changed mid-bar: some readers keep the bar's accidentals and
    /// some reset them, so every note restates its own until the bar ends.
    unsure: bool,
}

/// Neither rule can be relied on (after a grace note's accidental, which some
/// readers carry and some don't): the next note of the letter restates its own.
const UNSURE: i32 = i32::MIN;

impl Accidentals {
    fn new(fifths: i32) -> Self {
        let mut a = Accidentals {
            fifths: 0,
            key: [0; 7],
            by_octave: HashMap::new(),
            by_letter: HashMap::new(),
            noted: false,
            unsure: false,
        };
        a.set_key(fifths);
        a
    }

    fn set_key(&mut self, fifths: i32) {
        const SHARPS: [usize; 7] = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B
                                                          // ABC has no key past 7 sharps or flats: such a key is written `K:C`
                                                          // with every alteration explicit (see `key_to_abc`).
        let fifths = if fifths.abs() > 7 { 0 } else { fifths };
        self.fifths = fifths;
        self.unsure |= self.noted;
        self.key = [0; 7];
        for k in 0..fifths.unsigned_abs() as usize {
            if fifths > 0 {
                self.key[SHARPS[k % 7]] += 1;
            } else {
                self.key[SHARPS[6 - k % 7]] -= 1;
            }
        }
    }

    /// A bar line: accidentals stop carrying.
    fn bar(&mut self) {
        self.by_octave.clear();
        self.by_letter.clear();
        self.noted = false;
        self.unsure = false;
    }

    /// The accidental to print before `pitch` (`None` when both rules already
    /// give it), recording what it tells the reader.
    fn mark(&mut self, pitch: &Pitch, grace: bool) -> Option<i32> {
        let letter = pitch.step.index() as usize;
        let alter = *pitch.alter.numer() / *pitch.alter.denom();
        let key = self.key[letter];
        let by_octave = *self.by_octave.get(&(letter, pitch.octave)).unwrap_or(&key);
        let by_letter = *self.by_letter.get(&letter).unwrap_or(&key);
        self.noted = true;
        if !self.unsure && by_octave == alter && by_letter == alter {
            return None;
        }
        let told = if grace { UNSURE } else { alter };
        self.by_octave.insert((letter, pitch.octave), told);
        self.by_letter.insert(letter, told);
        Some(alter)
    }
}

/// Render a pitch as an ABC token: the accidental `acc` (if any) and the
/// letter with its octave marks.
fn pitch_to_abc(pitch: &Pitch, acc: Option<i32>) -> String {
    let mut s = match acc {
        Some(0) => "=".to_string(),
        Some(a) if a > 0 => "^".repeat(a as usize),
        Some(a) => "_".repeat((-a) as usize),
        None => String::new(),
    };
    let letter = pitch.step.name().chars().next().unwrap_or('C');
    if pitch.octave >= 5 {
        s.push(letter.to_ascii_lowercase());
        s.push_str(&"'".repeat((pitch.octave - 5) as usize));
    } else {
        s.push(letter.to_ascii_uppercase());
        s.push_str(&",".repeat((4 - pitch.octave) as usize));
    }
    s
}

/// Render one note/chord/rest as its ABC token (with a trailing `-` for a tie).
fn sounding_token(m: &Music, acc: &mut Accidentals, grace: bool) -> Option<String> {
    let mut note = |p: &Pitch| {
        // ABC writes at most a double sharp or flat: spell anything more
        // enharmonically, in the key.
        let p = if (*p.alter.numer() / *p.alter.denom()).abs() > 2 {
            respell(*p, acc.fifths)
        } else {
            *p
        };
        pitch_to_abc(&p, acc.mark(&p, grace))
    };
    let (body, duration, annotations) = match m {
        Music::Note {
            pitch,
            duration,
            annotations,
        } => (note(pitch), duration, Some(annotations)),
        Music::Chord {
            pitches,
            duration,
            annotations,
        } => {
            // Ties live on the chord or on each note (as `lift` stores them): all
            // tied → `[CEG]2-`; some → a tie inside the chord, `[C-EG]2`.
            let all = has_tie(annotations) || pitches.iter().all(|(_, a)| has_tie(a));
            let inner: String = pitches
                .iter()
                .map(|(p, a)| {
                    let t = if !all && has_tie(a) { "-" } else { "" };
                    format!("{}{t}", note(p))
                })
                .collect();
            let tie = if all { "-" } else { "" };
            let tok = format!(
                "[{inner}]{}{tie}",
                duration_suffix(written_duration(duration))
            );
            return Some(tok);
        }
        Music::Rest { duration, .. } => ("z".to_string(), duration, None),
        // An invisible rest.
        Music::Skip { duration } => ("x".to_string(), duration, None),
        _ => return None,
    };
    let mut tok = format!("{body}{}", duration_suffix(written_duration(duration)));
    if annotations.is_some_and(|a| has_tie(a)) {
        tok.push('-');
    }
    Some(tok)
}

/// The tuplet ratio (actual, normal) of a sounding event, if it is in one.
fn tuplet_ratio(m: &Music) -> Option<(u8, u8)> {
    let d = match m {
        Music::Note { duration, .. }
        | Music::Chord { duration, .. }
        | Music::Rest { duration, .. }
        | Music::Skip { duration } => duration,
        _ => return None,
    };
    tuplet_ratio_of(d)
}

/// Duration as ABC writes it: inside a tuplet the *notated* value is printed
/// and the `(p:q:r` prefix supplies the ratio, so undo the tuplet scaling.
fn written_duration(d: &Duration) -> Frac {
    match tuplet_ratio_of(d) {
        Some((a, n)) => d.actual_duration() * Frac::new(a as i64, n as i64),
        None => d.actual_duration(),
    }
}

fn tuplet_ratio_of(d: &Duration) -> Option<(u8, u8)> {
    (d.tuplet_actual != d.tuplet_normal && d.tuplet_actual > 0 && d.tuplet_normal > 0)
        .then_some((d.tuplet_actual, d.tuplet_normal))
}

/// Render a duration as an ABC multiplier of the unit length.
fn duration_suffix(dur: Frac) -> String {
    let mult = dur / UNIT_LENGTH;
    let num = *mult.numer();
    let den = *mult.denom();
    if num == 1 && den == 1 {
        String::new()
    } else if den == 1 {
        num.to_string()
    } else if num == 1 {
        format!("/{den}")
    } else {
        format!("{num}/{den}")
    }
}

fn meter_to_abc(ts: &TimeSignature) -> String {
    match ts.symbol.as_deref() {
        Some("common") => "C".to_string(),
        Some("cut") => "C|".to_string(),
        _ => format!("{}/{}", ts.beats, ts.beat_type),
    }
}

fn barline_to_abc(b: &Barline) -> String {
    if b.ending_type.as_deref() == Some("start") {
        return format!("[{}", b.ending_number.unwrap_or(1));
    }
    // The repeat sign first: MusicXML reads `light-heavy` + backward repeat
    // as a final bar line that repeats.
    match (b.style, b.repeat_direction) {
        (BarlineType::RepeatBoth, _) => "::".to_string(),
        (BarlineType::RepeatBackward, _) | (_, Some(RepeatDirection::Backward)) => ":|".to_string(),
        (BarlineType::RepeatForward, _) | (_, Some(RepeatDirection::Forward)) => "|:".to_string(),
        (BarlineType::Double, _) => "||".to_string(),
        (BarlineType::Final, _) => "|]".to_string(),
        _ => "|".to_string(),
    }
}

/// Render a key signature as an ABC `K:` value (tonic + mode suffix).
fn key_to_abc(key: &KeySignature) -> String {
    if key.fifths.abs() > 7 {
        return "C".to_string();
    }
    let (suffix, offset) = match key.mode {
        KeyMode::Major | KeyMode::Ionian => ("", 0),
        KeyMode::Minor | KeyMode::Aeolian => ("m", -3),
        KeyMode::Dorian => ("dor", -2),
        KeyMode::Phrygian => ("phr", -4),
        KeyMode::Lydian => ("lyd", 1),
        KeyMode::Mixolydian => ("mix", -1),
        KeyMode::Locrian => ("loc", -5),
    };
    let tonic_fifths = key.fifths - offset;
    format!("{}{suffix}", tonic_name(tonic_fifths))
}

/// Map a circle-of-fifths position to a (major-key) tonic name. Modes reach
/// past the major keys (G♯ minor is the tonic 8 fifths up, F♭ lydian 8 down).
fn tonic_name(fifths: i8) -> String {
    let i = fifths as i32 + 1; // F = 0
    let letter = ['F', 'C', 'G', 'D', 'A', 'E', 'B'][i.rem_euclid(7) as usize];
    match i.div_euclid(7) {
        0 => letter.to_string(),
        n if n > 0 => format!("{letter}{}", "#".repeat(n as usize)),
        n => format!("{letter}{}", "b".repeat((-n) as usize)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::pitch::{Alter, PitchStep};

    fn emit(doc: &MusicDocument) -> String {
        IrToAbcAdapter::new().convert_music(doc).unwrap()
    }

    fn note(step: PitchStep, octave: i32, dur: Frac) -> Music {
        Music::Note {
            pitch: Pitch::new(step, octave),
            duration: crate::ir::duration::Duration::new(dur),
            annotations: vec![],
        }
    }

    fn staff(events: Vec<Music>) -> MusicDocument {
        MusicDocument::new(
            Music::Sequential(events).in_context(crate::ir::music::ContextType::Staff, None),
        )
    }

    #[test]
    fn test_header_and_simple_notes() {
        let mut doc = staff(vec![
            Music::TimeSignature(TimeSignature {
                beats: "4".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            Music::KeySignature(KeySignature {
                fifths: 1,
                mode: KeyMode::Major,
            }),
            note(PitchStep::G, 4, Frac::new(1, 8)),
            note(PitchStep::A, 4, Frac::new(1, 4)),
        ]);
        doc.metadata.title = Some("Tune".to_string());
        let abc = emit(&doc);
        assert!(abc.contains("X:1"));
        assert!(abc.contains("T:Tune"));
        assert!(abc.contains("M:4/4"));
        assert!(abc.contains("L:1/8"));
        assert!(abc.contains("K:G"));
        // G eighth = "G"; A quarter = "A2".
        assert!(abc.contains("G A2"), "body was: {abc}");
    }

    #[test]
    fn test_pitch_octaves_and_accidentals() {
        let doc = staff(vec![
            note(PitchStep::C, 4, Frac::new(1, 8)),
            note(PitchStep::C, 5, Frac::new(1, 8)),
            note(PitchStep::C, 3, Frac::new(1, 8)),
            note(PitchStep::C, 6, Frac::new(1, 8)),
            Music::Note {
                pitch: Pitch::with_alter(PitchStep::F, Alter::from_integer(1), 4),
                duration: crate::ir::duration::Duration::new(Frac::new(1, 8)),
                annotations: vec![],
            },
        ]);
        let abc = emit(&doc);
        assert!(abc.contains("C c C, c' ^F"), "body was: {abc}");
    }

    #[test]
    fn test_key_emission() {
        for (fifths, mode, expected) in [
            (0, KeyMode::Major, "K:C"),
            (1, KeyMode::Major, "K:G"),
            (-2, KeyMode::Major, "K:Bb"),
            (0, KeyMode::Minor, "K:Am"),
            (0, KeyMode::Dorian, "K:Ddor"),
        ] {
            let doc = staff(vec![
                Music::KeySignature(KeySignature { fifths, mode }),
                note(PitchStep::C, 4, Frac::new(1, 8)),
            ]);
            assert!(emit(&doc).contains(expected), "expected {expected}");
        }
    }

    fn n(step: PitchStep, alter: i32, octave: i32) -> Music {
        Music::Note {
            pitch: Pitch::with_alter(step, Alter::from_integer(alter), octave),
            duration: crate::ir::duration::Duration::new(Frac::new(1, 8)),
            annotations: vec![],
        }
    }

    fn in_key(fifths: i8, mode: KeyMode, notes: Vec<Music>) -> String {
        let mut events = vec![Music::KeySignature(KeySignature { fifths, mode })];
        events.extend(notes);
        let abc = emit(&staff(events));
        abc.lines()
            .skip_while(|l| !l.starts_with("K:"))
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_accidentals_follow_the_key_and_keep_the_spelling() {
        // F♯ in D major is in the key: no accidental. F natural needs `=`.
        assert_eq!(
            in_key(2, KeyMode::Major, vec![n(PitchStep::F, 1, 4)]).trim(),
            "F"
        );
        assert_eq!(
            in_key(2, KeyMode::Major, vec![n(PitchStep::F, 0, 4)]).trim(),
            "=F"
        );
        // The score's spelling is kept: F♯ stays F♯ in a flat key.
        assert_eq!(
            in_key(-5, KeyMode::Major, vec![n(PitchStep::F, 1, 4)]).trim(),
            "^F"
        );
        // B natural in F major.
        assert_eq!(
            in_key(-1, KeyMode::Major, vec![n(PitchStep::B, 0, 4)]).trim(),
            "=B"
        );
    }

    #[test]
    fn test_accidentals_carry_through_the_bar_under_both_rules() {
        // ^F then F♯ again: the accidental carries, nothing to print.
        let abc = in_key(
            0,
            KeyMode::Major,
            vec![n(PitchStep::F, 1, 4), n(PitchStep::F, 1, 4)],
        );
        assert_eq!(abc.trim(), "^F F");
        // ^F then F natural: must be cancelled.
        let abc = in_key(
            0,
            KeyMode::Major,
            vec![n(PitchStep::F, 1, 4), n(PitchStep::F, 0, 4)],
        );
        assert_eq!(abc.trim(), "^F =F");
        // ^F then f natural (another octave): readers disagree, so say it.
        let abc = in_key(
            0,
            KeyMode::Major,
            vec![n(PitchStep::F, 1, 4), n(PitchStep::F, 0, 5)],
        );
        assert_eq!(abc.trim(), "^F =f");
        // ^F then f sharp: readers disagree too, so say that as well.
        let abc = in_key(
            0,
            KeyMode::Major,
            vec![n(PitchStep::F, 1, 4), n(PitchStep::F, 1, 5)],
        );
        assert_eq!(abc.trim(), "^F ^f");
    }

    #[test]
    fn test_bar_line_resets_accidentals() {
        let mut notes = vec![Music::TimeSignature(crate::ir::measure::TimeSignature {
            beats: "1".into(),
            beat_type: 8,
            symbol: None,
        })];
        notes.extend([n(PitchStep::F, 1, 4), n(PitchStep::F, 1, 4)]);
        let abc = in_key(0, KeyMode::Major, notes);
        assert!(abc.contains("^F | ^F"), "{abc}");
    }

    #[test]
    fn test_every_key_has_a_name() {
        for (fifths, mode, name) in [
            (5, KeyMode::Minor, "G#m"),
            (6, KeyMode::Minor, "D#m"),
            (7, KeyMode::Minor, "A#m"),
            (-7, KeyMode::Lydian, "Fblyd"),
            (7, KeyMode::Locrian, "B#loc"),
            (-6, KeyMode::Minor, "Ebm"),
        ] {
            assert_eq!(key_to_abc(&KeySignature { fifths, mode }), name);
        }
    }

    #[test]
    fn test_durations_and_rest() {
        let doc = staff(vec![
            note(PitchStep::C, 4, Frac::new(1, 16)),
            note(PitchStep::C, 4, Frac::new(3, 16)),
            Music::Rest {
                duration: crate::ir::duration::Duration::new(Frac::new(1, 4)),
                is_measure_rest: false,
            },
        ]);
        let abc = emit(&doc);
        // 1/16 = "/2"; 3/16 = "3/2"; rest 1/4 = "z2".
        assert!(abc.contains("C/2 C3/2 z2"), "body was: {abc}");
    }

    #[test]
    fn test_empty_leading_staff_simultaneous() {
        use crate::ir::music::ContextType;
        // Mirror the lifted tree for the Music21-exported fixture:
        // Simultaneous[ empty Staff, full Staff ].
        let empty_staff =
            Music::Sequential(vec![]).in_context(ContextType::Staff, Some("P1".to_string()));
        let full_staff = Music::Sequential(vec![
            Music::TimeSignature(TimeSignature {
                beats: "2".to_string(),
                beat_type: 2,
                symbol: None,
            }),
            note(PitchStep::A, 4, Frac::new(1, 4)),
            note(PitchStep::B, 4, Frac::new(1, 4)),
        ])
        .in_context(ContextType::Staff, None);
        let doc = MusicDocument::new(Music::Simultaneous(vec![empty_staff, full_staff]));
        let abc = emit(&doc);
        assert!(abc.contains("M:2/2"), "meter missing, abc was:\n{abc}");
        assert!(abc.contains("A2 B2"), "notes missing, abc was:\n{abc}");

        // And the reverse order (full staff first) must also work.
        let empty_staff2 =
            Music::Sequential(vec![]).in_context(ContextType::Staff, Some("P1".to_string()));
        let full_staff2 = Music::Sequential(vec![
            note(PitchStep::A, 4, Frac::new(1, 4)),
            note(PitchStep::B, 4, Frac::new(1, 4)),
        ])
        .in_context(ContextType::Staff, None);
        let doc2 = MusicDocument::new(Music::Simultaneous(vec![full_staff2, empty_staff2]));
        let abc2 = emit(&doc2);
        assert!(
            abc2.contains("A2 B2"),
            "notes missing (rev), abc was:\n{abc2}"
        );
    }

    #[test]
    fn test_chord_and_barlines() {
        let doc = staff(vec![
            Music::Barline(Barline {
                style: BarlineType::RepeatForward,
                ..Default::default()
            }),
            Music::Chord {
                pitches: vec![
                    (Pitch::new(PitchStep::C, 4), vec![]),
                    (Pitch::new(PitchStep::E, 4), vec![]),
                    (Pitch::new(PitchStep::G, 4), vec![]),
                ],
                duration: crate::ir::duration::Duration::new(Frac::new(1, 4)),
                annotations: vec![],
            },
            Music::Barline(Barline {
                style: BarlineType::RepeatBackward,
                ..Default::default()
            }),
        ]);
        let abc = emit(&doc);
        assert!(abc.contains("|: [CEG]2 :|"), "body was: {abc}");
    }

    #[test]
    fn test_chord_ties_stored_on_notes() {
        // `lift` stores chord ties on each note; all tied → `[CE]2-`, some → inside.
        let chord = |ties: [bool; 2]| Music::Chord {
            pitches: vec![
                (
                    Pitch::new(PitchStep::C, 4),
                    if ties[0] {
                        vec![Annotation::TieStart]
                    } else {
                        vec![]
                    },
                ),
                (
                    Pitch::new(PitchStep::E, 4),
                    if ties[1] {
                        vec![Annotation::TieStart]
                    } else {
                        vec![]
                    },
                ),
            ],
            duration: crate::ir::duration::Duration::new(Frac::new(1, 4)),
            annotations: vec![],
        };
        let all = emit(&staff(vec![chord([true, true])]));
        assert!(all.contains("[CE]2-"), "abc:\n{all}");
        let some = emit(&staff(vec![chord([true, false])]));
        assert!(some.contains("[C-E]2"), "abc:\n{some}");
    }

    // ---- Multi-voice (ABC 2.1 V:) ----

    fn named_staff(name: &str, events: Vec<Music>) -> Music {
        Music::Sequential(events)
            .in_context(crate::ir::music::ContextType::Staff, Some(name.to_string()))
    }

    #[test]
    fn test_multivoice_emits_v_blocks() {
        let soprano = named_staff(
            "Soprano",
            vec![
                Music::KeySignature(KeySignature {
                    fifths: 0,
                    mode: KeyMode::Major,
                }),
                note(PitchStep::C, 5, Frac::new(1, 4)),
                note(PitchStep::D, 5, Frac::new(1, 4)),
            ],
        );
        let bass = named_staff(
            "Bass",
            vec![
                Music::KeySignature(KeySignature {
                    fifths: 0,
                    mode: KeyMode::Major,
                }),
                note(PitchStep::C, 3, Frac::new(1, 4)),
                note(PitchStep::D, 3, Frac::new(1, 4)),
            ],
        );
        let doc = MusicDocument::new(Music::Simultaneous(vec![soprano, bass]));
        let abc = emit(&doc);
        // Two voice blocks with names; one shared K: in the header.
        assert!(abc.contains("V:1 name=\"Soprano\""), "abc:\n{abc}");
        assert!(abc.contains("V:2 name=\"Bass\""), "abc:\n{abc}");
        assert_eq!(
            abc.matches("K:").count(),
            1,
            "key should be header-only:\n{abc}"
        );
        // Soprano body c2 d2 (octave 5 = lowercase); bass C2 D2 (octave 3).
        assert!(abc.contains("c2 d2"), "soprano body missing:\n{abc}");
        assert!(
            abc.contains("C, D,") || abc.contains("C,2 D,2"),
            "bass body missing:\n{abc}"
        );
    }

    #[test]
    fn test_voice_in_another_key_states_it() {
        // Header K: comes from the first voice (D major); a voice in C must say
        // so, or a standard reader plays its F as F♯.
        let d = named_staff(
            "Up",
            vec![
                Music::KeySignature(KeySignature {
                    fifths: 2,
                    mode: KeyMode::Major,
                }),
                note(PitchStep::F, 5, Frac::new(1, 4)),
            ],
        );
        let c = named_staff(
            "Down",
            vec![
                Music::KeySignature(KeySignature {
                    fifths: 0,
                    mode: KeyMode::Major,
                }),
                note(PitchStep::F, 3, Frac::new(1, 4)),
            ],
        );
        let abc = emit(&MusicDocument::new(Music::Simultaneous(vec![d, c])));
        assert!(abc.contains("K:D"), "abc:\n{abc}");
        assert!(abc.contains("[K:C] F,2"), "abc:\n{abc}");
    }

    #[test]
    fn test_no_blank_line_inside_the_tune() {
        // A blank line ends an ABC tune: every voice must stay inside it.
        let voice = |name: &str| {
            named_staff(
                name,
                (0..40)
                    .map(|_| note(PitchStep::C, 4, Frac::new(1, 4)))
                    .collect(),
            )
        };
        let doc = MusicDocument::new(Music::Simultaneous(vec![voice("A"), voice("B")]));
        let abc = emit(&doc);
        assert!(
            abc.trim_end().lines().all(|l| !l.trim().is_empty()),
            "abc:\n{abc}"
        );
    }

    #[test]
    fn test_single_voice_no_v_marker() {
        // A lone staff must not gain a V: block (back-compat single-line ABC).
        let doc = staff(vec![note(PitchStep::C, 4, Frac::new(1, 8))]);
        let abc = emit(&doc);
        assert!(
            !abc.contains("V:"),
            "single voice should not emit V::\n{abc}"
        );
    }
}
