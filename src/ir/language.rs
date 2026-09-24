//! Pitch-language tables for LilyPond's `\language` command.
//!
//! LilyPond supports multiple note-name languages (Nederlands, English, Italiano, …).
//! This module provides:
//! - A [`PitchLanguage`] enum for each supported language.
//! - A lookup table from `(PitchStep, Alter)` → note-name string per language.
//! - A reverse parser from note-name string → `(PitchStep, Alter)` per language.
//! - A [`PitchMode`] enum for relative/absolute pitch entry mode.
//!
//! # Data source
//! The note names and accidental suffixes are LilyPond's `\language`
//! definitions (`scm/define-note-names.scm`), first transcribed from
//! python-ly's `pitchInfo` table.

use std::fmt;

use num::rational::Ratio;
use serde::{Deserialize, Serialize};

use super::pitch::{Alter, PitchStep};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Supported LilyPond note-name languages.
///
/// Each variant corresponds to a `\language "…"` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PitchLanguage {
    #[default]
    Nederlands,
    English,
    Deutsch,
    Svenska,
    Italiano,
    Espanol,
    Portugues,
    Vlaams,
    Norsk,
    Suomi,
    Catalan,
    Francais,
}

impl PitchLanguage {
    /// The LilyPond `\language` string value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nederlands => "nederlands",
            Self::English => "english",
            Self::Deutsch => "deutsch",
            Self::Svenska => "svenska",
            Self::Italiano => "italiano",
            Self::Espanol => "espanol",
            Self::Portugues => "portugues",
            Self::Vlaams => "vlaams",
            Self::Norsk => "norsk",
            Self::Suomi => "suomi",
            Self::Catalan => "catalan",
            Self::Francais => "français",
        }
    }

    /// Parse from the LilyPond `\language` string (plus English language names).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "nederlands" | "dutch" => Some(Self::Nederlands),
            "english" => Some(Self::English),
            "deutsch" | "german" => Some(Self::Deutsch),
            "svenska" | "swedish" => Some(Self::Svenska),
            "italiano" | "italian" => Some(Self::Italiano),
            "espanol" | "español" | "spanish" => Some(Self::Espanol),
            "portugues" | "português" | "portuguese" => Some(Self::Portugues),
            "vlaams" | "flemish" => Some(Self::Vlaams),
            "norsk" | "norwegian" => Some(Self::Norsk),
            "suomi" | "finnish" => Some(Self::Suomi),
            "catalan" | "català" => Some(Self::Catalan),
            "français" | "francais" | "french" => Some(Self::Francais),
            _ => None,
        }
    }

    /// All supported languages.
    pub const ALL: [PitchLanguage; 12] = [
        Self::Nederlands,
        Self::English,
        Self::Deutsch,
        Self::Svenska,
        Self::Italiano,
        Self::Espanol,
        Self::Portugues,
        Self::Vlaams,
        Self::Norsk,
        Self::Suomi,
        Self::Catalan,
        Self::Francais,
    ];
}

impl fmt::Display for PitchLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// Default is derived via #[default] on PitchLanguage::Nederlands.

/// Pitch-entry mode: relative or absolute.
///
/// In the IR, pitches are always stored as absolute. This enum only governs
/// how the LilyPond emitter writes pitches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PitchMode {
    /// `\absolute` — octave marks relative to middle C.
    #[default]
    Absolute,
    /// `\relative` — octave marks relative to the previous note.
    Relative,
}

// ---------------------------------------------------------------------------
// Language data tables
// ---------------------------------------------------------------------------

/// A language's pitch-name data.
///
/// - `note_names`: 7 base names (C through B in that language).
/// - `alterations`: 9 accidental suffixes, indexed by alteration:
///   `[double-flat, three-quarter-flat, flat, quarter-flat, natural,
///    quarter-sharp, sharp, three-quarter-sharp, double-sharp]`.
///   Empty string `""` means that alteration is not supported.
/// - `replacements`: `(built, official)` — where base + suffix is not the name
///   LilyPond uses, the name written instead (Deutsch `ees` → `es`, `hes` → `b`).
/// - `input_names` / `input_alterations`: extra `(index, spelling)` pairs
///   accepted on input only, combining freely with the other part (français
///   `re` for `ré`, `x` for double-sharp).
/// - `input_aliases`: `(alias, built)` — other full names accepted on input.
///
/// Every written name is one LilyPond accepts for that language
/// (`scm/define-note-names.scm`).
struct LangData {
    note_names: [&'static str; 7],
    alterations: [&'static str; 9],
    replacements: &'static [(&'static str, &'static str)],
    input_names: &'static [(usize, &'static str)],
    input_alterations: &'static [(usize, &'static str)],
    input_aliases: &'static [(&'static str, &'static str)],
}

// LilyPond's note-name tables (scm/define-note-names.scm).
// Ordering: C D E F G A B
// Alteration indices: 0=double-flat  1=¾flat  2=flat  3=¼flat  4=natural
//                     5=¼sharp  6=sharp  7=¾sharp  8=double-sharp

const NEDERLANDS: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "b"],
    alterations: ["eses", "eseh", "es", "eh", "", "ih", "is", "isih", "isis"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[],
    input_aliases: &[
        ("es", "ees"),
        ("eses", "eeses"),
        ("as", "aes"),
        ("ases", "aeses"),
    ],
};

const ENGLISH: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "b"],
    alterations: ["ff", "tqf", "f", "qf", "", "qs", "s", "tqs", "ss"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[
        (0, "-flatflat"),
        (2, "-flat"),
        (4, "-natural"),
        (6, "-sharp"),
        (8, "-sharpsharp"),
        (8, "x"),
    ],
    input_aliases: &[],
};

// Also Suomi: every name written here is one Suomi accepts too.
const DEUTSCH: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "h"],
    alterations: ["eses", "eseh", "es", "eh", "", "ih", "is", "isih", "isis"],
    replacements: &[
        ("eeses", "eses"),
        ("eeseh", "eseh"),
        ("ees", "es"),
        ("aeses", "asas"),
        ("aeseh", "asah"),
        ("aes", "as"),
        ("hes", "b"),
    ],
    input_names: &[],
    input_alterations: &[],
    // `bb` is Suomi's.
    input_aliases: &[
        ("ases", "aeses"),
        ("aseh", "aeseh"),
        ("eh", "eeh"),
        ("ah", "aeh"),
        ("bes", "heses"),
        ("bb", "heses"),
    ],
};

const NORSK: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "h"],
    alterations: [
        "essess", "esseh", "ess", "eh", "", "ih", "iss", "issih", "ississ",
    ],
    replacements: &[("hessess", "bess"), ("hesseh", "beh"), ("hess", "b")],
    input_names: &[],
    // The German-style spellings are accepted too.
    input_alterations: &[
        (0, "eses"),
        (1, "eseh"),
        (2, "es"),
        (6, "is"),
        (7, "isih"),
        (8, "isis"),
    ],
    input_aliases: &[
        ("es", "eess"),
        ("ess", "eess"),
        ("eses", "eessess"),
        ("essess", "eessess"),
        ("as", "aess"),
        ("ass", "aess"),
        ("ases", "aessess"),
        ("assess", "aessess"),
        ("bes", "hessess"),
    ],
};

const SVENSKA: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "h"],
    alterations: [
        "essess", "esseh", "ess", "eh", "", "ih", "iss", "issih", "ississ",
    ],
    replacements: &[
        ("eessess", "essess"),
        ("eesseh", "esseh"),
        ("eess", "ess"),
        ("aessess", "assess"),
        ("aesseh", "asseh"),
        ("aess", "ass"),
        ("hess", "b"),
    ],
    input_names: &[],
    input_alterations: &[],
    input_aliases: &[],
};

const ITALIANO: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "bsb", "b", "sb", "", "sd", "d", "dsd", "dd"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[],
    input_aliases: &[],
};

const CATALAN: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "tqb", "b", "qb", "", "qd", "d", "tqd", "dd"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[(5, "qs"), (6, "s"), (7, "tqs"), (8, "ss")],
    input_aliases: &[],
};

const ESPANOL: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "tcb", "b", "cb", "", "cs", "s", "tcs", "ss"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[(8, "x")],
    input_aliases: &[],
};

// Names and aliases per LilyPond's `scm/define-note-names.scm` (`français`).
const FRANCAIS: LangData = LangData {
    note_names: ["do", "ré", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "bsb", "b", "sb", "", "sd", "d", "dsd", "dd"],
    replacements: &[],
    input_names: &[(1, "re")],
    input_alterations: &[(8, "x")],
    input_aliases: &[],
};

const PORTUGUES: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "btqt", "b", "bqt", "", "sqt", "s", "stqt", "ss"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[],
    input_aliases: &[],
};

const VLAAMS: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "bhb", "b", "hb", "", "hk", "k", "khk", "kk"],
    replacements: &[],
    input_names: &[],
    input_alterations: &[],
    input_aliases: &[],
};

fn lang_data(lang: PitchLanguage) -> &'static LangData {
    match lang {
        PitchLanguage::Nederlands => &NEDERLANDS,
        PitchLanguage::English => &ENGLISH,
        PitchLanguage::Deutsch | PitchLanguage::Suomi => &DEUTSCH,
        PitchLanguage::Norsk => &NORSK,
        PitchLanguage::Svenska => &SVENSKA,
        PitchLanguage::Italiano => &ITALIANO,
        PitchLanguage::Catalan => &CATALAN,
        PitchLanguage::Espanol => &ESPANOL,
        PitchLanguage::Portugues => &PORTUGUES,
        PitchLanguage::Vlaams => &VLAAMS,
        PitchLanguage::Francais => &FRANCAIS,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert an alteration value (in semitones, as `Ratio<i32>`) to the
/// 0–8 alteration table index. Returns `None` for unsupported alterations.
fn alter_to_index(alter: Alter) -> Option<usize> {
    // index = alter_semitones * 2 + 4
    // alter is Ratio<i32> in semitones. Multiply by 2 (= Ratio(2,1)) and add 4.
    let idx = alter * Ratio::new(2, 1) + Ratio::new(4, 1);
    if idx.is_integer() {
        let i = *idx.numer() / *idx.denom();
        if (0..9).contains(&i) {
            return Some(i as usize);
        }
    }
    None
}

/// Convert an alteration table index (0–8) back to an `Alter` value.
fn index_to_alter(index: usize) -> Alter {
    // alter = (index - 4) / 2
    Ratio::new(index as i32 - 4, 2)
}

/// The LilyPond note name for a `(step, alter)` pair in the given language —
/// the spelling LilyPond itself uses (Deutsch E-flat is `es`, B-flat `b`).
///
/// Returns `None` if the alteration is not representable in that language
/// (i.e. the suffix is an empty string for a non-zero alteration).
pub fn pitch_name(step: PitchStep, alter: Alter, lang: PitchLanguage) -> Option<String> {
    let data = lang_data(lang);
    let idx = alter_to_index(alter)?;
    let suffix = data.alterations[idx];
    // Natural (index 4) always has empty suffix → always OK.
    // Non-natural with empty suffix means unsupported alteration.
    if idx != 4 && suffix.is_empty() {
        return None;
    }
    let built = format!("{}{}", data.note_names[step as usize], suffix);
    let official = data
        .replacements
        .iter()
        .find(|(from, _)| *from == built)
        .map(|(_, to)| to.to_string());
    Some(official.unwrap_or(built))
}

/// Parse a LilyPond note name into `(PitchStep, Alter)`, accepting every
/// spelling the language allows.
///
/// Returns `None` if the string doesn't match any known pitch.
pub fn parse_pitch_name(name: &str, lang: PitchLanguage) -> Option<(PitchStep, Alter)> {
    let data = lang_data(lang);
    let name = name.to_ascii_lowercase();
    // A full-name spelling stands for the built name it replaces.
    let built = data
        .replacements
        .iter()
        .map(|(from, to)| (*to, *from))
        .chain(data.input_aliases.iter().copied())
        .find(|(spelling, _)| *spelling == name)
        .map_or(name.as_str(), |(_, from)| from);

    // Otherwise: note name + alteration suffix, input-only spellings included.
    let bases = data.note_names.iter().copied().enumerate();
    for (step_idx, base) in bases.chain(data.input_names.iter().copied()) {
        let Some(suffix) = built.strip_prefix(base) else {
            continue;
        };
        let alterations = data.alterations.iter().copied().enumerate();
        // An empty suffix off the natural slot means "not supported".
        let found = alterations
            .filter(|&(i, sfx)| i == 4 || !sfx.is_empty())
            .chain(data.input_alterations.iter().copied())
            .find(|&(_, sfx)| sfx == suffix);
        if let Some((alt_idx, _)) = found {
            return Some((
                PitchStep::from_index(step_idx as i32),
                index_to_alter(alt_idx),
            ));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pitch_name_nederlands() {
        let c = PitchStep::C;
        let nat = Alter::from_integer(0);
        let sharp = Alter::from_integer(1);
        let flat = Alter::from_integer(-1);

        assert_eq!(
            pitch_name(c, nat, PitchLanguage::Nederlands),
            Some("c".into())
        );
        assert_eq!(
            pitch_name(c, sharp, PitchLanguage::Nederlands),
            Some("cis".into())
        );
        assert_eq!(
            pitch_name(c, flat, PitchLanguage::Nederlands),
            Some("ces".into())
        );
    }

    #[test]
    fn test_pitch_name_english() {
        let a = PitchStep::A;
        let sharp = Alter::from_integer(1);
        let flat = Alter::from_integer(-1);
        let dsharp = Alter::from_integer(2);

        assert_eq!(
            pitch_name(a, sharp, PitchLanguage::English),
            Some("as".into())
        );
        assert_eq!(
            pitch_name(a, flat, PitchLanguage::English),
            Some("af".into())
        );
        assert_eq!(
            pitch_name(a, dsharp, PitchLanguage::English),
            Some("ass".into())
        );
    }

    #[test]
    fn test_pitch_name_italiano() {
        let c = PitchStep::C;
        let g = PitchStep::G;
        let nat = Alter::from_integer(0);
        let sharp = Alter::from_integer(1);

        assert_eq!(
            pitch_name(c, nat, PitchLanguage::Italiano),
            Some("do".into())
        );
        assert_eq!(
            pitch_name(g, sharp, PitchLanguage::Italiano),
            Some("sold".into())
        );
    }

    #[test]
    fn test_pitch_name_deutsch_b_note() {
        // In Deutsch, B natural is written "h", B flat is "b".
        let b = PitchStep::B;
        let nat = Alter::from_integer(0);
        let flat = Alter::from_integer(-1);
        assert_eq!(pitch_name(b, nat, PitchLanguage::Deutsch), Some("h".into()));
        assert_eq!(
            pitch_name(b, flat, PitchLanguage::Deutsch),
            Some("b".into())
        );
        assert_eq!(
            parse_pitch_name("hes", PitchLanguage::Deutsch),
            Some((b, flat))
        );
    }

    /// Written names are the ones LilyPond accepts (scm/define-note-names.scm):
    /// `ees`/`aes` are Dutch only, Norwegian B flats are `b`-based, Catalan has
    /// its own quarter-tone suffixes.
    #[test]
    fn test_written_names_are_lilyponds() {
        use PitchLanguage::*;
        let name = |step, alter: (i32, i32), lang| {
            pitch_name(step, Ratio::new(alter.0, alter.1), lang).unwrap()
        };
        let (e, a, b, c) = (PitchStep::E, PitchStep::A, PitchStep::B, PitchStep::C);
        for lang in [Deutsch, Suomi] {
            assert_eq!(name(e, (-1, 1), lang), "es");
            assert_eq!(name(e, (-2, 1), lang), "eses");
            assert_eq!(name(a, (-1, 1), lang), "as");
            assert_eq!(name(a, (-2, 1), lang), "asas");
        }
        assert_eq!(name(e, (-1, 1), Svenska), "ess");
        assert_eq!(name(a, (-2, 1), Svenska), "assess");
        assert_eq!(name(b, (-1, 1), Svenska), "b");
        assert_eq!(name(b, (-1, 1), Norsk), "b");
        assert_eq!(name(b, (-2, 1), Norsk), "bess");
        assert_eq!(name(b, (-3, 2), Norsk), "beh");
        assert_eq!(name(c, (1, 1), Norsk), "ciss");
        assert_eq!(name(c, (-1, 2), Catalan), "doqb");
        assert_eq!(name(c, (3, 2), Catalan), "dotqd");
        // Dutch output is unchanged; the short forms are read.
        assert_eq!(name(e, (-1, 1), Nederlands), "ees");
        assert_eq!(
            parse_pitch_name("es", Nederlands),
            Some((e, Ratio::new(-1, 1)))
        );
    }

    #[test]
    fn test_alternative_spellings_are_read() {
        use PitchLanguage::*;
        let r = |n| Ratio::new(n, 1);
        assert_eq!(
            parse_pitch_name("ases", Deutsch),
            Some((PitchStep::A, r(-2)))
        );
        assert_eq!(parse_pitch_name("cis", Norsk), Some((PitchStep::C, r(1))));
        assert_eq!(parse_pitch_name("es", Norsk), Some((PitchStep::E, r(-1))));
        assert_eq!(parse_pitch_name("bes", Norsk), Some((PitchStep::B, r(-2))));
        assert_eq!(parse_pitch_name("dos", Catalan), Some((PitchStep::C, r(1))));
        assert_eq!(parse_pitch_name("cx", English), Some((PitchStep::C, r(2))));
    }

    #[test]
    fn test_pitch_name_quarter_tone() {
        let c = PitchStep::C;
        let qsharp = Ratio::new(1, 2); // quarter-sharp

        assert_eq!(
            pitch_name(c, qsharp, PitchLanguage::Nederlands),
            Some("cih".into())
        );
        assert_eq!(
            pitch_name(c, qsharp, PitchLanguage::English),
            Some("cqs".into())
        );
        assert_eq!(
            pitch_name(c, qsharp, PitchLanguage::Espanol),
            Some("docs".into())
        );
        // A third-tone has no name in any language.
        assert_eq!(
            pitch_name(c, Ratio::new(1, 3), PitchLanguage::Nederlands),
            None
        );
    }

    #[test]
    fn test_parse_pitch_name_nederlands() {
        let (step, alter) = parse_pitch_name("cis", PitchLanguage::Nederlands).unwrap();
        assert_eq!(step, PitchStep::C);
        assert_eq!(alter, Alter::from_integer(1));

        let (step, alter) = parse_pitch_name("bes", PitchLanguage::Nederlands).unwrap();
        assert_eq!(step, PitchStep::B);
        assert_eq!(alter, Alter::from_integer(-1));
    }

    #[test]
    fn test_parse_pitch_name_english() {
        let (step, alter) = parse_pitch_name("cs", PitchLanguage::English).unwrap();
        assert_eq!(step, PitchStep::C);
        assert_eq!(alter, Alter::from_integer(1));

        let (step, alter) = parse_pitch_name("bf", PitchLanguage::English).unwrap();
        assert_eq!(step, PitchStep::B);
        assert_eq!(alter, Alter::from_integer(-1));
    }

    #[test]
    fn test_parse_pitch_name_italiano() {
        let (step, alter) = parse_pitch_name("do", PitchLanguage::Italiano).unwrap();
        assert_eq!(step, PitchStep::C);
        assert_eq!(alter, Alter::from_integer(0));

        let (step, alter) = parse_pitch_name("sold", PitchLanguage::Italiano).unwrap();
        assert_eq!(step, PitchStep::G);
        assert_eq!(alter, Alter::from_integer(1));
    }

    #[test]
    fn test_parse_short_forms() {
        // "es" is the short form of "ees" (E flat in Nederlands).
        let result = parse_pitch_name("es", PitchLanguage::Nederlands);
        assert!(result.is_some());
        let (step, alter) = result.unwrap();
        assert_eq!(step, PitchStep::E);
        assert_eq!(alter, Alter::from_integer(-1));

        // "as" is the short form of "aes" (A flat in Nederlands).
        let (step, alter) = parse_pitch_name("as", PitchLanguage::Nederlands).unwrap();
        assert_eq!(step, PitchStep::A);
        assert_eq!(alter, Alter::from_integer(-1));
    }

    #[test]
    fn test_language_aliases() {
        // Norsk and Suomi use the same data as Deutsch.
        assert_eq!(
            pitch_name(PitchStep::B, Alter::from_integer(0), PitchLanguage::Norsk),
            pitch_name(PitchStep::B, Alter::from_integer(0), PitchLanguage::Deutsch),
        );
        assert_eq!(
            pitch_name(PitchStep::B, Alter::from_integer(0), PitchLanguage::Suomi),
            pitch_name(PitchStep::B, Alter::from_integer(0), PitchLanguage::Deutsch),
        );
        // Catalan uses same data as Italiano.
        assert_eq!(
            pitch_name(PitchStep::C, Alter::from_integer(1), PitchLanguage::Catalan),
            pitch_name(
                PitchStep::C,
                Alter::from_integer(1),
                PitchLanguage::Italiano
            ),
        );
    }

    #[test]
    fn test_francais() {
        // Suffixes and names per LilyPond's scm/define-note-names.scm (`français`).
        let lang = PitchLanguage::from_str_loose("français").unwrap();
        assert_eq!(lang, PitchLanguage::Francais);
        assert_eq!(lang.as_str(), "français");
        let name = |step, alter: (i32, i32)| pitch_name(step, Ratio::new(alter.0, alter.1), lang);
        assert_eq!(name(PitchStep::D, (0, 1)).as_deref(), Some("ré"));
        assert_eq!(name(PitchStep::D, (-1, 1)).as_deref(), Some("réb"));
        assert_eq!(name(PitchStep::F, (1, 1)).as_deref(), Some("fad"));
        assert_eq!(name(PitchStep::B, (-2, 1)).as_deref(), Some("sibb"));
        assert_eq!(name(PitchStep::G, (2, 1)).as_deref(), Some("soldd"));
        assert_eq!(name(PitchStep::E, (-3, 2)).as_deref(), Some("mibsb"));
        assert_eq!(name(PitchStep::C, (1, 2)).as_deref(), Some("dosd"));

        let parse = |s| parse_pitch_name(s, lang);
        let r = |n, d| Ratio::new(n, d);
        assert_eq!(parse("ré"), Some((PitchStep::D, r(0, 1))));
        assert_eq!(parse("rédsd"), Some((PitchStep::D, r(3, 2))));
        // ASCII `re` spellings and `x` double-sharps are accepted on input.
        assert_eq!(parse("re"), Some((PitchStep::D, r(0, 1))));
        assert_eq!(parse("reb"), Some((PitchStep::D, r(-1, 1))));
        assert_eq!(parse("solx"), Some((PitchStep::G, r(2, 1))));
        assert_eq!(parse("rex"), Some((PitchStep::D, r(2, 1))));
        assert_eq!(parse("réx"), Some((PitchStep::D, r(2, 1))));
        assert_eq!(parse("sold"), Some((PitchStep::G, r(1, 1))));
        assert_eq!(parse("sol"), Some((PitchStep::G, r(0, 1))));
        assert_eq!(parse("cis"), None);
    }

    #[test]
    fn test_roundtrip_all_languages() {
        // For every language, for every step, for standard alterations
        // (double-flat through double-sharp), verify pitch_name → parse_pitch_name roundtrip.
        let standard_alters = [
            Ratio::new(-2, 1),
            Ratio::new(-1, 1),
            Ratio::new(0, 1),
            Ratio::new(1, 1),
            Ratio::new(2, 1),
        ];
        let steps = [
            PitchStep::C,
            PitchStep::D,
            PitchStep::E,
            PitchStep::F,
            PitchStep::G,
            PitchStep::A,
            PitchStep::B,
        ];

        for lang in PitchLanguage::ALL {
            for &step in &steps {
                for &alter in &standard_alters {
                    if let Some(name) = pitch_name(step, alter, lang) {
                        let parsed = parse_pitch_name(&name, lang);
                        assert!(
                            parsed.is_some(),
                            "Failed to parse '{}' back in {:?}",
                            name,
                            lang
                        );
                        let (ps, pa) = parsed.unwrap();
                        assert_eq!(
                            (ps, pa),
                            (step, alter),
                            "Roundtrip mismatch for '{}' in {:?}: got ({:?}, {})",
                            name,
                            lang,
                            ps,
                            pa
                        );
                    }
                }
            }
        }
    }
}
