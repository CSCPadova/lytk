//! Mappings between dynamic marks and MIDI velocities.
//!
//! Two ladders are in use:
//! - the **common** one (MuseScore, Finale: p 49, mf 80, f 96), used by the
//!   note-array encodings and to read MIDI files written by other tools;
//! - **LilyPond's** (`ly/midi-init.ly`, `absolute-volume-alist` × 127: p 69,
//!   mf 86, f 95; 90 without a dynamic), used by lytk's MIDI export so that
//!   `ly → MIDI` sounds like LilyPond's own MIDI, and to read files written
//!   by LilyPond or lytk.

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

/// LilyPond's volume without any dynamic (`Audio_span_dynamic::DEFAULT_VOLUME`,
/// 90/127 — velocity 90).
pub(crate) const LILYPOND_DEFAULT_VOLUME: f64 = 90.0 / 127.0;

/// LilyPond's `absolute-volume-alist` (`ly/midi-init.ly`), volume 0–1.
const LILYPOND_VOLUMES: [(&str, f64); 13] = [
    ("sf", 1.00),
    ("fffff", 0.95),
    ("ffff", 0.92),
    ("fff", 0.85),
    ("ff", 0.80),
    ("f", 0.75),
    ("mf", 0.68),
    ("mp", 0.61),
    ("p", 0.55),
    ("pp", 0.49),
    ("ppp", 0.42),
    ("pppp", 0.34),
    ("ppppp", 0.25),
];

/// The volume (0–1) LilyPond plays a dynamic at. A sign its table lacks
/// (`sfz`, `fp`, …) gets the default volume, as in LilyPond's
/// `look_up_absolute_volume`.
pub(crate) fn lilypond_volume(sign: &str) -> f64 {
    LILYPOND_VOLUMES
        .iter()
        .find(|(s, _)| *s == sign)
        .map_or(LILYPOND_DEFAULT_VOLUME, |(_, v)| *v)
}

/// The dynamic whose LilyPond velocity (volume × 127) is nearest to `vel`, for
/// reading MIDI written by LilyPond or lytk. `sf` is an accent, not a level.
pub(crate) fn lilypond_dynamic(vel: u8) -> &'static str {
    LILYPOND_VOLUMES[1..]
        .iter()
        .min_by_key(|(_, v)| ((v * 127.0) as i32 - vel as i32).abs())
        .map_or("mf", |(s, _)| *s)
}

/// LilyPond's `instrument-equalizer-alist` (`ly/midi-init.ly`): the (min, max)
/// volume range an instrument's dynamics are squeezed into. `None` for every
/// other instrument (piano, voices, …), whose volumes stay as they are.
pub(crate) fn lilypond_equalizer(midi_instrument: &str) -> Option<(f64, f64)> {
    Some(match midi_instrument {
        "flute" | "oboe" | "clarinet" => (0.0, 0.7),
        "bassoon" => (0.0, 0.6),
        "french horn" => (0.1, 0.7),
        "trumpet" => (0.1, 0.8),
        "timpani" => (0.2, 0.9),
        "violin" => (0.2, 1.0),
        "viola" => (0.1, 0.7),
        "cello" | "contrabass" => (0.2, 0.8),
        _ => return None,
    })
}

#[cfg(test)]
mod lilypond_tests {
    use super::*;

    #[test]
    fn lilypond_table_matches_its_midi_output() {
        // Velocities LilyPond itself writes: volume × 127, truncated.
        let vel = |s: &str| (lilypond_volume(s) * 127.0) as u8;
        for (sign, v) in [
            ("pp", 62),
            ("p", 69),
            ("mp", 77),
            ("mf", 86),
            ("f", 95),
            ("ff", 101),
            ("sf", 127),
        ] {
            assert_eq!(vel(sign), v, "{sign}");
        }
        assert_eq!(vel("sfz"), 90);
        assert_eq!((LILYPOND_DEFAULT_VOLUME * 127.0) as u8, 90);
        assert_eq!(lilypond_equalizer("violin"), Some((0.2, 1.0)));
        assert_eq!(lilypond_equalizer("acoustic grand"), None);
        assert_eq!(lilypond_dynamic(69), "p");
        assert_eq!(lilypond_dynamic(53), "ppp");
        assert_eq!(lilypond_dynamic(90), "mf");
    }
}
