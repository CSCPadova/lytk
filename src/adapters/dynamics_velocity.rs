//! Shared mapping between dynamic marks and MIDI velocities.
//!
//! Used by the MIDI adapters so that dynamics survive IR→MIDI (as note
//! velocities) and MIDI→IR (velocity quantized back to the nearest dynamic).
//! Values follow the conventional MIDI velocity ladder used by LilyPond and
//! most notation software.

/// Map a dynamic sign (e.g. `"mf"`, `"pp"`, `"sfz"`) to a MIDI velocity 1–127.
/// Unknown signs fall back to `mf` (80).
pub(crate) fn dynamic_to_velocity(sign: &str) -> u8 {
    match sign {
        "ppppp" => 5,
        "pppp" => 10,
        "ppp" => 16,
        "pp" => 33,
        "p" => 49,
        "mp" => 64,
        "mf" => 80,
        "f" => 96,
        "ff" => 112,
        "fff" => 120,
        "ffff" => 126,
        "fffff" => 127,
        // Accent-like / combined dynamics — treat as strong attacks.
        "fp" => 96,
        "sf" | "sfz" | "fz" => 112,
        "sff" | "sffz" => 120,
        "rfz" | "rf" => 112,
        "sfp" | "sfpp" => 96,
        _ => 80,
    }
}

/// Quantize a MIDI velocity (0–127) to the nearest standard dynamic sign.
pub(crate) fn velocity_to_dynamic(vel: u8) -> &'static str {
    match vel {
        0..=12 => "pppp",
        13..=24 => "ppp",
        25..=40 => "pp",
        41..=55 => "p",
        56..=70 => "mp",
        71..=85 => "mf",
        86..=100 => "f",
        101..=115 => "ff",
        _ => "fff",
    }
}
