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
use crate::ir::direction::{Barline, RepeatDirection};
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
        // Kern spells beams out (`L`, `J`): what the source left to the
        // engraver is engraved first.
        let mut score = score.clone();
        crate::ir::beams::engrave(&mut score);
        Ok(emit_kern(&score))
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

impl SpineSrc<'_> {
    /// The verses the voice's notes sing.
    fn verses(&self) -> Vec<u8> {
        let set: std::collections::BTreeSet<u8> = self
            .part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .filter(|v| v.number == self.voice)
            .flat_map(|v| &v.elements)
            .flat_map(|e| e.notes().first())
            .flat_map(|n| &n.lyrics)
            .map(|l| l.number)
            .collect();
        set.into_iter().collect()
    }

    /// The staff the voice is written on (its first element's).
    fn staff(&self) -> u8 {
        self.part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .filter(|v| v.number == self.voice)
            .flat_map(|v| &v.elements)
            .map(VoiceElement::staff)
            .next()
            .unwrap_or(1)
            .max(1)
    }
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

    // Each spine's verses: a `**text` spine each, right of its kern spine.
    let verses: Vec<Vec<u8>> = spines.iter().map(SpineSrc::verses).collect();
    // A row: each kern spine's cell, then its text spines' (`text(spine,
    // verse)`).
    let row_with = |kern: Vec<String>, text: &dyn Fn(usize, u8) -> String| {
        let mut cells = Vec::new();
        for (i, cell) in kern.into_iter().enumerate() {
            cells.push(cell);
            cells.extend(verses[i].iter().map(|&v| text(i, v)));
        }
        cells.join("\t") + "\n"
    };
    // Most rows say the same in a text spine as in its kern spine.
    let row = |kern: Vec<String>| {
        let same = kern.clone();
        row_with(kern, &|i, _| same[i].clone())
    };
    let interp = |kern: Vec<String>| row_with(kern, &|_, _| "*".to_string());
    let n = spines.len();
    out.push_str(&row_with(vec!["**kern".to_string(); n], &|_, _| {
        "**text".to_string()
    }));

    // Parts and staves, numbered from the top of the score.
    let parts = score.parts();
    let first_staff: Vec<u8> = parts
        .iter()
        .scan(0u8, |next, p| {
            let first = *next;
            *next = next.saturating_add(p.staves.max(1));
            Some(first)
        })
        .collect();
    let number = |s: &SpineSrc| {
        parts
            .iter()
            .position(|p| std::ptr::eq(*p, s.part))
            .unwrap_or(0)
    };
    out.push_str(&row(spines
        .iter()
        .map(|s| format!("*part{}", number(s) + 1))
        .collect()));
    out.push_str(&row(spines
        .iter()
        .map(|s| format!("*staff{}", first_staff[number(s)] + s.staff()))
        .collect()));

    // Instrument names (only where the part declares one).
    if spines.iter().any(|s| !s.part.name.is_empty()) {
        out.push_str(&interp(
            spines
                .iter()
                .map(|s| {
                    if s.part.name.is_empty() {
                        "*".to_string()
                    } else {
                        format!("*I\"{}", s.part.name)
                    }
                })
                .collect(),
        ));
    }

    // Clef, key, meter and transposition of the first bar.
    for r in interpretation_rows(&spines, 0) {
        out.push_str(&interp(r));
    }

    // Measures: aligned time slices with `.` padding.
    let measure_count = spines
        .iter()
        .map(|s| s.part.measures.len())
        .max()
        .unwrap_or(0);
    for mi in 0..measure_count {
        // What changes at this bar (the first bar's are above).
        if mi > 0 {
            for r in interpretation_rows(&spines, mi) {
                out.push_str(&interp(r));
            }
        }
        // Per spine: onset (in whole notes from measure start) → tokens.
        // Several events can share an onset — a grace note has zero duration,
        // so it sits on the same onset as the note it decorates. Each gets its
        // own kern data record (the Humdrum spelling), so the value is a Vec:
        // keying by onset alone silently dropped every grace note.
        // Each token with the note it writes, for its syllables.
        type Stream<'a> = BTreeMap<Frac, Vec<(String, Option<&'a Note>)>>;
        let mut streams: Vec<Stream> = Vec::with_capacity(n);
        let mut ends: Vec<Frac> = Vec::with_capacity(n);
        for s in &spines {
            let mut events: Stream = BTreeMap::new();
            let mut end = Frac::from_integer(0);
            if let Some(measure) = s.part.measures.get(mi) {
                for voice in measure.voices.iter().filter(|v| v.number == s.voice) {
                    let mut onset = Frac::from_integer(0);
                    for elem in &voice.elements {
                        let (token, dur) = element_token(elem);
                        if let Some(tok) = token {
                            events
                                .entry(onset)
                                .or_default()
                                .push((tok, elem.notes().first()));
                        }
                        onset += dur;
                    }
                    end = end.max(onset);
                }
            }
            streams.push(events);
            ends.push(end);
        }
        // A voice absent from the bar, or ending early, is silent there: an
        // invisible rest (`.` would mean its last note goes on).
        let bar = ends.iter().copied().max().unwrap_or_default();
        for (events, end) in streams.iter_mut().zip(&ends) {
            if *end < bar {
                let rest = format!("{}ryy", recip_str(&Duration::new(bar - *end)));
                events.entry(*end).or_default().push((rest, None));
            }
        }
        // Clef changes inside the bar, per spine.
        let clefs: Vec<BTreeMap<Frac, String>> = spines
            .iter()
            .map(|s| {
                s.part
                    .measures
                    .get(mi)
                    .into_iter()
                    .flat_map(|m| &m.directions)
                    .filter(|d| d.staff.max(1) == s.staff())
                    .filter_map(|d| Some((d.offset_frac, format!("*clef{}", clef_str(&d.clef?)))))
                    .collect()
            })
            .collect();
        let mut onsets: Vec<Frac> = streams
            .iter()
            .flat_map(|m| m.keys().copied())
            .chain(clefs.iter().flat_map(|c| c.keys().copied()))
            .collect();
        onsets.sort();
        onsets.dedup();
        for onset in onsets {
            if clefs.iter().any(|c| c.contains_key(&onset)) {
                out.push_str(&interp(
                    clefs
                        .iter()
                        .map(|c| c.get(&onset).cloned().unwrap_or_else(|| "*".to_string()))
                        .collect(),
                ));
            }
            let depth = streams
                .iter()
                .map(|m| m.get(&onset).map_or(0, |v| v.len()))
                .max()
                .unwrap_or(0);
            // Bottom-align: the leading rows hold the graces, the last row holds
            // the metrical event every spine shares.
            for k in 0..depth {
                let cell = |s: usize| {
                    streams[s]
                        .get(&onset)
                        .and_then(|v| k.checked_sub(depth - v.len()).and_then(|i| v.get(i)))
                };
                let kern = (0..n)
                    .map(|s| cell(s).map_or_else(|| ".".to_string(), |c| c.0.clone()))
                    .collect();
                out.push_str(&row_with(kern, &|i, verse| {
                    cell(i)
                        .and_then(|c| c.1)
                        .and_then(|n| n.lyrics.iter().find(|l| l.number == verse))
                        .map_or_else(|| ".".to_string(), text_token)
                }));
            }
        }
        if mi + 1 < measure_count {
            out.push_str(&row(spines
                .iter()
                .map(|s| format!("={}{}", mi + 2, repeat_str(s.part, mi)))
                .collect()));
        }
    }

    out.push_str(&row(spines
        .iter()
        .map(|s| format!("=={}", repeat_str(s.part, measure_count.saturating_sub(1))))
        .collect()));
    out.push_str(&row(vec!["*-".to_string(); n]));
    out
}

/// The interpretation rows for what measure `mi` sets, one row per kind
/// (clef, key, meter, transposition), `*` in the spines it doesn't touch.
fn interpretation_rows(spines: &[SpineSrc], mi: usize) -> Vec<Vec<String>> {
    (0..4)
        .map(|kind| {
            spines
                .iter()
                .map(|s| interpretation(s, mi, kind))
                .collect::<Vec<_>>()
        })
        .filter(|cells| cells.iter().any(Option::is_some))
        .map(|cells| {
            cells
                .into_iter()
                .map(|c| c.unwrap_or_else(|| "*".to_string()))
                .collect()
        })
        .collect()
}

/// A syllable in a `**text` spine: `Hal-`, `-le-`, `-lu`, `jah`.
fn text_token(l: &crate::ir::articulation::LyricSyllable) -> String {
    use crate::ir::articulation::SyllabicType;
    let text = l.text.replace(['\t', '\n'], " ");
    match l.syllabic {
        SyllabicType::Single => text,
        SyllabicType::Begin => format!("{text}-"),
        SyllabicType::Middle => format!("-{text}-"),
        SyllabicType::End => format!("-{text}"),
    }
}

/// What measure `mi` sets for a spine: its clef (0), key (1), meter (2) or
/// transposition (3), as a kern interpretation.
fn interpretation(s: &SpineSrc, mi: usize, kind: usize) -> Option<String> {
    let a = s.part.measures.get(mi)?.attributes.as_ref()?;
    match kind {
        0 => a
            .clefs
            .get(&s.staff())
            .map(|c| format!("*clef{}", clef_str(c))),
        1 => a.key.map(|k| format!("*k[{}]", key_str(&k))),
        2 => a.time.as_ref().map(time_str),
        // A transposing instrument: `*ITrd-1c-2` (B♭) sounds a major second
        // below what is written.
        _ => a.transpose.map(|t| {
            let d = i32::from(t.diatonic) + 7 * i32::from(t.octave_change);
            format!("*ITrd{d}c{}", t.semitones())
        }),
    }
}

/// The repeat signs of the bar line after measure `mi`: `:|!` ends a repeat
/// there, `!|:` starts one in the next measure.
fn repeat_str(part: &Part, mi: usize) -> &'static str {
    let has = |b: Option<&Option<Barline>>, dir| {
        b.and_then(Option::as_ref)
            .is_some_and(|b| b.repeat_direction == Some(dir))
    };
    let end = has(
        part.measures.get(mi).map(|m| &m.right_barline),
        RepeatDirection::Backward,
    );
    let start = has(
        part.measures.get(mi + 1).map(|m| &m.left_barline),
        RepeatDirection::Forward,
    ) || has(
        part.measures.get(mi).map(|m| &m.right_barline),
        RepeatDirection::Forward,
    );
    match (end, start) {
        (true, true) => ":|!|:",
        (true, false) => ":|!",
        (false, true) => "!|:",
        (false, false) => "",
    }
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
                // A spacer is an invisible rest (`yy`): a null token would
                // mean the note before goes on.
                (
                    Some(format!("{}ryy", recip_str(&rest.duration))),
                    rest.duration.actual_duration(),
                )
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
    // Articulations and a fermata.
    for a in &note.articulations {
        tok.push_str(match a.name.as_str() {
            "staccato" => "'",
            "staccatissimo" => "`",
            "accent" => "^",
            "strong-accent" => "^^",
            "tenuto" => "~",
            _ => "",
        });
    }
    if note.fermata.is_some() {
        tok.push(';');
    }
    // Beams, a sign per level: `L` begins, `J` ends, `K`/`k` hooks forward
    // and back.
    for b in &note.beams {
        match b.beam_type.as_str() {
            "begin" => tok.push('L'),
            "end" => tok.push('J'),
            "forward hook" => tok.push('K'),
            "backward hook" => tok.push('k'),
            _ => {}
        }
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

    #[test]
    fn beams_are_written_and_read() {
        // Kern given no beams: the writer engraves them (2/4: by the beat).
        let kern = "**kern\n*M2/4\n8c\n8d\n16e\n16f\n8g\n*-\n";
        let (_, after) = roundtrip(kern);
        let emitted = IrToHumdrumAdapter::new()
            .convert(&HumdrumToIrAdapter::new().convert_str(kern).unwrap())
            .unwrap();
        let data: Vec<&str> = emitted
            .lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .collect();
        assert_eq!(
            data,
            ["8cL", "8dJ", "16eLL", "16fJ", "8gJ"].map(|t| t.to_string())
        );
        let firsts: Vec<String> = after.parts()[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .map(|e| {
                let n = &e.notes()[0];
                assert!(n.no_auto_beam);
                n.beams
                    .first()
                    .map_or(String::new(), |b| b.beam_type.clone())
            })
            .collect();
        assert_eq!(firsts, ["begin", "end", "begin", "", "end"]);
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
