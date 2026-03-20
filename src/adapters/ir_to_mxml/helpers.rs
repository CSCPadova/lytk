//! Utility functions for IR → MusicXML conversion.

use crate::ir::articulation::StartStop;
use crate::ir::note::VoiceElement;
use crate::ir::score::Score;

use musicxml::datatypes as mdt;

/// Map a chromatic alteration (stored as `Alter = Ratio<i32>`) to the
/// musicxml `AccidentalValue` enum.
pub(super) fn alter_to_accidental_value(alter: crate::ir::pitch::Alter) -> mdt::AccidentalValue {
    let num = *alter.numer();
    let den = *alter.denom();
    // Normalise to halves so we can match on small integers.
    let halves = num * 2 / den; // rounds toward zero
    match halves {
        -4 => mdt::AccidentalValue::FlatFlat,
        -3 => mdt::AccidentalValue::ThreeQuartersFlat,
        -2 => mdt::AccidentalValue::Flat,
        -1 => mdt::AccidentalValue::QuarterFlat,
        0 => mdt::AccidentalValue::Natural,
        1 => mdt::AccidentalValue::QuarterSharp,
        2 => mdt::AccidentalValue::Sharp,
        3 => mdt::AccidentalValue::ThreeQuartersSharp,
        4 => mdt::AccidentalValue::DoubleSharp,
        _ => mdt::AccidentalValue::Natural,
    }
}

/// Convert a `StartStop` IR enum to the musicxml `StartStopContinue` datatype.
pub(super) fn start_stop_to_mxml(ss: &StartStop) -> mdt::StartStopContinue {
    match ss {
        StartStop::Start => mdt::StartStopContinue::Start,
        StartStop::Stop => mdt::StartStopContinue::Stop,
        StartStop::Continue => mdt::StartStopContinue::Continue,
    }
}

/// Convert a `StartStop` IR enum to the musicxml `StartStop` datatype.
pub(super) fn start_stop_to_mxml_ss(ss: &StartStop) -> mdt::StartStop {
    match ss {
        StartStop::Start | StartStop::Continue => mdt::StartStop::Start,
        StartStop::Stop => mdt::StartStop::Stop,
    }
}

/// Format a float, removing trailing ".0" for integer values.
pub(super) fn format_float(val: f64) -> String {
    if val == val.floor() {
        format!("{}", val as i64)
    } else {
        format!("{val}")
    }
}

// ---------------------------------------------------------------------------
// Division auto-computation
// ---------------------------------------------------------------------------

/// Compute divisions per quarter note that exactly represent all durations in
/// the score (including tuplets and short durations).
pub(super) fn compute_score_divisions(score: &Score, base: u16) -> u16 {
    let mut result = base as u64;
    for part in score.parts() {
        for measure in &part.measures {
            for voice in &measure.voices {
                for elem in &voice.elements {
                    let dur = match elem {
                        VoiceElement::Note(n) => &n.duration,
                        VoiceElement::Rest(r) => &r.duration,
                        VoiceElement::Chord(c) => &c.duration,
                    };
                    if dur.tuplet_actual > 1 {
                        result = lcm_u64(result, dur.tuplet_actual as u64);
                    }
                    let base_n = *dur.base.numer();
                    let base_d = *dur.base.denom();
                    let g = gcd_u64(base_d.unsigned_abs(), (4 * base_n).unsigned_abs());
                    let needed = base_d.unsigned_abs() / g;
                    if needed > 1 {
                        result = lcm_u64(result, needed);
                    }
                }
            }
        }
    }
    result as u16
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn lcm_u64(a: u64, b: u64) -> u64 {
    a / gcd_u64(a, b) * b
}
