//! IR → ABC notation adapter (EET2).
//!
//! Emits a Layer-1 [`MusicDocument`] as an ABC tune: an `X/T/C/M/L/K` header
//! followed by the note/rest/chord/barline body. The unit note length `L:` is
//! fixed at `1/8`; every duration is rendered as a multiple of it.
//!
//! Limitation (v1): pitches are emitted with only their explicit accidentals —
//! the key signature is not used to re-spell notes. This is self-consistent
//! with [`abc_to_ir`](super::abc_to_ir) (parse → emit → parse is faithful on
//! pitch and duration) but is not idiomatic key-aware ABC.

use crate::ir::annotation::Annotation;
use crate::ir::direction::{Barline, BarlineType};
use crate::ir::duration::Frac;
use crate::ir::measure::{KeyMode, KeySignature, TimeSignature};
use crate::ir::music::{Music, MusicDocument};
use crate::ir::pitch::Pitch;

use super::{FromMusicAdapter, Result};

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

fn emit_tune(doc: &MusicDocument) -> String {
    let events = collect_events(&doc.music);

    // Header: pull the first time/key signature.
    let first_time = events.iter().find_map(|m| match m {
        Music::TimeSignature(t) => Some(t.clone()),
        _ => None,
    });
    let first_key = events.iter().find_map(|m| match m {
        Music::KeySignature(k) => Some(*k),
        _ => None,
    });

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

    // Body: emit everything after the header signatures have been consumed.
    let mut tokens: Vec<String> = Vec::new();
    let mut time_used = first_time.is_none();
    let mut key_used = first_key.is_none();
    for ev in &events {
        match ev {
            Music::TimeSignature(t) => {
                if time_used {
                    tokens.push(format!("[M:{}]", meter_to_abc(t)));
                } else {
                    time_used = true;
                }
            }
            Music::KeySignature(k) => {
                if key_used {
                    tokens.push(format!("[K:{}]", key_to_abc(k)));
                } else {
                    key_used = true;
                }
            }
            Music::Note {
                pitch,
                duration,
                annotations,
            } => {
                let mut tok = format!(
                    "{}{}",
                    pitch_to_abc(pitch),
                    duration_suffix(duration.actual_duration())
                );
                if has_tie(annotations) {
                    tok.push('-');
                }
                tokens.push(tok);
            }
            Music::Chord {
                pitches,
                duration,
                annotations,
            } => {
                let inner: String = pitches.iter().map(|(p, _)| pitch_to_abc(p)).collect();
                let mut tok = format!("[{inner}]{}", duration_suffix(duration.actual_duration()));
                if has_tie(annotations) {
                    tok.push('-');
                }
                tokens.push(tok);
            }
            Music::Rest { duration, .. } => {
                tokens.push(format!("z{}", duration_suffix(duration.actual_duration())));
            }
            Music::Barline(b) => tokens.push(barline_to_abc(b)),
            _ => {}
        }
    }

    out.push_str(&tokens.join(" "));
    out.push('\n');
    out
}

/// Flatten a Music tree into its leaf event sequence (in order).
fn collect_events(music: &Music) -> Vec<Music> {
    let mut out = Vec::new();
    walk(music, &mut out);
    out
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
}
