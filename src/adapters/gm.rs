//! General MIDI instrument table — the canonical bridge for MIDI instrument
//! preservation across formats.
//!
//! Different formats carry instrument identity differently:
//! - **LilyPond** gives a *name* (`\set Staff.midiInstrument = "violin"`).
//! - **MIDI** gives a *program number* (a Program Change, 0-indexed 0–127).
//! - **MusicXML** carries *both* a `<midi-name>` and a `<midi-program>` (the
//!   latter 1-indexed, 1–128, per the MusicXML spec).
//!
//! To preserve the instrument through any conversion, the IR keeps both
//! [`Part::midi_instrument`](crate::ir::part::Part::midi_instrument) (the GM
//! name) and [`Part::midi_program`](crate::ir::part::Part::midi_program) (the
//! **0-indexed** MIDI program), and each adapter cross-populates the missing
//! half through this table:
//! - [`gm_program_from_name`] — name → 0-indexed program (None if unknown).
//! - [`gm_name_from_program`] — 0-indexed program → canonical GM name.
//!
//! The 128 canonical names are LilyPond's `midiInstrument` names, so a name
//! produced here round-trips back to a valid LilyPond instrument.

/// The 128 General MIDI instrument names (LilyPond convention), indexed by the
/// **0-indexed** program number.
pub const GM_NAMES: [&str; 128] = [
    // Piano (0–7)
    "acoustic grand",
    "bright acoustic",
    "electric grand",
    "honky-tonk",
    "electric piano 1",
    "electric piano 2",
    "harpsichord",
    "clav",
    // Chromatic percussion (8–15)
    "celesta",
    "glockenspiel",
    "music box",
    "vibraphone",
    "marimba",
    "xylophone",
    "tubular bells",
    "dulcimer",
    // Organ (16–23)
    "drawbar organ",
    "percussive organ",
    "rock organ",
    "church organ",
    "reed organ",
    "accordion",
    "harmonica",
    "concertina",
    // Guitar (24–31)
    "acoustic guitar (nylon)",
    "acoustic guitar (steel)",
    "electric guitar (jazz)",
    "electric guitar (clean)",
    "electric guitar (muted)",
    "overdriven guitar",
    "distorted guitar",
    "guitar harmonics",
    // Bass (32–39)
    "acoustic bass",
    "electric bass (finger)",
    "electric bass (pick)",
    "fretless bass",
    "slap bass 1",
    "slap bass 2",
    "synth bass 1",
    "synth bass 2",
    // Strings (40–47)
    "violin",
    "viola",
    "cello",
    "contrabass",
    "tremolo strings",
    "pizzicato strings",
    "orchestral harp",
    "timpani",
    // Ensemble (48–55)
    "string ensemble 1",
    "string ensemble 2",
    "synthstrings 1",
    "synthstrings 2",
    "choir aahs",
    "voice oohs",
    "synth voice",
    "orchestra hit",
    // Brass (56–63)
    "trumpet",
    "trombone",
    "tuba",
    "muted trumpet",
    "french horn",
    "brass section",
    "synthbrass 1",
    "synthbrass 2",
    // Reed (64–71)
    "soprano sax",
    "alto sax",
    "tenor sax",
    "baritone sax",
    "oboe",
    "english horn",
    "bassoon",
    "clarinet",
    // Pipe (72–79)
    "piccolo",
    "flute",
    "recorder",
    "pan flute",
    "blown bottle",
    "shakuhachi",
    "whistle",
    "ocarina",
    // Synth lead (80–87)
    "lead 1 (square)",
    "lead 2 (sawtooth)",
    "lead 3 (calliope)",
    "lead 4 (chiff)",
    "lead 5 (charang)",
    "lead 6 (voice)",
    "lead 7 (fifths)",
    "lead 8 (bass+lead)",
    // Synth pad (88–95)
    "pad 1 (new age)",
    "pad 2 (warm)",
    "pad 3 (polysynth)",
    "pad 4 (choir)",
    "pad 5 (bowed)",
    "pad 6 (metallic)",
    "pad 7 (halo)",
    "pad 8 (sweep)",
    // Synth effects (96–103)
    "fx 1 (rain)",
    "fx 2 (soundtrack)",
    "fx 3 (crystal)",
    "fx 4 (atmosphere)",
    "fx 5 (brightness)",
    "fx 6 (goblins)",
    "fx 7 (echoes)",
    "fx 8 (sci-fi)",
    // Ethnic (104–111)
    "sitar",
    "banjo",
    "shamisen",
    "koto",
    "kalimba",
    "bagpipe",
    "fiddle",
    "shanai",
    // Percussive (112–119)
    "tinkle bell",
    "agogo",
    "steel drums",
    "woodblock",
    "taiko drum",
    "melodic tom",
    "synth drum",
    "reverse cymbal",
    // Sound effects (120–127)
    "guitar fret noise",
    "breath noise",
    "seashore",
    "bird tweet",
    "telephone ring",
    "helicopter",
    "applause",
    "gunshot",
];

/// The canonical GM name for a 0-indexed program number, or `None` if out of
/// range (a program is 0–127).
pub fn gm_name_from_program(program: u8) -> Option<&'static str> {
    GM_NAMES.get(program as usize).copied()
}

/// The 0-indexed GM program for an instrument name, or `None` if unrecognised.
///
/// Matches the canonical names first (so a name from [`gm_name_from_program`]
/// always round-trips), then a set of common aliases / alternate spellings.
pub fn gm_program_from_name(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    let n = n.trim();
    if let Some(i) = GM_NAMES.iter().position(|&g| g == n) {
        return Some(i as u8);
    }
    let prog = match n {
        "acoustic grand piano" => 0,
        "bright acoustic piano" => 1,
        "electric grand piano" => 2,
        "honky-tonk piano" => 3,
        "rhodes piano" => 4,
        "chorused piano" => 5,
        "clavinet" => 7,
        "church organ reed" => 19,
        "nylon string guitar" => 24,
        "steel string guitar" => 25,
        "electric bass" => 33,
        "double bass" => 43,
        "harp" => 46,
        "string ensemble" => 48,
        "synth strings 1" => 50,
        "synth strings 2" => 51,
        "synth brass 1" => 62,
        "synth brass 2" => 63,
        "bottle" => 76,
        "square" => 80,
        "sawtooth" => 81,
        _ => return None,
    };
    Some(prog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_program_round_trip() {
        // Every canonical name maps back to its own index.
        for (i, &name) in GM_NAMES.iter().enumerate() {
            assert_eq!(gm_program_from_name(name), Some(i as u8), "{name}");
            assert_eq!(gm_name_from_program(i as u8), Some(name));
        }
    }

    #[test]
    fn known_instruments() {
        assert_eq!(gm_program_from_name("violin"), Some(40));
        assert_eq!(gm_program_from_name("VIOLIN"), Some(40));
        assert_eq!(gm_program_from_name(" flute "), Some(73));
        assert_eq!(gm_name_from_program(40), Some("violin"));
        assert_eq!(gm_name_from_program(0), Some("acoustic grand"));
    }

    #[test]
    fn aliases() {
        assert_eq!(gm_program_from_name("harp"), Some(46));
        assert_eq!(gm_program_from_name("double bass"), Some(43));
        assert_eq!(gm_program_from_name("clavinet"), Some(7));
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(gm_program_from_name("kazoo orchestra"), None);
        assert_eq!(gm_name_from_program(200), None);
    }
}
