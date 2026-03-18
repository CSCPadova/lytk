//! Part and harmony/figured-bass variable emission.

use num::rational::Ratio;

use crate::ir::duration::Duration;
use crate::ir::language::{pitch_name, PitchLanguage, PitchMode};
use crate::ir::note::VoiceElement;
use crate::ir::Part;

use super::emit::emit_measures;
use super::helpers::{part_var_name, roman};
use super::maps::{chord_pitch_to_ly, duration_to_ly, figure_to_ly, harmony_kind_to_ly};

/// Emit a `\chordmode` variable if any measure has harmonies.
pub(super) fn emit_harmony_variable(part: &Part, lines: &mut Vec<String>) {
    let has_any = part.measures.iter().any(|m| !m.harmonies.is_empty());
    if !has_any {
        return;
    }

    let var = format!("{}Chords", part_var_name(part));
    lines.push(format!("{var} = \\chordmode {{"));

    // Track time signature for measure durations
    let mut ts_beats: i64 = 4;
    let mut ts_beat_type: i64 = 4;

    for measure in &part.measures {
        if let Some(attrs) = &measure.attributes {
            if let Some(ts) = &attrs.time {
                // Parse beats (may be compound like "3+2")
                ts_beats = ts
                    .beats
                    .split('+')
                    .filter_map(|s| s.trim().parse::<i64>().ok())
                    .sum::<i64>()
                    .max(1);
                ts_beat_type = ts.beat_type as i64;
            }
        }

        if measure.harmonies.is_empty() {
            // Spacer for full measure
            let measure_frac = Ratio::new(ts_beats, ts_beat_type);
            let dur = Duration::new(measure_frac);
            lines.push(format!("  s{}", duration_to_ly(&dur)));
        } else if measure.harmonies.len() == 1 {
            let h = &measure.harmonies[0];
            let root = chord_pitch_to_ly(&h.root);
            let kind = harmony_kind_to_ly(&h.kind);
            let bass = h
                .bass
                .as_ref()
                .map(|b| format!("/{}", chord_pitch_to_ly(b)))
                .unwrap_or_default();
            let measure_frac = Ratio::new(ts_beats, ts_beat_type);
            let dur = Duration::new(measure_frac);
            lines.push(format!("  {root}{kind}{bass}{}", duration_to_ly(&dur)));
        } else {
            // Multiple harmonies: divide measure evenly
            let n = measure.harmonies.len() as i64;
            let each_frac = Ratio::new(ts_beats, ts_beat_type * n);
            let dur = Duration::new(each_frac);
            let mut tokens: Vec<String> = Vec::new();
            for h in &measure.harmonies {
                let root = chord_pitch_to_ly(&h.root);
                let kind = harmony_kind_to_ly(&h.kind);
                let bass = h
                    .bass
                    .as_ref()
                    .map(|b| format!("/{}", chord_pitch_to_ly(b)))
                    .unwrap_or_default();
                tokens.push(format!("{root}{kind}{bass}{}", duration_to_ly(&dur)));
            }
            lines.push(format!("  {}", tokens.join(" ")));
        }
    }

    lines.push("}".to_string());
    lines.push(String::new());
}

/// Emit a `\figuremode` variable if any measure has figured bass.
pub(super) fn emit_figured_bass_variable(part: &Part, lines: &mut Vec<String>) {
    let has_any = part.measures.iter().any(|m| !m.figured_bass.is_empty());
    if !has_any {
        return;
    }

    let var = format!("{}Figures", part_var_name(part));
    lines.push(format!("{var} = \\figuremode {{"));

    // Divisions per quarter note -- must match FIGURED_BASS_DIVISIONS in ly_to_ir.rs
    // and DEFAULT_DIVISIONS in ir_to_mxml.rs.
    const DIVISIONS: i64 = 4;
    // Fractions are in units of whole notes (= 4 quarter notes = 4 * DIVISIONS divs)
    let divs_per_whole = 4 * DIVISIONS; // 16

    // Track time signature for measure duration
    let mut ts_beats: i64 = 4;
    let mut ts_beat_type: i64 = 4;

    for measure in &part.measures {
        if let Some(attrs) = &measure.attributes {
            if let Some(ts) = &attrs.time {
                ts_beats = ts
                    .beats
                    .split('+')
                    .filter_map(|s| s.trim().parse::<i64>().ok())
                    .sum::<i64>()
                    .max(1);
                ts_beat_type = ts.beat_type as i64;
            }
        }

        // Measure duration in divisions
        // (ts_beats / ts_beat_type) * divs_per_whole
        let measure_divs = ts_beats * divs_per_whole / ts_beat_type;

        if measure.figured_bass.is_empty() {
            // Whole-measure spacer
            let measure_frac = Ratio::new(ts_beats, ts_beat_type);
            let dur = Duration::new(measure_frac);
            lines.push(format!("  s{}", duration_to_ly(&dur)));
        } else {
            let mut tokens: Vec<String> = Vec::new();
            let mut elapsed_divs: i64 = 0; // tracks position through this measure

            // Sort figures by offset so we process them in time order
            let mut figs: Vec<&crate::ir::harmony::FiguredBass> =
                measure.figured_bass.iter().collect();
            figs.sort_by_key(|f| f.offset);

            for fb in &figs {
                let fig_offset = fb.offset as i64;

                // Emit a spacer for any gap before this figure
                if fig_offset > elapsed_divs {
                    let gap_divs = fig_offset - elapsed_divs;
                    // Convert gap_divs back to a Duration fraction of whole note
                    let gap_frac = Ratio::new(gap_divs, divs_per_whole);
                    let gap_dur = Duration::new(gap_frac);
                    tokens.push(format!("s{}", duration_to_ly(&gap_dur)));
                    elapsed_divs = fig_offset;
                }

                let figs_str: Vec<String> = fb.figures.iter().map(figure_to_ly).collect();
                let d = duration_to_ly(&fb.duration);
                tokens.push(format!("<{}>{d}", figs_str.join(" ")));

                // Advance elapsed by figure duration
                let fig_dur_divs = {
                    let actual = fb.duration.actual_duration();
                    let q = actual * crate::ir::duration::Frac::from_integer(4);
                    let divs = q * crate::ir::duration::Frac::from_integer(DIVISIONS);
                    *divs.numer() / *divs.denom()
                };
                elapsed_divs += fig_dur_divs;
            }

            // Trailing spacer if figures don't fill the measure
            if elapsed_divs < measure_divs {
                let gap_divs = measure_divs - elapsed_divs;
                let gap_frac = Ratio::new(gap_divs, divs_per_whole);
                let gap_dur = Duration::new(gap_frac);
                tokens.push(format!("s{}", duration_to_ly(&gap_dur)));
            }

            lines.push(format!("  {}", tokens.join(" ")));
        }
    }

    lines.push("}".to_string());
    lines.push(String::new());
}

pub(super) fn emit_part_variable(
    part: &Part,
    lang: PitchLanguage,
    mode: PitchMode,
    partial_dur: Option<&Duration>,
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

    // MIDI instrument setting (emitted at the top of the part variable body)
    let midi_set = if !part.midi_instrument.is_empty() {
        let name = part.midi_instrument.to_lowercase();
        format!("  \\set Staff.midiInstrument = \"{name}\"\n")
    } else {
        String::new()
    };

    if part.staves > 1 {
        for staff_num in 1..=part.staves {
            let staff_var = format!("{}Staff{}", var, roman(staff_num));
            lines.push(format!("{staff_var} = {relative_prefix}{{"));
            if !midi_set.is_empty() {
                // trim trailing newline -- push as a separate line
                lines.push(midi_set.trim_end().to_string());
            }
            emit_measures(part, lang, mode, Some(staff_num), partial_dur, 2, lines);
            lines.push("}".to_string());
            lines.push(String::new());
        }
    } else {
        lines.push(format!("{var} = {relative_prefix}{{"));
        if !midi_set.is_empty() {
            lines.push(midi_set.trim_end().to_string());
        }
        emit_measures(part, lang, mode, None, partial_dur, 2, lines);
        lines.push("}".to_string());
        lines.push(String::new());
    }
}
