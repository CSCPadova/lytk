//! IR → LilyPond emitter.
//!
//! Converts an IR [`Score`] into LilyPond source text.
//!
//! # Reference
//! Ported from the Python prototype `lytk-py/converters/ir_to_ly.py` and
//! helper functions in `lytk-py/converters/ly_emitter.py`.

use num::rational::Ratio;

use crate::ir::articulation::{StartStop, TupletDisplay};
use crate::ir::direction::BarlineType;
use crate::ir::duration::Duration;
use crate::ir::language::{pitch_name, PitchLanguage, PitchMode};
use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, TimeSignature};
use crate::ir::note::{Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::Pitch;
use crate::ir::score::{PartGroup, Score, ScoreChild};
use crate::ir::voice::Voice;
use crate::ir::Part;

use super::{FromIrAdapter, Result};

// ---------------------------------------------------------------------------
// Name maps
// ---------------------------------------------------------------------------

/// Articulation name → LilyPond suffix.
fn articulation_to_ly(name: &str) -> &str {
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

/// Ornament name → LilyPond suffix.
fn ornament_to_ly(name: &str) -> &str {
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

/// Clef (sign, line, octave_change) → LilyPond clef name.
fn clef_to_ly(clef: &Clef) -> String {
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

/// Key signature → LilyPond `\key` command (in Nederlands).
fn key_to_ly(key: &KeySignature) -> String {
    let (tonic, mode_str) = match key.mode {
        KeyMode::Minor => match key.fifths {
            -7 => ("aes", "\\minor"),
            -6 => ("ees", "\\minor"),
            -5 => ("bes", "\\minor"),
            -4 => ("f", "\\minor"),
            -3 => ("c", "\\minor"),
            -2 => ("g", "\\minor"),
            -1 => ("d", "\\minor"),
            0 => ("a", "\\minor"),
            1 => ("e", "\\minor"),
            2 => ("b", "\\minor"),
            3 => ("fis", "\\minor"),
            4 => ("cis", "\\minor"),
            5 => ("gis", "\\minor"),
            6 => ("dis", "\\minor"),
            7 => ("ais", "\\minor"),
            _ => ("a", "\\minor"),
        },
        _ => {
            let mode_cmd = match key.mode {
                KeyMode::Major | KeyMode::Ionian => "\\major",
                KeyMode::Dorian => "\\dorian",
                KeyMode::Phrygian => "\\phrygian",
                KeyMode::Lydian => "\\lydian",
                KeyMode::Mixolydian => "\\mixolydian",
                KeyMode::Aeolian | KeyMode::Minor => "\\minor",
                KeyMode::Locrian => "\\locrian",
            };
            let tonic = match key.fifths {
                -7 => "ces",
                -6 => "ges",
                -5 => "des",
                -4 => "aes",
                -3 => "ees",
                -2 => "bes",
                -1 => "f",
                0 => "c",
                1 => "g",
                2 => "d",
                3 => "a",
                4 => "e",
                5 => "b",
                6 => "fis",
                7 => "cis",
                _ => "c",
            };
            (tonic, mode_cmd)
        }
    };
    format!("\\key {tonic} {mode_str}")
}

/// Time signature → LilyPond `\time` command.
fn time_to_ly(ts: &TimeSignature) -> String {
    format!("\\time {}/{}", ts.beats, ts.beat_type)
}

/// Duration → LilyPond duration string (e.g. "4", "8.", "2..").
fn duration_to_ly(dur: &Duration) -> String {
    let base = match dur.lilypond_log() {
        Some(log) => {
            if log == 0 {
                // Whole note: lilypond_log returns log2(1) = 0 → "1"
                "1".to_string()
            } else {
                (1i32 << log).to_string()
            }
        }
        None => {
            // Fallback: try reciprocal
            let recip = dur.base.recip();
            let n = *recip.numer();
            if n > 0 {
                n.to_string()
            } else {
                "4".to_string()
            }
        }
    };

    // Handle breve (base = 2/1 → lilypond_log would be negative)
    let result = if dur.base == Ratio::new(2, 1) {
        "\\breve".to_string()
    } else if dur.base == Ratio::new(4, 1) {
        "\\longa".to_string()
    } else {
        base
    };

    let dots = ".".repeat(dur.dots as usize);
    format!("{result}{dots}")
}

/// Pitch → LilyPond pitch string with octave marks.
///
/// In absolute mode, octave marks are relative to `c` (octave 3 in our
/// numbering: LilyPond's unadorned `c` = middle-C-minus-one-octave = C3).
/// In relative mode, the closest interval to `prev` is computed and
/// extra `'` or `,` marks adjust from the inferred octave.
fn pitch_to_ly(
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

    let oct_marks = if octave_diff > 0 {
        "'".repeat(octave_diff as usize)
    } else if octave_diff < 0 {
        ",".repeat((-octave_diff) as usize)
    } else {
        String::new()
    };

    format!("{name}{oct_marks}")
}

/// Calculate the octave marks needed for LilyPond relative mode.
///
/// In relative mode each pitch is interpreted as the nearest interval (within
/// a fourth) from the previous pitch. Extra `'` or `,` marks adjust from
/// that inferred octave.
fn relative_octave(prev: &Pitch, curr: &Pitch) -> i32 {
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

/// Beat-unit name → LilyPond duration number.
fn beat_unit_to_ly(unit: &str) -> &str {
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

/// Sanitise a part id/name into a valid LilyPond variable name.
fn part_var_name(part: &Part) -> String {
    let raw = if !part.part_id.is_empty() {
        &part.part_id
    } else if !part.name.is_empty() {
        &part.name
    } else {
        "part"
    };

    // LilyPond variable names must be all-letter (no digits) so the
    // tree-sitter grammar produces an `assignment_lhs` node. Convert any
    // trailing numeric index to an alphabetic suffix.
    let alpha: String = raw.chars().filter(|c| c.is_alphabetic()).collect();
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();

    let base = if alpha.is_empty() { "part".to_string() } else { alpha };
    let mut name = camel_to_lower(&base);

    if !digits.is_empty() {
        if let Ok(n) = digits.parse::<usize>() {
            name.push_str(&index_to_alpha(n));
        }
    }

    name
}

/// Convert a 1-based index to alphabetic suffix: 1→A, 2→B, 26→Z, 27→AA
fn index_to_alpha(n: usize) -> String {
    if n == 0 {
        return "A".to_string();
    }
    let mut result = String::new();
    let mut val = n;
    while val > 0 {
        val -= 1;
        result.insert(0, (b'A' + (val % 26) as u8) as char);
        val /= 26;
    }
    result
}

/// Lowercase the first character of single-word names (e.g. "P" → "p").
fn camel_to_lower(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Simple Roman numeral for staff numbering (1–5).
fn roman(n: u8) -> &'static str {
    match n {
        1 => "I",
        2 => "II",
        3 => "III",
        4 => "IV",
        5 => "V",
        _ => "X",
    }
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// IR → LilyPond adapter.
///
/// Converts a [`Score`] into LilyPond source text.
///
/// # Example
/// ```ignore
/// use lytk::adapters::ir_to_ly::IrToLyAdapter;
/// use lytk::adapters::FromIrAdapter;
///
/// let adapter = IrToLyAdapter::new();
/// let ly_text = adapter.convert(&score)?;
/// ```
pub struct IrToLyAdapter {
    language: PitchLanguage,
    mode: PitchMode,
    version: String,
}

impl IrToLyAdapter {
    pub fn new() -> Self {
        Self {
            language: PitchLanguage::Nederlands,
            mode: PitchMode::Absolute,
            version: "2.24.0".to_string(),
        }
    }

    /// Set the pitch language.
    pub fn with_language(mut self, lang: PitchLanguage) -> Self {
        self.language = lang;
        self
    }

    /// Set the pitch mode (absolute / relative).
    pub fn with_mode(mut self, mode: PitchMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the LilyPond version string.
    pub fn with_version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }
}

impl Default for IrToLyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIrAdapter for IrToLyAdapter {
    fn convert(&self, score: &Score) -> Result<String> {
        // Resolve language: prefer score metadata, fall back to adapter config
        let lang = score
            .metadata
            .pitch_language
            .unwrap_or(self.language);
        let mode = score.metadata.pitch_mode;

        let mut lines: Vec<String> = Vec::new();

        // Preamble
        emit_preamble(score, &self.version, lang, &mut lines);

        // Part variables
        for part in score.parts() {
            emit_part_variable(part, lang, mode, &mut lines);
        }

        // Score block
        emit_score_block(score, &mut lines);

        lines.push(String::new()); // trailing newline
        Ok(lines.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// Emission helpers
// ---------------------------------------------------------------------------

fn emit_preamble(
    score: &Score,
    version: &str,
    lang: PitchLanguage,
    lines: &mut Vec<String>,
) {
    lines.push(format!("\\version \"{version}\""));
    lines.push(format!("\\language \"{}\"", lang.as_str()));
    lines.push(String::new());

    let meta = &score.metadata;
    let has_header = meta.title.is_some()
        || meta.composer.is_some()
        || meta.arranger.is_some()
        || meta.lyricist.is_some();

    if has_header {
        lines.push("\\header {".to_string());
        if let Some(t) = &meta.title {
            lines.push(format!("  title = \"{t}\""));
        }
        if let Some(s) = &meta.subtitle {
            lines.push(format!("  subtitle = \"{s}\""));
        }
        if let Some(c) = &meta.composer {
            lines.push(format!("  composer = \"{c}\""));
        }
        if let Some(a) = &meta.arranger {
            lines.push(format!("  arranger = \"{a}\""));
        }
        if let Some(l) = &meta.lyricist {
            lines.push(format!("  poet = \"{l}\""));
        }
        for (key, val) in &meta.extra {
            lines.push(format!("  {key} = \"{val}\""));
        }
        lines.push("}".to_string());
        lines.push(String::new());
    }
}

fn emit_part_variable(
    part: &Part,
    lang: PitchLanguage,
    mode: PitchMode,
    lines: &mut Vec<String>,
) {
    let var = part_var_name(part);
    let relative_prefix = if mode == PitchMode::Relative {
        // Find the first pitch to use as the reference pitch
        let first_pitch = part
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .find_map(|e| match e {
                VoiceElement::Note(n) => Some(&n.pitch),
                _ => None,
            });
        if let Some(p) = first_pitch {
            let name = pitch_name(p.step, p.alter, lang)
                .unwrap_or_else(|| p.step.name().to_lowercase());
            let oct = p.octave - 3;
            let oct_marks = if oct > 0 {
                "'".repeat(oct as usize)
            } else if oct < 0 {
                ",".repeat((-oct) as usize)
            } else {
                String::new()
            };
            format!("\\relative {name}{oct_marks} ")
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    if part.staves > 1 {
        for staff_num in 1..=part.staves {
            let staff_var = format!("{}Staff{}", var, roman(staff_num));
            lines.push(format!("{staff_var} = {relative_prefix}{{"));
            emit_measures(part, lang, mode, Some(staff_num), 2, lines);
            lines.push("}".to_string());
            lines.push(String::new());
        }
    } else {
        lines.push(format!("{var} = {relative_prefix}{{"));
        emit_measures(part, lang, mode, None, 2, lines);
        lines.push("}".to_string());
        lines.push(String::new());
    }
}

fn emit_measures(
    part: &Part,
    lang: PitchLanguage,
    mode: PitchMode,
    staff_filter: Option<u8>,
    indent: usize,
    lines: &mut Vec<String>,
) {
    let pad = " ".repeat(indent);
    let mut prev_pitch: Option<Pitch> = None;

    for measure in &part.measures {
        // Attributes
        if let Some(attrs) = &measure.attributes {
            if let Some(key) = &attrs.key {
                lines.push(format!("{pad}{}", key_to_ly(key)));
            }
            if let Some(ts) = &attrs.time {
                lines.push(format!("{pad}{}", time_to_ly(ts)));
            }
            // Clef for our staff
            let staff_num = staff_filter.unwrap_or(1);
            if let Some(clef) = attrs.clefs.get(&staff_num) {
                lines.push(format!("{pad}{}", clef_to_ly(clef)));
            } else if staff_filter.is_none() {
                // Single-staff: emit first clef
                if let Some(clef) = attrs.clefs.values().next() {
                    lines.push(format!("{pad}{}", clef_to_ly(clef)));
                }
            }
        }

        // Separate directions into standalone (tempo, rehearsal) and note-attached (dynamics, wedges, markup)
        let mut pending_dirs: Vec<String> = Vec::new();
        for dir in &measure.directions {
            // Tempo and rehearsal marks can stand alone
            if let Some(tempo) = &dir.tempo {
                lines.push(format!("{pad}{}", tempo_to_ly(tempo)));
            }
            if dir.rehearsal.is_some() {
                lines.push(format!("{pad}\\mark \\default"));
            }
            // Dynamics, wedges, text, pedal, octave shifts must attach to a note
            let mut parts: Vec<String> = Vec::new();
            if let Some(dyn_mark) = &dir.dynamic {
                parts.push(format!("\\{}", dyn_mark.sign));
            }
            if let Some(wedge) = &dir.wedge {
                let cmd = match wedge.wedge_type.as_str() {
                    "crescendo" => "\\<",
                    "diminuendo" => "\\>",
                    "stop" => "\\!",
                    _ => "",
                };
                if !cmd.is_empty() {
                    parts.push(cmd.to_string());
                }
            }
            if let Some(text) = &dir.text {
                if !text.text.is_empty() {
                    parts.push(format!("^\\markup {{ \"{}\" }}", text.text));
                }
            }
            if let Some(pedal) = &dir.pedal {
                match pedal.pedal_type.as_str() {
                    "start" => parts.push("\\sustainOn".to_string()),
                    "stop" => parts.push("\\sustainOff".to_string()),
                    "change" => parts.push("\\sustainOff\\sustainOn".to_string()),
                    _ => {}
                }
            }
            if let Some(oct) = &dir.octave_shift {
                match oct.shift_type.as_str() {
                    "up" => parts.push(format!("\\ottava #{}", oct.size / 8)),
                    "down" => parts.push(format!("\\ottava #-{}", oct.size / 8)),
                    "stop" => parts.push("\\ottava #0".to_string()),
                    _ => {}
                }
            }
            if !parts.is_empty() {
                pending_dirs.extend(parts);
            }
        }

        // Left barline
        if let Some(bl) = &measure.left_barline {
            if let Some(ref _rd) = bl.repeat_direction {
                lines.push(format!("{pad}\\repeat volta 2 {{"));
            }
        }

        // Voices
        let voices: Vec<&Voice> = if let Some(sf) = staff_filter {
            measure
                .voices
                .iter()
                .filter(|v| voice_matches_staff(v, sf))
                .collect()
        } else {
            measure.voices.iter().collect()
        };

        if voices.len() <= 1 {
            if let Some(voice) = voices.first() {
                prev_pitch =
                    emit_voice_elements(voice, lang, mode, prev_pitch, &pad, &pending_dirs, lines);
            }
        } else {
            // Multi-voice: << \\ >> syntax
            lines.push(format!("{pad}<<"));
            for (i, voice) in voices.iter().enumerate() {
                if i > 0 {
                    lines.push(format!("{pad}  \\\\"));
                }
                lines.push(format!("{pad}  {{"));
                let inner_pad = format!("{pad}    ");
                // Only attach directions to the first voice
                let dirs_for_voice = if i == 0 { &pending_dirs } else { &Vec::new() };
                prev_pitch =
                    emit_voice_elements(voice, lang, mode, prev_pitch, &inner_pad, dirs_for_voice, lines);
                lines.push(format!("{pad}  }}"));
            }
            lines.push(format!("{pad}>>"));
        }

        // Right barline
        if let Some(bl) = &measure.right_barline {
            if bl.repeat_direction.is_some() {
                lines.push(format!("{pad}}}"));
            } else {
                let bar_cmd = match bl.style {
                    BarlineType::Final => Some("\\bar \"|.\""),
                    BarlineType::Double => Some("\\bar \"||\""),
                    BarlineType::Dashed => Some("\\bar \"!\""),
                    BarlineType::RepeatBoth => Some("\\bar \":|.|:\""),
                    _ => None,
                };
                if let Some(cmd) = bar_cmd {
                    lines.push(format!("{pad}{cmd}"));
                }
            }
        }

        // Measure separator comment
        if measure.number > 0 {
            lines.push(format!("{pad}| % {}", measure.number));
        }
    }
}

/// Extract the tuplet display hint from a voice element, if present.
fn element_tuplet(elem: &VoiceElement) -> Option<&TupletDisplay> {
    match elem {
        VoiceElement::Note(n) => n.tuplet.as_ref(),
        VoiceElement::Chord(c) => c.notes.first().and_then(|n| n.tuplet.as_ref()),
        _ => None,
    }
}

/// Extract the tuplet ratio (actual, normal) from a voice element's duration.
fn element_tuplet_ratio(elem: &VoiceElement) -> (u8, u8) {
    let dur = match elem {
        VoiceElement::Note(n) => &n.duration,
        VoiceElement::Rest(r) => &r.duration,
        VoiceElement::Chord(c) => &c.duration,
        VoiceElement::Forward(f) => &f.duration,
        VoiceElement::Backup(b) => &b.duration,
    };
    (dur.tuplet_actual, dur.tuplet_normal)
}

fn emit_voice_elements(
    voice: &Voice,
    lang: PitchLanguage,
    mode: PitchMode,
    mut prev_pitch: Option<Pitch>,
    pad: &str,
    pending_dirs: &[String],
    lines: &mut Vec<String>,
) -> Option<Pitch> {
    let mut tokens: Vec<String> = Vec::new();
    let mut dirs_attached = false;
    let mut in_tuplet = false;

    for elem in &voice.elements {
        // Check for tuplet start
        if let Some(td) = element_tuplet(elem) {
            if td.tuplet_type == StartStop::Start && !in_tuplet {
                let (actual, normal) = element_tuplet_ratio(elem);
                tokens.push(format!("\\tuplet {actual}/{normal} {{"));
                in_tuplet = true;
            }
        }

        match elem {
            VoiceElement::Note(note) => {
                let mut token = note_to_ly(note, lang, mode, prev_pitch.as_ref());
                if !dirs_attached && !pending_dirs.is_empty() {
                    token = format!("{token}{}", pending_dirs.join(""));
                    dirs_attached = true;
                }
                prev_pitch = Some(note.pitch);
                tokens.push(token);
            }
            VoiceElement::Rest(rest) => {
                let mut token = rest_to_ly(rest);
                if !dirs_attached && !pending_dirs.is_empty() {
                    token = format!("{token}{}", pending_dirs.join(""));
                    dirs_attached = true;
                }
                tokens.push(token);
            }
            VoiceElement::Chord(chord) => {
                let (mut token, last) = chord_to_ly(chord, lang, mode, prev_pitch.as_ref());
                if !dirs_attached && !pending_dirs.is_empty() {
                    token = format!("{token}{}", pending_dirs.join(""));
                    dirs_attached = true;
                }
                prev_pitch = last;
                tokens.push(token);
            }
            VoiceElement::Forward(fwd) => {
                tokens.push(format!("s{}", duration_to_ly(&fwd.duration)));
            }
            VoiceElement::Backup(_) => {
                // Backups are structural; they don't emit LilyPond tokens
            }
        }

        // Check for tuplet stop
        if let Some(td) = element_tuplet(elem) {
            if td.tuplet_type == StartStop::Stop && in_tuplet {
                tokens.push("}".to_string());
                in_tuplet = false;
            }
        }
    }

    // Safety: close any unclosed tuplet
    if in_tuplet {
        tokens.push("}".to_string());
    }

    // Group tokens into lines of ~72 chars
    if !tokens.is_empty() {
        let mut current_line: Vec<&str> = Vec::new();
        let mut current_len = 0usize;
        for token in &tokens {
            current_len += token.len() + 1;
            current_line.push(token);
            if current_len > 72 {
                lines.push(format!("{pad}{}", current_line.join(" ")));
                current_line.clear();
                current_len = 0;
            }
        }
        if !current_line.is_empty() {
            lines.push(format!("{pad}{}", current_line.join(" ")));
        }
    }

    prev_pitch
}

fn note_to_ly(
    note: &Note,
    lang: PitchLanguage,
    mode: PitchMode,
    prev: Option<&Pitch>,
) -> String {
    if note.is_grace {
        return grace_note_to_ly(note, lang, mode, prev);
    }

    let p = pitch_to_ly(&note.pitch, lang, prev, mode);
    let d = duration_to_ly(&note.duration);
    let attach = attachments_to_ly(note);
    format!("{p}{d}{attach}")
}

fn grace_note_to_ly(
    note: &Note,
    lang: PitchLanguage,
    mode: PitchMode,
    prev: Option<&Pitch>,
) -> String {
    let p = pitch_to_ly(&note.pitch, lang, prev, mode);
    let d = duration_to_ly(&note.duration);
    let cmd = if note.grace_slash {
        "\\acciaccatura"
    } else {
        "\\grace"
    };
    format!("{cmd} {p}{d}")
}

fn rest_to_ly(rest: &Rest) -> String {
    let d = duration_to_ly(&rest.duration);
    if rest.is_measure_rest {
        let mut s = format!("R{d}");
        if rest.fermata.is_some() {
            s.push_str("\\fermata");
        }
        s
    } else if rest.is_spacer {
        format!("s{d}")
    } else {
        let mut s = format!("r{d}");
        if rest.fermata.is_some() {
            s.push_str("\\fermata");
        }
        s
    }
}

fn chord_to_ly(
    chord: &Chord,
    lang: PitchLanguage,
    mode: PitchMode,
    prev: Option<&Pitch>,
) -> (String, Option<Pitch>) {
    if chord.notes.is_empty() {
        return (format!("r{}", duration_to_ly(&chord.duration)), None);
    }

    let mut pitch_strs: Vec<String> = Vec::new();
    let mut last_pitch = prev.cloned();
    for n in &chord.notes {
        let p = pitch_to_ly(&n.pitch, lang, last_pitch.as_ref(), mode);
        pitch_strs.push(p);
        last_pitch = Some(n.pitch);
    }

    let d = duration_to_ly(&chord.duration);
    let attach = attachments_to_ly(&chord.notes[0]);
    let result = format!("<{}>{d}{attach}", pitch_strs.join(" "));
    (result, last_pitch)
}

fn attachments_to_ly(note: &Note) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut owned: Vec<String> = Vec::new();

    // Ties
    for tie in &note.ties {
        if tie.tie_type == StartStop::Start {
            parts.push("~");
        }
    }

    // Slurs
    for slur in &note.slurs {
        match slur.slur_type {
            StartStop::Start => parts.push("("),
            StartStop::Stop => parts.push(")"),
            _ => {}
        }
    }

    // Articulations
    for art in &note.articulations {
        let ly = articulation_to_ly(&art.name);
        if !ly.is_empty() {
            parts.push(ly);
        }
    }

    // Fermata
    if note.fermata.is_some() {
        parts.push("\\fermata");
    }

    // Ornaments
    for orn in &note.ornaments {
        let ly = ornament_to_ly(&orn.name);
        if !ly.is_empty() {
            parts.push(ly);
        }
    }

    // Dynamics (note-attached)
    for dyn_mark in &note.dynamics {
        owned.push(format!("\\{}", dyn_mark.sign));
    }

    // Wedges (note-attached)
    for wedge in &note.wedges {
        let cmd = match wedge.wedge_type.as_str() {
            "crescendo" => "\\<",
            "diminuendo" => "\\>",
            "stop" => "\\!",
            _ => "",
        };
        if !cmd.is_empty() {
            parts.push(cmd);
        }
    }

    let mut result: String = parts.join("");
    for o in &owned {
        result.push_str(o);
    }
    result
}

fn tempo_to_ly(tempo: &crate::ir::direction::TempoDirection) -> String {
    let dots = ".".repeat(tempo.dots as usize);

    match (&tempo.text, &tempo.beat_unit, tempo.per_minute) {
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

fn emit_score_block(score: &Score, lines: &mut Vec<String>) {
    lines.push("\\score {".to_string());
    lines.push("  <<".to_string());

    // Group parts by whether they appear in a PartGroup
    let mut emitted_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for child in &score.children {
        match child {
            ScoreChild::PartGroup(group) => {
                emit_part_group_ref(group, 4, lines, &mut emitted_ids);
            }
            ScoreChild::Part(part) => {
                if emitted_ids.insert(part.part_id.clone()) {
                    emit_part_ref(part, 4, lines);
                }
            }
        }
    }

    lines.push("  >>".to_string());
    lines.push("  \\layout { }".to_string());
    lines.push("  \\midi { }".to_string());
    lines.push("}".to_string());
}

fn emit_part_group_ref(
    group: &PartGroup,
    indent: usize,
    lines: &mut Vec<String>,
    emitted: &mut std::collections::HashSet<String>,
) {
    let pad = " ".repeat(indent);
    let context = if group.group_type.is_empty() {
        "StaffGroup"
    } else {
        &group.group_type
    };
    lines.push(format!("{pad}\\new {context} <<"));

    for child in &group.children {
        match child {
            ScoreChild::Part(part) => {
                if emitted.insert(part.part_id.clone()) {
                    emit_part_ref(part, indent + 2, lines);
                }
            }
            ScoreChild::PartGroup(sub) => {
                emit_part_group_ref(sub, indent + 2, lines, emitted);
            }
        }
    }

    lines.push(format!("{pad}>>"));
}

fn emit_part_ref(part: &Part, indent: usize, lines: &mut Vec<String>) {
    let pad = " ".repeat(indent);
    let var = part_var_name(part);

    if part.staves > 1 {
        lines.push(format!("{pad}\\new PianoStaff <<"));
        for staff_num in 1..=part.staves {
            let staff_var = format!("{}Staff{}", var, roman(staff_num));
            lines.push(format!(
                "{pad}  \\new Staff = \"{} {}\" \\{staff_var}",
                part.name, staff_num
            ));
        }
        lines.push(format!("{pad}>>"));
    } else if !part.name.is_empty() {
        lines.push(format!(
            "{pad}\\new Staff \\with {{ instrumentName = \"{}\" }} \\{var}",
            part.name
        ));
    } else {
        lines.push(format!("{pad}\\new Staff \\{var}"));
    }
}

fn voice_matches_staff(voice: &Voice, staff_num: u8) -> bool {
    for elem in &voice.elements {
        match elem {
            VoiceElement::Note(n) => {
                if n.staff == staff_num {
                    return true;
                }
            }
            VoiceElement::Rest(r) => {
                if r.staff == staff_num {
                    return true;
                }
            }
            VoiceElement::Chord(c) => {
                if c.staff == staff_num {
                    return true;
                }
            }
            VoiceElement::Forward(f) => {
                if f.staff == staff_num {
                    return true;
                }
            }
            VoiceElement::Backup(_) => {}
        }
    }
    // Default: include if empty or no staff info found
    true
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::ir::articulation::{Articulation, Placement, SlurEvent, TieEvent};
    use crate::ir::duration::Duration;
    use crate::ir::measure::{KeyMode, KeySignature, Measure, MeasureAttributes, TimeSignature};
    use crate::ir::note::{Note, Rest, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use num::rational::Ratio;

    fn make_note(step: PitchStep, octave: i32, dur: Duration) -> Note {
        Note::new(Pitch::new(step, octave), dur)
    }

    fn make_simple_score() -> Score {
        // C4 quarter, D4 quarter, E4 quarter, F4 quarter
        let notes: Vec<VoiceElement> = vec![
            VoiceElement::Note(Box::new(make_note(PitchStep::C, 4, Duration::quarter()))),
            VoiceElement::Note(Box::new(make_note(PitchStep::D, 4, Duration::quarter()))),
            VoiceElement::Note(Box::new(make_note(PitchStep::E, 4, Duration::quarter()))),
            VoiceElement::Note(Box::new(make_note(PitchStep::F, 4, Duration::quarter()))),
        ];
        let voice = Voice {
            number: 1,
            elements: notes,
        };
        let mut measure = Measure::new(1);
        measure.attributes = Some(MeasureAttributes {
            key: Some(KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            }),
            time: Some(TimeSignature::default()), // 4/4
            clefs: {
                let mut m = HashMap::new();
                m.insert(1, Clef::default()); // treble
                m
            },
            ..Default::default()
        });
        measure.voices.push(voice);

        let mut part = Part::new("P1");
        part.name = "Piano".to_string();
        part.measures.push(measure);

        let mut score = Score::new();
        score.metadata.title = Some("Test".to_string());
        score.children.push(ScoreChild::Part(part));
        score
    }

    #[test]
    fn test_emit_simple_score() {
        let score = make_simple_score();
        let adapter = IrToLyAdapter::new();
        let ly = adapter.convert(&score).unwrap();

        assert!(ly.contains("\\version \"2.24.0\""));
        assert!(ly.contains("\\language \"nederlands\""));
        assert!(ly.contains("title = \"Test\""));
        assert!(ly.contains("\\key c \\major"));
        assert!(ly.contains("\\time 4/4"));
        assert!(ly.contains("\\clef \"treble\""));
        // Check notes are present (absolute mode: C4 = c')
        assert!(ly.contains("c'"));
        assert!(ly.contains("d'"));
        assert!(ly.contains("e'"));
        assert!(ly.contains("f'"));
        assert!(ly.contains("\\score {"));
        assert!(ly.contains("\\layout { }"));
        assert!(ly.contains("\\midi { }"));
    }

    #[test]
    fn test_emit_duration_formats() {
        assert_eq!(duration_to_ly(&Duration::whole()), "1");
        assert_eq!(duration_to_ly(&Duration::half()), "2");
        assert_eq!(duration_to_ly(&Duration::quarter()), "4");
        assert_eq!(duration_to_ly(&Duration::eighth()), "8");
        assert_eq!(duration_to_ly(&Duration::sixteenth()), "16");
        assert_eq!(duration_to_ly(&Duration::dotted(Ratio::new(1, 4), 1)), "4.");
        assert_eq!(
            duration_to_ly(&Duration::dotted(Ratio::new(1, 4), 2)),
            "4.."
        );
    }

    #[test]
    fn test_emit_pitch_absolute() {
        // C4 = c', D5 = d'', B3 = b
        let c4 = Pitch::new(PitchStep::C, 4);
        let d5 = Pitch::new(PitchStep::D, 5);
        let b3 = Pitch::new(PitchStep::B, 3);
        let c3 = Pitch::new(PitchStep::C, 3);

        let lang = PitchLanguage::Nederlands;

        assert_eq!(pitch_to_ly(&c4, lang, None, PitchMode::Absolute), "c'");
        assert_eq!(pitch_to_ly(&d5, lang, None, PitchMode::Absolute), "d''");
        assert_eq!(pitch_to_ly(&b3, lang, None, PitchMode::Absolute), "b");
        assert_eq!(pitch_to_ly(&c3, lang, None, PitchMode::Absolute), "c");
    }

    #[test]
    fn test_emit_pitch_with_alter() {
        let fsharp4 = Pitch::with_alter(PitchStep::F, Ratio::new(1, 1), 4);
        let bflat3 = Pitch::with_alter(PitchStep::B, Ratio::new(-1, 1), 3);

        assert_eq!(
            pitch_to_ly(&fsharp4, PitchLanguage::Nederlands, None, PitchMode::Absolute),
            "fis'"
        );
        assert_eq!(
            pitch_to_ly(&bflat3, PitchLanguage::Nederlands, None, PitchMode::Absolute),
            "bes"
        );
    }

    #[test]
    fn test_emit_rest_types() {
        assert_eq!(rest_to_ly(&Rest::new(Duration::quarter())), "r4");
        assert_eq!(
            rest_to_ly(&Rest::measure_rest(Duration::whole())),
            "R1"
        );
        let mut spacer = Rest::new(Duration::half());
        spacer.is_spacer = true;
        assert_eq!(rest_to_ly(&spacer), "s2");
    }

    #[test]
    fn test_emit_key_signatures() {
        assert_eq!(
            key_to_ly(&KeySignature {
                fifths: 0,
                mode: KeyMode::Major
            }),
            "\\key c \\major"
        );
        assert_eq!(
            key_to_ly(&KeySignature {
                fifths: 2,
                mode: KeyMode::Major
            }),
            "\\key d \\major"
        );
        assert_eq!(
            key_to_ly(&KeySignature {
                fifths: -3,
                mode: KeyMode::Minor
            }),
            "\\key c \\minor"
        );
    }

    #[test]
    fn test_emit_clef() {
        assert_eq!(clef_to_ly(&Clef::default()), "\\clef \"treble\"");
        assert_eq!(
            clef_to_ly(&Clef {
                sign: ClefSign::F,
                line: 4,
                octave_change: 0
            }),
            "\\clef \"bass\""
        );
        assert_eq!(
            clef_to_ly(&Clef {
                sign: ClefSign::G,
                line: 2,
                octave_change: -1
            }),
            "\\clef \"treble_8\""
        );
    }

    #[test]
    fn test_emit_attachments() {
        let mut note = make_note(PitchStep::C, 4, Duration::quarter());
        note.ties.push(TieEvent {
            tie_type: StartStop::Start,
        });
        note.slurs.push(SlurEvent {
            slur_type: StartStop::Start,
            number: 1,
            placement: Placement::Unspecified,
        });
        note.articulations.push(Articulation {
            name: "staccato".to_string(),
            placement: Placement::Unspecified,
        });

        let attach = attachments_to_ly(&note);
        assert!(attach.contains('~'));
        assert!(attach.contains('('));
        assert!(attach.contains("-."));
    }

    #[test]
    fn test_emit_chord() {
        let notes = vec![
            make_note(PitchStep::C, 4, Duration::quarter()),
            make_note(PitchStep::E, 4, Duration::quarter()),
            make_note(PitchStep::G, 4, Duration::quarter()),
        ];
        let chord = Chord::new(Duration::quarter(), notes);
        let (ly, _) = chord_to_ly(&chord, PitchLanguage::Nederlands, PitchMode::Absolute, None);
        assert!(ly.starts_with('<'));
        assert!(ly.contains("c'"));
        assert!(ly.contains("e'"));
        assert!(ly.contains("g'"));
        assert!(ly.contains(">4"));
    }

    #[test]
    fn test_roundtrip_mxml_to_ly() {
        // Parse a MusicXML fixture, then emit as LilyPond
        use crate::adapters::mxml_to_ir::MxmlToIrAdapter;
        use crate::adapters::ToIrAdapter;

        let xml_path = std::path::Path::new("tests/fixtures/xml/01a-Pitches-Pitches.xml");
        if !xml_path.exists() {
            return; // skip if fixtures not available
        }

        let to_ir = MxmlToIrAdapter;
        let score = to_ir.convert_file(xml_path).unwrap();

        let from_ir = IrToLyAdapter::new();
        let ly = from_ir.convert(&score).unwrap();

        assert!(ly.contains("\\version"));
        assert!(ly.contains("\\score {"));
        // Should have at least some notes
        assert!(ly.len() > 100);
    }

    #[test]
    fn test_roundtrip_ly_to_ir_to_ly_with_variables() {
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::ToIrAdapter;

        let input = r#"\version "2.24.0"

melody = {
  \key g \major
  \time 3/4
  c'4 d' e' |
  f' g' a' |
}

\score {
  \new Staff \melody
  \layout {}
}
"#;
        let to_ir = LyToIrAdapter::new();
        let score = to_ir.convert_str(input).unwrap();

        // Score should have notes from the variable
        let parts = score.parts();
        assert!(!parts.is_empty());
        let notes: Vec<_> = parts[0]
            .measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                crate::ir::note::VoiceElement::Note(n) => Some(n.as_ref()),
                _ => None,
            })
            .collect();
        assert!(notes.len() >= 6, "Expected >= 6 notes, got {}", notes.len());

        // Emit back to LilyPond
        let from_ir = IrToLyAdapter::new();
        let ly = from_ir.convert(&score).unwrap();

        assert!(ly.contains("\\version"));
        assert!(ly.contains("\\score {"));
        // Should contain actual notes, not empty
        assert!(ly.contains("c'") || ly.contains("d'") || ly.contains("e'"));
    }

    #[test]
    fn test_roundtrip_ly_preserves_relative_mode() {
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::ToIrAdapter;

        let input = r#"\relative c' { c4 d e f }"#;
        let to_ir = LyToIrAdapter::new();
        let score = to_ir.convert_str(input).unwrap();

        let from_ir = IrToLyAdapter::new();
        let ly = from_ir.convert(&score).unwrap();

        // Should emit \relative since the score used relative mode
        assert!(
            ly.contains("\\relative"),
            "Output should contain \\relative when input used relative mode. Got:\n{}",
            ly
        );
    }

    #[test]
    fn test_roundtrip_all_xml_fixtures_to_ly() {
        // MusicXML → IR → LilyPond for every fixture file; verify
        // non-empty output with required structural elements.
        use crate::adapters::mxml_to_ir::MxmlToIrAdapter;
        use crate::adapters::ToIrAdapter;

        let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("xml");
        let to_ir = MxmlToIrAdapter::new();
        let from_ir = IrToLyAdapter::new();

        let mut failures: Vec<(String, String)> = Vec::new();
        let mut success_count = 0;

        for entry in std::fs::read_dir(&fixture_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "xml") {
                let xml = std::fs::read_to_string(&path).unwrap();
                let score = match to_ir.convert_str(&xml) {
                    Ok(s) => s,
                    Err(e) => {
                        failures.push((
                            path.file_name().unwrap().to_string_lossy().to_string(),
                            format!("parse: {e}"),
                        ));
                        continue;
                    }
                };

                match from_ir.convert(&score) {
                    Ok(ly) => {
                        if !ly.contains("\\version") || !ly.contains("\\score {") {
                            failures.push((
                                path.file_name().unwrap().to_string_lossy().to_string(),
                                "missing \\version or \\score block".to_string(),
                            ));
                        } else {
                            success_count += 1;
                        }
                    }
                    Err(e) => {
                        failures.push((
                            path.file_name().unwrap().to_string_lossy().to_string(),
                            format!("emit: {e}"),
                        ));
                    }
                }
            }
        }

        if !failures.is_empty() {
            let report: Vec<String> = failures
                .iter()
                .map(|(f, e)| format!("  {f}: {e}"))
                .collect();
            panic!(
                "{} of {} fixture files failed roundtrip:\n{}",
                failures.len(),
                success_count + failures.len(),
                report.join("\n")
            );
        }

        assert!(
            success_count >= 100,
            "expected at least 100 fixtures, got {success_count}"
        );
    }

    #[test]
    fn test_roundtrip_musicxml_fixture_to_ly_and_back() {
        // MusicXML → IR → LilyPond → IR (re-parse) → IR → MusicXML
        // Verify the full round-trip produces non-empty output with
        // matching part counts.
        use crate::adapters::ir_to_mxml::IrToMxmlAdapter;
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::mxml_to_ir::MxmlToIrAdapter;
        use crate::adapters::{FromIrAdapter, ToIrAdapter};

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("musicxml")
            .join("ross_32_Rossini_Duetto_for_Cello_and_Bass_mvt.1.musicxml");
        if !fixture.exists() {
            return;
        }

        // MusicXML → IR
        let mxml_to_ir = MxmlToIrAdapter::new();
        let score1 = mxml_to_ir.convert_file(&fixture).unwrap();
        let part_count = score1.parts().len();

        // IR → LilyPond
        let ir_to_ly = IrToLyAdapter::new();
        let ly = ir_to_ly.convert(&score1).unwrap();
        assert!(ly.contains("\\version"));
        assert!(ly.len() > 500, "LilyPond output unexpectedly short");

        // LilyPond → IR (re-parse)
        let ly_to_ir = LyToIrAdapter::new();
        let score2 = ly_to_ir.convert_str(&ly).unwrap();
        assert!(
            !score2.parts().is_empty(),
            "re-parsed IR should have parts"
        );

        // IR → MusicXML
        let ir_to_mxml = IrToMxmlAdapter::new();
        let mxml = ir_to_mxml.convert(&score2).unwrap();
        assert!(mxml.contains("<score-partwise"));
        assert!(mxml.contains("<part "));

        // Part count should match
        assert_eq!(
            score2.parts().len(),
            part_count,
            "re-parsed score should have same number of parts"
        );
    }

    #[test]
    fn test_emit_tuplet() {
        // Build a score with 3 notes in a 3/2 tuplet
        let notes: Vec<VoiceElement> = (0..3)
            .map(|i| {
                let mut n = Note::new(
                    Pitch::new(PitchStep::C, 4),
                    Duration::quarter(),
                );
                n.duration.tuplet_actual = 3;
                n.duration.tuplet_normal = 2;
                if i == 0 {
                    n.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Start,
                        bracket: true,
                        show_number: "actual".to_string(),
                    });
                } else if i == 2 {
                    n.tuplet = Some(TupletDisplay {
                        tuplet_type: StartStop::Stop,
                        bracket: true,
                        show_number: String::new(),
                    });
                }
                VoiceElement::Note(Box::new(n))
            })
            .collect();

        let voice = Voice { number: 1, elements: notes };
        let mut measure = Measure::new(1);
        measure.voices.push(voice);

        let mut part = Part::new("P1");
        part.measures.push(measure);

        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        let adapter = IrToLyAdapter::new();
        let ly = adapter.convert(&score).unwrap();
        assert!(ly.contains("\\tuplet 3/2"), "should emit \\tuplet 3/2: {}", ly);
    }

    #[test]
    fn test_emit_acciaccatura() {
        let mut n = Note::new(
            Pitch::new(PitchStep::E, 5),
            Duration::new(Ratio::new(1, 16)),
        );
        n.is_grace = true;
        n.grace_slash = true;
        let main = Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::quarter(),
        );
        let voice = Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(n)),
                VoiceElement::Note(Box::new(main)),
            ],
        };
        let mut measure = Measure::new(1);
        measure.voices.push(voice);

        let mut part = Part::new("P1");
        part.measures.push(measure);

        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        let adapter = IrToLyAdapter::new();
        let ly = adapter.convert(&score).unwrap();
        assert!(ly.contains("\\acciaccatura"), "should emit \\acciaccatura: {}", ly);
    }

    #[test]
    fn test_emit_grace_not_acciaccatura() {
        let mut n = Note::new(
            Pitch::new(PitchStep::E, 5),
            Duration::new(Ratio::new(1, 16)),
        );
        n.is_grace = true;
        n.grace_slash = false;
        let main = Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::quarter(),
        );
        let voice = Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(n)),
                VoiceElement::Note(Box::new(main)),
            ],
        };
        let mut measure = Measure::new(1);
        measure.voices.push(voice);

        let mut part = Part::new("P1");
        part.measures.push(measure);

        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        let adapter = IrToLyAdapter::new();
        let ly = adapter.convert(&score).unwrap();
        assert!(ly.contains("\\grace"), "should emit \\grace: {}", ly);
        assert!(!ly.contains("\\acciaccatura"), "should NOT emit \\acciaccatura: {}", ly);
    }
}
