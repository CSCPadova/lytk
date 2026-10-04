//! Mappings between dynamic marks and MIDI velocities.
//!
//! Two ladders are in use:
//! - the **common** one (MuseScore, Finale: p 49, mf 80, f 96), to read MIDI
//!   files written by other tools;
//! - **LilyPond's** (`ly/midi-init.ly`, `absolute-volume-alist` × 127: p 69,
//!   mf 86, f 95; 90 without a dynamic), used by lytk's MIDI export so that
//!   `ly → MIDI` sounds like LilyPond's own MIDI, by the note arrays so they
//!   match that export, and to read files written by LilyPond or lytk.

use crate::ir::articulation::DynamicType;

/// Quantize a MIDI velocity (0–127) to the nearest standard dynamic sign.
pub(crate) fn velocity_to_dynamic(vel: u8) -> DynamicType {
    match vel {
        0..=12 => DynamicType::Pppp,
        13..=24 => DynamicType::Ppp,
        25..=40 => DynamicType::Pp,
        41..=55 => DynamicType::P,
        56..=70 => DynamicType::Mp,
        71..=85 => DynamicType::Mf,
        86..=100 => DynamicType::F,
        101..=115 => DynamicType::Ff,
        _ => DynamicType::Fff,
    }
}

/// LilyPond's volume without any dynamic (`Audio_span_dynamic::DEFAULT_VOLUME`,
/// 90/127 — velocity 90).
pub(crate) const LILYPOND_DEFAULT_VOLUME: f64 = 90.0 / 127.0;

/// LilyPond's `absolute-volume-alist` (`ly/midi-init.ly`), volume 0–1.
const LILYPOND_VOLUMES: [(DynamicType, f64); 13] = [
    (DynamicType::Sf, 1.00),
    (DynamicType::Fffff, 0.95),
    (DynamicType::Ffff, 0.92),
    (DynamicType::Fff, 0.85),
    (DynamicType::Ff, 0.80),
    (DynamicType::F, 0.75),
    (DynamicType::Mf, 0.68),
    (DynamicType::Mp, 0.61),
    (DynamicType::P, 0.55),
    (DynamicType::Pp, 0.49),
    (DynamicType::Ppp, 0.42),
    (DynamicType::Pppp, 0.34),
    (DynamicType::Ppppp, 0.25),
];

/// The volume (0–1) LilyPond plays a dynamic at. A sign its table lacks
/// (`sfz`, `fp`, …) gets the default volume, as in LilyPond's
/// `look_up_absolute_volume`.
pub(crate) fn lilypond_volume(sign: &DynamicType) -> f64 {
    LILYPOND_VOLUMES
        .iter()
        .find(|(s, _)| s == sign)
        .map_or(LILYPOND_DEFAULT_VOLUME, |(_, v)| *v)
}

/// The velocity lytk's MIDI export plays a dynamic at before any instrument's
/// equalizer (LilyPond's volume × 127, truncated as the export does): the
/// note arrays' velocities, so they match what `to_midi` plays.
pub(crate) fn lilypond_velocity(sign: &DynamicType) -> u8 {
    (lilypond_volume(sign) * 127.0) as u8
}

/// The velocity without any dynamic (LilyPond's default volume): 90.
pub(crate) const LILYPOND_DEFAULT_VELOCITY: u8 = 90;

/// The dynamic whose LilyPond velocity (volume × 127) is nearest to `vel`, for
/// reading MIDI written by LilyPond or lytk. `sf` is an accent, not a level.
pub(crate) fn lilypond_dynamic(vel: u8) -> DynamicType {
    LILYPOND_VOLUMES[1..]
        .iter()
        .min_by_key(|(_, v)| ((v * 127.0) as i32 - vel as i32).abs())
        .map_or(DynamicType::Mf, |(s, _)| s.clone())
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
        let vel = |s: &str| (lilypond_volume(&s.into()) * 127.0) as u8;
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
        assert_eq!(lilypond_dynamic(69), DynamicType::P);
        assert_eq!(lilypond_dynamic(53), DynamicType::Ppp);
        assert_eq!(lilypond_dynamic(90), DynamicType::Mf);
    }
}
