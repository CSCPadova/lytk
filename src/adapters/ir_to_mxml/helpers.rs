//! Utility functions for MusicXML emission.

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::W;
use crate::adapters::Result;
use crate::ir::articulation::StartStop;

/// Write a simple `<tag>text</tag>` element.
pub(super) fn text_element(w: &mut W, tag: &str, text: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    w.write_event(Event::Text(BytesText::new(text)))?;
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

/// Format a float, removing trailing ".0" for integer values.
pub(super) fn format_float(val: f64) -> String {
    if val == val.floor() {
        format!("{}", val as i64)
    } else {
        format!("{val}")
    }
}

/// Map a chromatic alteration (stored as `Alter = Ratio<i32>`) to the
/// MusicXML accidental name used inside `<accidental>`.
pub(super) fn alter_to_accidental_name(alter: crate::ir::pitch::Alter) -> &'static str {
    let num = *alter.numer();
    let den = *alter.denom();
    // Normalise to halves so we can match on small integers.
    let halves = num * 2 / den; // rounds toward zero
    match halves {
        -4 => "double-flat",
        -3 => "three-quarters-flat",
        -2 => "flat",
        -1 => "quarter-flat",
        0 => "natural",
        1 => "quarter-sharp",
        2 => "sharp",
        3 => "three-quarters-sharp",
        4 => "double-sharp",
        _ => "natural",
    }
}

/// Convert a `StartStop` enum to its MusicXML attribute string.
pub(super) fn start_stop_str(ss: &StartStop) -> &'static str {
    match ss {
        StartStop::Start => "start",
        StartStop::Stop => "stop",
        StartStop::Continue => "continue",
    }
}

// ---------------------------------------------------------------------------
// Division auto-computation
// ---------------------------------------------------------------------------

use crate::ir::note::VoiceElement;
use crate::ir::score::Score;

/// Compute divisions per quarter note that exactly represent all durations in
/// the score (including tuplets and short durations).
///
/// Starting from `base` (typically 4), takes the LCM with every `tuplet_actual`
/// value and every base-duration denominator (in quarter-note units) found in
/// the score so that `duration_to_divisions` never truncates.
///
/// For example, a 32nd note has base = 1/32 of a whole note = 1/8 of a quarter,
/// so divisions must be divisible by 8.  A 64th note needs divisible by 16.
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
                    // Account for tuplet ratios
                    if dur.tuplet_actual > 1 {
                        result = lcm_u64(result, dur.tuplet_actual as u64);
                    }
                    // Account for short base durations: base = n/d of a whole
                    // note, so in quarter-note units the denominator is d/4n.
                    // divisions must be divisible by that denominator.
                    let base_n = *dur.base.numer();
                    let base_d = *dur.base.denom();
                    // Quarter-note fraction = base * 4 = 4n/d.
                    // For this to produce an integer when multiplied by
                    // divisions, we need divisions * 4n / d to be integer,
                    // i.e. divisions must be divisible by d / gcd(d, 4n).
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
