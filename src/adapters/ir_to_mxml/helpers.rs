//! Utility functions for IR → MusicXML conversion.

use crate::ir::articulation::StartStop;
use crate::ir::duration::Frac;
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

/// Divisions used when no `u16` value represents every duration exactly (the
/// lcm of several coprime tuplets, 3·5·7·9·11·13 = 135135 already, does not
/// fit): 10080 = 2^5·3^2·5·7 is exact for every tuplet up to 10 and for many
/// beyond, and other durations are rounded.
const FALLBACK_DIVISIONS: u16 = 10080;

/// Compute divisions per quarter note that exactly represent all durations in
/// the score (including tuplets and short durations), or
/// [`FALLBACK_DIVISIONS`] when that number does not fit MusicXML's `u16`.
pub(super) fn compute_score_divisions(score: &Score, base: u16) -> u16 {
    // A multiple of the IR's chord-symbol/figure offset unit, so those
    // offsets convert exactly.
    let unit = crate::ir::timeline::OFFSET_DIVISIONS as u64;
    let mut result = lcm_u64(u64::from(base.max(1)), unit);
    for part in score.parts() {
        for measure in &part.measures {
            for voice in &measure.voices {
                for elem in &voice.elements {
                    let dur = match elem {
                        VoiceElement::Note(n) => &n.duration,
                        VoiceElement::Rest(r) => &r.duration,
                        VoiceElement::Chord(c) => &c.duration,
                    };
                    // Every length must be a whole number of divisions: dots
                    // and tuplets included (a dotted 128th needs 64).
                    let quarters = dur.actual_duration() * Frac::from_integer(4);
                    result = result.and_then(|r| lcm_u64(r, quarters.denom().unsigned_abs()));
                }
            }
            for d in &measure.directions {
                let quarters = d.offset_frac * Frac::from_integer(4);
                result = result.and_then(|r| lcm_u64(r, quarters.denom().unsigned_abs()));
            }
        }
    }
    result
        .and_then(|r| u16::try_from(r).ok())
        .unwrap_or(FALLBACK_DIVISIONS)
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// `None` when the lcm overflows `u64`.
fn lcm_u64(a: u64, b: u64) -> Option<u64> {
    (a / gcd_u64(a, b)).checked_mul(b)
}
