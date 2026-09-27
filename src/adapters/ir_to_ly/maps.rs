//! Mapping/conversion functions: articulation, ornament, clef, key, time,
//! duration, pitch, harmony, figured-bass, tempo.

use num::rational::Ratio;

use crate::ir::duration::Duration;
use crate::ir::harmony::{ChordPitch, Figure};
use crate::ir::language::{pitch_name, PitchLanguage, PitchMode};
use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, TimeSignature};
use crate::ir::note::Note;
use crate::ir::pitch::Pitch;

/// Articulation name -> LilyPond suffix.
pub(super) fn articulation_to_ly(name: &str) -> &str {
    match name {
        "accent" => "->",
        "strong-accent" => "-^",
        "staccato" => "-.",
        "staccatissimo" => "-!",
        "tenuto" => "--",
        "detached-legato" => "-_",
        "stress" => "->",
        "unstress" => "-!",
        "spiccato" => "-.",
        "breath-mark" => "\\breathe",
        _ => "",
    }
}

/// Ornament name -> LilyPond suffix.
pub(super) fn ornament_to_ly(name: &str) -> &str {
    match name {
        "trill-mark" => "\\trill",
        "mordent" => "\\mordent",
        "inverted-mordent" => "\\prall",
        "turn" => "\\turn",
        "inverted-turn" => "\\reverseturn",
        "shake" => "\\shake",
        "tremolo" => ":",
        _ => "",
    }
}

/// Clef (sign, line, octave_change) -> LilyPond clef name.
pub(super) fn clef_to_ly(clef: &Clef) -> String {
    let base = match (clef.sign, clef.line) {
        (ClefSign::G, 2) => "treble",
        (ClefSign::G, 1) => "french",
        (ClefSign::C, 1) => "soprano",
        (ClefSign::C, 2) => "mezzosoprano",
        (ClefSign::C, 3) => "alto",
        (ClefSign::C, 4) => "tenor",
        (ClefSign::C, 5) => "baritone",
        (ClefSign::F, 4) => "bass",
        (ClefSign::F, 3) => "varbaritone",
        (ClefSign::F, 5) => "subbass",
        (ClefSign::Percussion, _) => "percussion",
        (ClefSign::Tab, _) => "tab",
        _ => "treble",
    };

    let suffix = match clef.octave_change {
        1 => "^8",
        -1 => "_8",
        2 => "^15",
        -2 => "_15",
        _ => "",
    };

    format!("\\clef \"{base}{suffix}\"")
}

/// Key signature -> LilyPond `\key` command, with the tonic spelled in the
/// emitted `\language` (a Nederlands "fis" under `\language "english"` would
/// not compile and was silently dropped on re-parse).
pub(super) fn key_to_ly(key: &KeySignature, lang: PitchLanguage) -> String {
    use crate::ir::pitch::PitchStep;
    // Tonic as (step, alter in semitones) on the circle of fifths.
    let (step, alter, mode_str): (PitchStep, i32, &str) = match key.mode {
        KeyMode::Minor | KeyMode::Aeolian => {
            let (step, alter) = match key.fifths {
                -7 => (PitchStep::A, -1),
                -6 => (PitchStep::E, -1),
                -5 => (PitchStep::B, -1),
                -4 => (PitchStep::F, 0),
                -3 => (PitchStep::C, 0),
                -2 => (PitchStep::G, 0),
                -1 => (PitchStep::D, 0),
                1 => (PitchStep::E, 0),
                2 => (PitchStep::B, 0),
                3 => (PitchStep::F, 1),
                4 => (PitchStep::C, 1),
                5 => (PitchStep::G, 1),
                6 => (PitchStep::D, 1),
                7 => (PitchStep::A, 1),
                _ => (PitchStep::A, 0),
            };
            (step, alter, "\\minor")
        }
        _ => {
            let mode_cmd = match key.mode {
                KeyMode::Dorian => "\\dorian",
                KeyMode::Phrygian => "\\phrygian",
                KeyMode::Lydian => "\\lydian",
                KeyMode::Mixolydian => "\\mixolydian",
                KeyMode::Locrian => "\\locrian",
                _ => "\\major",
            };
            let (step, alter) = match key.fifths {
                -7 => (PitchStep::C, -1),
                -6 => (PitchStep::G, -1),
                -5 => (PitchStep::D, -1),
                -4 => (PitchStep::A, -1),
                -3 => (PitchStep::E, -1),
                -2 => (PitchStep::B, -1),
                -1 => (PitchStep::F, 0),
                1 => (PitchStep::G, 0),
                2 => (PitchStep::D, 0),
                3 => (PitchStep::A, 0),
                4 => (PitchStep::E, 0),
                5 => (PitchStep::B, 0),
                6 => (PitchStep::F, 1),
                7 => (PitchStep::C, 1),
                _ => (PitchStep::C, 0),
            };
            (step, alter, mode_cmd)
        }
    };
    let alter = Ratio::from_integer(alter);
    let tonic = pitch_name(step, alter, lang)
        .or_else(|| pitch_name(step, alter, PitchLanguage::Nederlands))
        .unwrap_or_else(|| step.name().to_lowercase());
    format!("\\key {tonic} {mode_str}")
}

/// Time signature -> LilyPond `\time` command.
/// `\time 3/4`; an additive meter as LilyPond writes it, `\compoundMeter
/// #'((3 2 8))` (`\time 3+2/8` is no LilyPond syntax).
pub(super) fn time_to_ly(ts: &TimeSignature) -> String {
    if ts.beats.contains('+') {
        let parts: Vec<&str> = ts.beats.split('+').map(str::trim).collect();
        format!("\\compoundMeter #'(({} {}))", parts.join(" "), ts.beat_type)
    } else {
        format!("\\time {}/{}", ts.beats, ts.beat_type)
    }
}

/// Duration -> LilyPond duration string (e.g. "4", "8.", "2..").
/// A length no value can write (a 3/4 bar rest) is a scaled whole: `1*3/4`.
pub(super) fn duration_to_ly(dur: &Duration) -> String {
    let base = if dur.base == Ratio::new(2, 1) {
        "\\breve".to_string()
    } else if dur.base == Ratio::new(4, 1) {
        "\\longa".to_string()
    } else if let Some(log) = dur.lilypond_log() {
        (1i64 << log).to_string()
    } else {
        // The dotted length, as dots can't follow a factor.
        let dotted = Duration {
            tuplet_normal: 1,
            tuplet_actual: 1,
            ..dur.clone()
        }
        .actual_duration();
        return match (*dotted.numer(), *dotted.denom()) {
            (n, 1) => format!("1*{n}"),
            (n, d) => format!("1*{n}/{d}"),
        };
    };
    let dots = ".".repeat(dur.dots as usize);
    format!("{base}{dots}")
}

/// A `\partial` length: one written value when it is one (`2.`), else a
/// multiple of its unit (`8*5`, and `4*2/3` for a tuplet's length).
pub(super) fn partial_to_ly(dur: &Duration) -> String {
    let len = dur.actual_duration();
    if let [one] = crate::ir::notate::notate(len, None).as_slice() {
        if one.actual_duration() == len {
            return duration_to_ly(one);
        }
    }
    let den = *len.denom();
    if (den as u64).is_power_of_two() {
        format!("{den}*{}", len.numer())
    } else {
        let q = len * Ratio::from_integer(4);
        format!("4*{}/{}", q.numer(), q.denom())
    }
}

/// Tremolo suffix -> `:N` for single-note tremolo (e.g. `:32`), empty if no tremolo.
/// N = base_dur_denom x 2^marks.
pub(super) fn tremolo_suffix(note: &Note) -> String {
    if note.tremolo_marks > 0 && !note.two_note_tremolo {
        let base_denom = *note.duration.base.denom() as u32;
        let n = base_denom * (1u32 << note.tremolo_marks);
        format!(":{n}")
    } else {
        String::new()
    }
}

/// Pitch -> LilyPond pitch string with octave marks.
///
/// In absolute mode, octave marks are relative to `c` (octave 3 in our
/// numbering: LilyPond's unadorned `c` = middle-C-minus-one-octave = C3).
/// In relative mode, the closest interval to `prev` is computed and
/// extra `'` or `,` marks adjust from the inferred octave.
pub(super) fn pitch_to_ly(
    pitch: &Pitch,
    lang: PitchLanguage,
    prev: Option<&Pitch>,
    mode: PitchMode,
) -> String {
    let name = pitch_name(pitch.step, pitch.alter, lang)
        .unwrap_or_else(|| pitch.step.name().to_lowercase());

    let octave_diff = match mode {
        PitchMode::Relative => {
            if let Some(prev) = prev {
                relative_octave(prev, pitch)
            } else {
                // First note in relative mode: absolute-style from c (octave 3)
                pitch.octave - 3
            }
        }
        PitchMode::Absolute => pitch.octave - 3,
    };

    let oct_marks = super::helpers::octave_marks(octave_diff);

    let acc_suffix = match pitch.accidental {
        crate::ir::pitch::AccidentalDisplay::Forced => "!",
        crate::ir::pitch::AccidentalDisplay::Cautionary => "?",
        _ => "",
    };

    format!("{name}{oct_marks}{acc_suffix}")
}

/// Calculate the octave marks needed for LilyPond relative mode.
///
/// In relative mode each pitch is interpreted as the nearest interval (within
/// a fourth) from the previous pitch. Extra `'` or `,` marks adjust from
/// that inferred octave.
pub(super) fn relative_octave(prev: &Pitch, curr: &Pitch) -> i32 {
    let prev_abs = prev.octave * 7 + prev.step.index();
    let curr_base = curr.octave * 7 + curr.step.index();

    // Find the octave that LilyPond would infer (closest within a fourth)
    let mut best_octave = curr.octave;
    let mut best_diff = (curr_base - prev_abs).abs();

    for oct_try in [curr.octave - 1, curr.octave + 1] {
        let try_abs = oct_try * 7 + curr.step.index();
        let diff = (try_abs - prev_abs).abs();
        if diff < best_diff {
            best_diff = diff;
            best_octave = oct_try;
        }
    }

    curr.octave - best_octave
}

/// Beat-unit name -> LilyPond duration number.
pub(super) fn beat_unit_to_ly(unit: &str) -> &str {
    match unit {
        "whole" => "1",
        "half" => "2",
        "quarter" => "4",
        "eighth" => "8",
        "16th" => "16",
        "32nd" => "32",
        "64th" => "64",
        "128th" => "128",
        _ => "4",
    }
}

/// ChordPitch -> LilyPond note name (Nederlands).
pub(super) fn chord_pitch_to_ly(cp: &ChordPitch) -> String {
    let base = cp.step.to_lowercase();
    let alter = if cp.alter > 0.5 {
        "is"
    } else if cp.alter < -0.5 {
        "es"
    } else {
        ""
    };
    format!("{base}{alter}")
}

/// Harmony kind -> LilyPond chordmode suffix.
pub(super) fn harmony_kind_to_ly(kind: &str) -> &str {
    match kind {
        "major" => "",
        "minor" => ":m",
        "dominant" => ":7",
        "major-seventh" => ":maj7",
        "minor-seventh" => ":m7",
        "diminished" => ":dim",
        "augmented" => ":aug",
        "half-diminished" => ":m7.5-",
        "diminished-seventh" => ":dim7",
        "major-sixth" => ":6",
        "minor-sixth" => ":m6",
        "dominant-ninth" => ":9",
        "major-ninth" => ":maj9",
        "minor-ninth" => ":m9",
        "dominant-11th" => ":11",
        "dominant-13th" => ":13",
        "suspended-second" => ":sus2",
        "suspended-fourth" => ":sus4",
        "power" => ":5",
        _ => "",
    }
}

/// Single figured bass figure -> LilyPond string.
pub(super) fn figure_to_ly(fig: &Figure) -> String {
    let num = match fig.number {
        Some(n) => n.to_string(),
        None => "_".to_string(),
    };
    let alter = match (&fig.prefix, &fig.suffix) {
        (_, Some(s)) | (Some(s), _) => match s.as_str() {
            "sharp" | "cross" => "+",
            "double-sharp" | "sharp-sharp" => "++",
            "flat" => "-",
            "double-flat" | "flat-flat" => "--",
            "natural" => "!",
            _ => "",
        },
        _ => "",
    };
    format!("{num}{alter}")
}

/// Tempo direction -> LilyPond `\tempo` command.
pub(super) fn tempo_to_ly(tempo: &crate::ir::direction::TempoDirection) -> String {
    let dots = ".".repeat(tempo.dots as usize);

    let escaped = tempo.text.as_deref().map(super::helpers::escape_ly_string);
    // LilyPond takes a whole number of beats a minute.
    let per_minute = tempo.per_minute.map(|b| b.round().max(1.0) as u32);
    match (&escaped, &tempo.beat_unit, per_minute) {
        (Some(text), Some(unit), Some(bpm)) => {
            let ly_dur = beat_unit_to_ly(unit);
            format!("\\tempo \"{text}\" {ly_dur}{dots} = {bpm}")
        }
        (None, Some(unit), Some(bpm)) => {
            let ly_dur = beat_unit_to_ly(unit);
            format!("\\tempo {ly_dur}{dots} = {bpm}")
        }
        (Some(text), _, _) => {
            format!("\\tempo \"{text}\"")
        }
        (None, None, Some(bpm)) => {
            format!("\\tempo 4 = {bpm}")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;

    #[test]
    fn partial_lengths() {
        let p = |n, d| partial_to_ly(&Duration::new(Ratio::new(n, d)));
        assert_eq!(p(1, 4), "4");
        assert_eq!(p(3, 4), "2.");
        assert_eq!(p(5, 8), "8*5");
        assert_eq!(p(1, 6), "4*2/3");
    }
}
