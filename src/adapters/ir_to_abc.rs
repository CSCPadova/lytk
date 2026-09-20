//! IR → ABC notation adapter (EET2).
//!
//! Emits a Layer-1 [`MusicDocument`] as an ABC tune: an `X/T/C/M/L/K` header
//! followed by the note/rest/chord/barline body. The unit note length `L:` is
//! fixed at `1/8`; every duration is rendered as a multiple of it.
//!
//! Pitches are re-spelled against the active key signature before emission
//! (e.g. the black key above F is `^F` in a sharp key but `_G` in a flat key),
//! so output follows the key's enharmonic spelling. The sounding pitch is
//! preserved, so parse → emit → parse stays faithful on pitch and duration.
//!
//! Remaining v1 limitation: every accidental is still printed explicitly rather
//! than omitting those implied by the `K:` header (idiomatic ABC carries
//! key/within-bar accidentals); doing so requires the parser to apply the same
//! rules — tracked as a follow-up.

use crate::ir::annotation::Annotation;
use crate::ir::direction::{Barline, BarlineType};
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::{KeyMode, KeySignature, TimeSignature};
use crate::ir::music::{ContextType, Music, MusicDocument};
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
    // Active key (circle-of-fifths position) used to re-spell pitches; updated by
    // any in-body key change.
    let init_fifths = first_key.map(|k| k.fifths as i32).unwrap_or(0);
    // Bar length in whole notes, for deriving the regular bar lines.
    let init_bar = first_time.as_ref().map(|t| t.beats_fraction());

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
                first_time.is_some(),
                first_key.is_some(),
                init_fifths,
                init_bar,
            );
            out.push_str(&body);
            out.push('\n');
        }
        _ => {
            // Each voice is its own `V:n` block; the shared M:/K: live in the
            // header, so each body suppresses its own leading time/key.
            for (i, v) in voices.iter().enumerate() {
                let id = i + 1;
                match &v.name {
                    Some(n) => out.push_str(&format!("V:{id} name=\"{n}\"\n")),
                    None => out.push_str(&format!("V:{id}\n")),
                }
                let body = emit_body(
                    &v.events,
                    first_time.is_some(),
                    first_key.is_some(),
                    init_fifths,
                    init_bar,
                );
                out.push_str(&body);
                out.push('\n');
            }
        }
    }
    out
}

/// Emit one voice's body as a space-joined token string. `skip_time`/`skip_key`
/// drop the first time/key signature (already shown in the `M:`/`K:` header).
/// `init_fifths` is the key in force at the start (from the `K:` header); pitches
/// are re-spelled against the active key.
fn emit_body(
    events: &[Music],
    skip_time: bool,
    skip_key: bool,
    init_fifths: i32,
    init_bar: Option<Frac>,
) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut time_used = !skip_time;
    let mut key_used = !skip_key;
    let mut active_fifths = init_fifths;
    // Bar accounting: the IR only carries explicit `Music::Barline` events for
    // *non-default* barlines, so regular bar lines have to be derived from the
    // running meter — without them the output is one giant ABC measure.
    let mut bar_len = init_bar;
    let mut filled = Frac::new(0, 1);
    let mut bars_on_line = 0usize;
    // Sounding events still inside the open tuplet run (0 = not in a tuplet).
    let mut tuplet_left = 0usize;
    for (idx, ev) in events.iter().enumerate() {
        match tuplet_ratio(ev) {
            Some(ratio) => {
                if tuplet_left == 0 {
                    // One group per `p` notes: always fits inside a bar, so a
                    // run never straddles a barline or a wrapped line.
                    let run = events[idx..]
                        .iter()
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
        match ev {
            Music::TimeSignature(t) => {
                bar_len = Some(t.beats_fraction());
                filled = Frac::new(0, 1);
                if time_used {
                    tokens.push(format!("[M:{}]", meter_to_abc(t)));
                } else {
                    time_used = true;
                }
            }
            Music::KeySignature(k) => {
                active_fifths = k.fifths as i32;
                if key_used {
                    tokens.push(format!("[K:{}]", key_to_abc(k)));
                } else {
                    key_used = true;
                }
            }
            Music::Note { .. } | Music::Chord { .. } | Music::Rest { .. } => {
                if let Some(tok) = sounding_token(ev, active_fifths) {
                    tokens.push(tok);
                }
            }
            // Grace group: `{ab}`, or `{/a}` for an acciaccatura (ABC 2.1 §4.10).
            // Graces carry no metrical time, so they never move the bar clock.
            Music::Grace { content, slash } => {
                let mut inner = Vec::new();
                walk(content, &mut inner);
                let body: String = inner
                    .iter()
                    .filter_map(|m| sounding_token(m, active_fifths))
                    .collect::<Vec<_>>()
                    .join("");
                if !body.is_empty() {
                    tokens.push(format!("{{{}{}}}", if *slash { "/" } else { "" }, body));
                }
            }
            Music::Barline(b) => {
                tokens.push(barline_to_abc(b));
                filled = Frac::new(0, 1);
                bars_on_line += 1;
            }
            _ => {}
        }
        // Regular bar line: close the bar as soon as the meter's worth of time
        // has been emitted (explicit barlines above reset the count themselves).
        if let (Some(len), Some(d)) = (bar_len, sounding_duration(ev)) {
            if len > Frac::new(0, 1) {
                filled += d;
                if filled >= len {
                    tokens.push("|".to_string());
                    filled = Frac::new(0, 1);
                    bars_on_line += 1;
                }
            }
        }
        if bars_on_line >= BARS_PER_LINE {
            bars_on_line = 0;
            tokens.push("\n".to_string());
        }
    }
    // Join on spaces, but keep the line breaks we inserted as real newlines.
    tokens.join(" ").replace(" \n ", "\n").replace(" \n", "\n")
}

/// Sounding length of an event (notes/chords/rests advance the bar clock).
fn sounding_duration(m: &Music) -> Option<Frac> {
    match m {
        Music::Note { duration, .. }
        | Music::Chord { duration, .. }
        | Music::Rest { duration, .. } => Some(duration.actual_duration()),
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
        // ABC is single-voice in v1: pick the branch with the most leaf events
        // (the first non-empty branch), so an empty leading staff/part is skipped.
        Music::Simultaneous(items) => {
            let best = items
                .iter()
                .map(|b| {
                    let mut tmp = Vec::new();
                    walk(b, &mut tmp);
                    (tmp.len(), b)
                })
                .max_by_key(|(n, _)| *n);
            if let Some((_, branch)) = best {
                walk(branch, out);
            }
        }
        other => out.push(other.clone()),
    }
}

fn has_tie(annotations: &[Annotation]) -> bool {
    annotations.contains(&Annotation::TieStart)
}

/// Render a pitch as an ABC token (explicit accidentals + octave marks).
fn pitch_to_abc(pitch: &Pitch) -> String {
    let mut s = String::new();
    let alter = *pitch.alter.numer() / *pitch.alter.denom();
    if alter > 0 {
        s.push_str(&"^".repeat(alter as usize));
    } else if alter < 0 {
        s.push_str(&"_".repeat((-alter) as usize));
    }
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
fn sounding_token(m: &Music, active_fifths: i32) -> Option<String> {
    let (body, duration, annotations) = match m {
        Music::Note {
            pitch,
            duration,
            annotations,
        } => (
            pitch_to_abc(&respell(*pitch, active_fifths)),
            duration,
            Some(annotations),
        ),
        Music::Chord {
            pitches,
            duration,
            annotations,
        } => (
            format!(
                "[{}]",
                pitches
                    .iter()
                    .map(|(p, _)| pitch_to_abc(&respell(*p, active_fifths)))
                    .collect::<String>()
            ),
            duration,
            Some(annotations),
        ),
        Music::Rest { duration, .. } => ("z".to_string(), duration, None),
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
        | Music::Rest { duration, .. } => duration,
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
    match b.style {
        BarlineType::RepeatForward => "|:".to_string(),
        BarlineType::RepeatBackward => ":|".to_string(),
        BarlineType::RepeatBoth => "::".to_string(),
        BarlineType::Double => "||".to_string(),
        BarlineType::Final => "|]".to_string(),
        _ => "|".to_string(),
    }
}

/// Render a key signature as an ABC `K:` value (tonic + mode suffix).
fn key_to_abc(key: &KeySignature) -> String {
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

/// Map a circle-of-fifths position to a (major-key) tonic name.
fn tonic_name(fifths: i8) -> &'static str {
    match fifths {
        -7 => "Cb",
        -6 => "Gb",
        -5 => "Db",
        -4 => "Ab",
        -3 => "Eb",
        -2 => "Bb",
        -1 => "F",
        0 => "C",
        1 => "G",
        2 => "D",
        3 => "A",
        4 => "E",
        5 => "B",
        6 => "F#",
        7 => "C#",
        _ => "C",
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

    #[test]
    fn test_key_aware_respelling() {
        // The black key above F: spelled `^F` (F♯) in a sharp key, `_G` (G♭) in a
        // flat key — the sounding pitch is identical, the spelling follows K:.
        let fsharp = || Music::Note {
            pitch: Pitch::with_alter(PitchStep::F, Alter::from_integer(1), 4),
            duration: crate::ir::duration::Duration::new(Frac::new(1, 8)),
            annotations: vec![],
        };
        let sharp_key = staff(vec![
            Music::KeySignature(KeySignature {
                fifths: 2,
                mode: KeyMode::Major,
            }),
            fsharp(),
        ]);
        assert!(
            emit(&sharp_key).contains("^F"),
            "sharp key should keep F#: {}",
            emit(&sharp_key)
        );

        let flat_key = staff(vec![
            Music::KeySignature(KeySignature {
                fifths: -5,
                mode: KeyMode::Major,
            }),
            fsharp(),
        ]);
        let abc = emit(&flat_key);
        assert!(abc.contains("_G"), "flat key should respell as Gb:\n{abc}");
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
