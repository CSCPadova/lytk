//! IR → Humdrum (`**kern`) emitter.
//!
//! Emits one kern spine per (part, voice), rightmost spine = top part
//! (kern reads low→high left→right). Time slices inside each measure are
//! aligned across spines with `.` null tokens; measures are separated by
//! numbered barline rows. Emits pitches, recip durations (dots, rational
//! `N%M` for irregular values, tuplet ratios folded in), rests, chords,
//! ties, slurs, grace notes, key/time signatures, clefs, instrument names,
//! and `!!!COM`/`!!!OTL` reference records.

use std::collections::BTreeMap;
use std::path::Path;

use crate::ir::articulation::StartStop;
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::{Clef, ClefSign, KeySignature, TimeSignature};
use crate::ir::note::{Note, VoiceElement};
use crate::ir::pitch::Pitch;
use crate::ir::score::Score;
use crate::ir::Part;

use super::{FromIrAdapter, FromMusicAdapter, Result};

/// Adapter that writes Humdrum `**kern`.
#[derive(Default)]
pub struct IrToHumdrumAdapter;

impl IrToHumdrumAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl FromIrAdapter for IrToHumdrumAdapter {
    fn convert(&self, score: &Score) -> Result<String> {
        Ok(emit_kern(score))
    }

    fn write(&self, score: &Score, path: &Path) -> Result<()> {
        std::fs::write(path, self.convert(score)?)?;
        Ok(())
    }
}

impl FromMusicAdapter for IrToHumdrumAdapter {
    fn convert_music(&self, doc: &crate::ir::music::MusicDocument) -> Result<String> {
        let score = crate::ir::lower::lower_to_score(doc);
        self.convert(&score)
    }

    fn write_music(&self, doc: &crate::ir::music::MusicDocument, path: &Path) -> Result<()> {
        std::fs::write(path, self.convert_music(doc)?)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

/// One emitted spine: a (part, voice-number) pair.
struct SpineSrc<'a> {
    part: &'a Part,
    voice: u8,
}

fn emit_kern(score: &Score) -> String {
    // Spine per (part, voice), reversed so the top part is rightmost.
    let mut spines: Vec<SpineSrc> = Vec::new();
    for part in score.parts() {
        let mut voice_numbers: Vec<u8> = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .map(|v| v.number)
            .collect();
        voice_numbers.sort_unstable();
        voice_numbers.dedup();
        if voice_numbers.is_empty() {
            voice_numbers.push(1);
        }
        for v in voice_numbers {
            spines.push(SpineSrc { part, voice: v });
        }
    }
    spines.reverse();

    let mut out = String::new();
    if let Some(c) = &score.metadata.composer {
        out.push_str(&format!("!!!COM: {c}\n"));
    }
    if let Some(t) = &score.metadata.title {
        out.push_str(&format!("!!!OTL: {t}\n"));
    }

    let row = |cells: Vec<String>| cells.join("\t") + "\n";
    let n = spines.len();
    out.push_str(&row(vec!["**kern".to_string(); n]));

    // Instrument names (only where the part declares one).
    if spines.iter().any(|s| !s.part.name.is_empty()) {
        out.push_str(&row(spines
            .iter()
            .map(|s| {
                if s.part.name.is_empty() {
                    "*".to_string()
                } else {
                    format!("*I\"{}", s.part.name)
                }
            })
            .collect()));
    }

    // Clef / key / time from each spine's first measure attributes.
    let first_attrs = |s: &SpineSrc| s.part.measures.first().and_then(|m| m.attributes.clone());
    if spines
        .iter()
        .any(|s| first_attrs(s).is_some_and(|a| !a.clefs.is_empty()))
    {
        out.push_str(&row(spines
            .iter()
            .map(|s| {
                first_attrs(s)
                    .and_then(|a| a.clefs.values().next().cloned())
                    .map(|c| format!("*clef{}", clef_str(&c)))
                    .unwrap_or_else(|| "*".to_string())
            })
            .collect()));
    }
    if spines
        .iter()
        .any(|s| first_attrs(s).is_some_and(|a| a.key.is_some()))
    {
        out.push_str(&row(spines
            .iter()
            .map(|s| {
                first_attrs(s)
                    .and_then(|a| a.key)
                    .map(|k| format!("*k[{}]", key_str(&k)))
                    .unwrap_or_else(|| "*".to_string())
            })
            .collect()));
    }
    if spines
        .iter()
        .any(|s| first_attrs(s).is_some_and(|a| a.time.is_some()))
    {
        out.push_str(&row(spines
            .iter()
            .map(|s| {
                first_attrs(s)
                    .and_then(|a| a.time.clone())
                    .map(|t| time_str(&t))
                    .unwrap_or_else(|| "*".to_string())
            })
            .collect()));
    }

    // Measures: aligned time slices with `.` padding.
    let measure_count = spines
        .iter()
        .map(|s| s.part.measures.len())
        .max()
        .unwrap_or(0);
    for mi in 0..measure_count {
        // Per spine: onset (in whole notes from measure start) → tokens.
        // Several events can share an onset — a grace note has zero duration,
        // so it sits on the same onset as the note it decorates. Each gets its
        // own kern data record (the Humdrum spelling), so the value is a Vec:
        // keying by onset alone silently dropped every grace note.
        let mut streams: Vec<BTreeMap<Frac, Vec<String>>> = Vec::with_capacity(n);
        for s in &spines {
            let mut events: BTreeMap<Frac, Vec<String>> = BTreeMap::new();
            if let Some(measure) = s.part.measures.get(mi) {
                for voice in measure.voices.iter().filter(|v| v.number == s.voice) {
                    let mut onset = Frac::from_integer(0);
                    for elem in &voice.elements {
                        let (token, dur) = element_token(elem);
                        if let Some(tok) = token {
                            events.entry(onset).or_default().push(tok);
                        }
                        onset += dur;
                    }
                }
            }
            streams.push(events);
        }
        let mut onsets: Vec<Frac> = streams.iter().flat_map(|m| m.keys().copied()).collect();
        onsets.sort();
        onsets.dedup();
        for onset in onsets {
            let depth = streams
                .iter()
                .map(|m| m.get(&onset).map_or(0, |v| v.len()))
                .max()
                .unwrap_or(0);
            // Bottom-align: the leading rows hold the graces, the last row holds
            // the metrical event every spine shares.
            for k in 0..depth {
                out.push_str(&row(streams
                    .iter()
                    .map(|m| {
                        m.get(&onset)
                            .and_then(|v| k.checked_sub(depth - v.len()).and_then(|i| v.get(i)))
                            .cloned()
                            .unwrap_or_else(|| ".".to_string())
                    })
                    .collect()));
            }
        }
        if mi + 1 < measure_count {
            out.push_str(&row(vec![format!("={}", mi + 2); n]));
        }
    }

    out.push_str(&row(vec!["==".to_string(); n]));
    out.push_str(&row(vec!["*-".to_string(); n]));
    out
}

/// Render one voice element as a kern token (grace notes have no metrical
/// duration, mirroring the parser).
fn element_token(elem: &VoiceElement) -> (Option<String>, Frac) {
    match elem {
        VoiceElement::Note(note) => {
            let tok = note_token(note);
            let dur = if note.is_grace {
                Frac::from_integer(0)
            } else {
                note.duration.actual_duration()
            };
            (Some(tok), dur)
        }
        VoiceElement::Chord(chord) => {
            let toks: Vec<String> = chord.notes.iter().map(note_token).collect();
            (Some(toks.join(" ")), chord.duration.actual_duration())
        }
        VoiceElement::Rest(rest) => {
            if rest.is_spacer {
                // Spacers occupy time but have no kern spelling; pad with `.`.
                (None, rest.duration.actual_duration())
            } else {
                (
                    Some(format!("{}r", recip_str(&rest.duration))),
                    rest.duration.actual_duration(),
                )
            }
        }
    }
}

fn note_token(note: &Note) -> String {
    let mut tok = String::new();
    let (mut tie_start, mut tie_stop) = (false, false);
    for tie in &note.ties {
        match tie.tie_type {
            StartStop::Start => tie_start = true,
            StartStop::Stop => tie_stop = true,
            StartStop::Continue => {}
        }
    }
    let slur_start = note.slurs.iter().any(|s| s.slur_type == StartStop::Start);
    let slur_stop = note.slurs.iter().any(|s| s.slur_type == StartStop::Stop);

    if tie_start && tie_stop {
        // continuation: emitted as `_` suffix below
    } else if tie_start {
        tok.push('[');
    }
    if slur_start {
        tok.push('(');
    }
    tok.push_str(&recip_str(&note.duration));
    tok.push_str(&pitch_str(&note.pitch));
    if note.is_grace {
        tok.push(if note.grace_slash { 'q' } else { 'Q' });
    }
    if tie_start && tie_stop {
        tok.push('_');
    } else if tie_stop {
        tok.push(']');
    }
    if slur_stop {
        tok.push(')');
    }
    tok
}

/// Kern recip for a Duration: `4`, `2.`, `12` (tuplet folded), `3%2`, `0`.
fn recip_str(dur: &Duration) -> String {
    // Duration = base * dots-factor * normal/actual; the recip encodes
    // base * normal/actual, dots stay symbolic.
    let base = dur.base * Frac::new(dur.tuplet_normal as i64, dur.tuplet_actual as i64);
    let (num, den) = (*base.numer(), *base.denom());
    let dots = ".".repeat(dur.dots as usize);
    if num == 2 && den == 1 {
        return format!("0{dots}"); // breve
    }
    if num == 1 {
        return format!("{den}{dots}");
    }
    // Rational recip: duration a/b whole notes → recip b%a.
    format!("{den}%{num}{dots}")
}

/// Kern pitch: `c`=C4, `cc`=C5, `C`=C3 + `#`/`-` accidentals.
fn pitch_str(pitch: &Pitch) -> String {
    let letter = format!("{:?}", pitch.step).to_ascii_lowercase();
    let octave = pitch.octave;
    let mut s = if octave >= 4 {
        letter.repeat((octave - 3) as usize)
    } else {
        letter.to_ascii_uppercase().repeat((4 - octave) as usize)
    };
    let alter = if pitch.alter.is_integer() {
        pitch.alter.to_integer()
    } else {
        0 // ponytail: kern has no standard microtone spelling; round to natural
    };
    if alter > 0 {
        s.push_str(&"#".repeat(alter as usize));
    } else if alter < 0 {
        s.push_str(&"-".repeat(-alter as usize));
    }
    s
}

fn clef_str(clef: &Clef) -> String {
    let sign = match clef.sign {
        ClefSign::G => "G",
        ClefSign::F => "F",
        ClefSign::C => "C",
        _ => "G",
    };
    let oct = match clef.octave_change {
        i8::MIN..=-1 => "v",
        1..=i8::MAX => "^",
        0 => "",
    };
    format!("{sign}{oct}{}", clef.line)
}

fn key_str(key: &KeySignature) -> String {
    const SHARPS: [&str; 7] = ["f#", "c#", "g#", "d#", "a#", "e#", "b#"];
    const FLATS: [&str; 7] = ["b-", "e-", "a-", "d-", "g-", "c-", "f-"];
    let f = key.fifths.clamp(-7, 7);
    if f >= 0 {
        SHARPS[..f as usize].concat()
    } else {
        FLATS[..(-f) as usize].concat()
    }
}

fn time_str(t: &TimeSignature) -> String {
    format!("*M{}/{}", t.beats, t.beat_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::humdrum_to_ir::HumdrumToIrAdapter;
    use crate::adapters::{ToIrAdapter, ToMusicAdapter};
    use crate::ir::pitch::PitchStep;

    fn roundtrip(kern: &str) -> (Score, Score) {
        let a = HumdrumToIrAdapter::new();
        let before = a.convert_str(kern).unwrap();
        let emitted = IrToHumdrumAdapter::new().convert(&before).unwrap();
        let after = a.convert_str(&emitted).unwrap();
        (before, after)
    }

    fn pitch_seq(score: &Score) -> Vec<(PitchStep, i32, i32)> {
        score
            .parts()
            .iter()
            .flat_map(|p| &p.measures)
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .flat_map(|e| match e {
                VoiceElement::Note(n) => {
                    vec![(n.pitch.step, n.pitch.alter.to_integer(), n.pitch.octave)]
                }
                VoiceElement::Chord(c) => c
                    .notes
                    .iter()
                    .map(|n| (n.pitch.step, n.pitch.alter.to_integer(), n.pitch.octave))
                    .collect(),
                _ => vec![],
            })
            .collect()
    }

    #[test]
    fn single_spine_round_trips() {
        let (before, after) =
            roundtrip("**kern\n*clefG2\n*k[f#]\n*M4/4\n4g\n8f#\n8e\n2d\n=2\n4c 4e 4g\n2.r\n*-\n");
        assert_eq!(pitch_seq(&before), pitch_seq(&after));
    }

    #[test]
    fn multi_spine_round_trips_in_order() {
        let kern =
            "**kern\t**kern\n*I\"Bass\t*I\"Soprano\n*M2/4\t*M2/4\n4C\t4g\n4D\t8a\n.\t8b\n*-\t*-\n";
        let (before, after) = roundtrip(kern);
        assert_eq!(before.parts().len(), 2);
        assert_eq!(pitch_seq(&before), pitch_seq(&after));
        assert_eq!(after.parts()[0].name, "Soprano");
    }

    #[test]
    fn ties_and_triplets_round_trip() {
        let (before, after) = roundtrip("**kern\n*M4/4\n[2c\n2c]\n=2\n12d\n12e\n12f\n2.r\n*-\n");
        assert_eq!(pitch_seq(&before), pitch_seq(&after));
        let durs = |s: &Score| -> Vec<Frac> {
            s.parts()
                .iter()
                .flat_map(|p| &p.measures)
                .flat_map(|m| &m.voices)
                .flat_map(|v| &v.elements)
                .filter_map(|e| match e {
                    VoiceElement::Note(n) => Some(n.duration.actual_duration()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(durs(&before), durs(&after), "durations must survive");
    }

    #[test]
    fn emits_from_music_document_too() {
        let doc = HumdrumToIrAdapter::new()
            .convert_str_to_music("**kern\n4c\n4d\n*-\n")
            .unwrap();
        let out = IrToHumdrumAdapter::new().convert_music(&doc).unwrap();
        assert!(
            out.contains("**kern") && out.contains("4c") && out.contains("*-"),
            "{out}"
        );
    }
}
