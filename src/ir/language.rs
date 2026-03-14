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
//! Ported from python-ly (`python-ly/ly/pitch/__init__.py`, `pitchInfo` dict).
//! See <https://github.com/frescobaldi/python-ly>.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PitchLanguage {
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
        }
    }

    /// Parse from the LilyPond `\language` string.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "nederlands" => Some(Self::Nederlands),
            "english" => Some(Self::English),
            "deutsch" => Some(Self::Deutsch),
            "svenska" => Some(Self::Svenska),
            "italiano" => Some(Self::Italiano),
            "espanol" | "español" => Some(Self::Espanol),
            "portugues" | "português" => Some(Self::Portugues),
            "vlaams" => Some(Self::Vlaams),
            "norsk" => Some(Self::Norsk),
            "suomi" => Some(Self::Suomi),
            "catalan" | "català" => Some(Self::Catalan),
            _ => None,
        }
    }

    /// All supported languages.
    pub const ALL: [PitchLanguage; 11] = [
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
    ];
}

impl fmt::Display for PitchLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Default for PitchLanguage {
    fn default() -> Self {
        Self::Nederlands
    }
}

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
/// - `replacements`: `(canonical, alternative)` pairs for note names that
///   have a shortened or alternate form (e.g. `"ees"` → `"es"` in Nederlands).
struct LangData {
    note_names: [&'static str; 7],
    alterations: [&'static str; 9],
    replacements: &'static [(&'static str, &'static str)],
}

// Data ported from python-ly `pitchInfo` dict.
// Ordering: C D E F G A B
// Alteration indices: 0=double-flat  1=¾flat  2=flat  3=¼flat  4=natural
//                     5=¼sharp  6=sharp  7=¾sharp  8=double-sharp

const NEDERLANDS: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "b"],
    alterations: ["eses", "eseh", "es", "eh", "", "ih", "is", "isih", "isis"],
    replacements: &[("ees", "es"), ("aes", "as")],
};

const ENGLISH: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "b"],
    alterations: ["ff", "tqf", "f", "qf", "", "qs", "s", "tqs", "ss"],
    replacements: &[],
};

const DEUTSCH: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "h"],
    alterations: ["eses", "eseh", "es", "eh", "", "ih", "is", "isih", "isis"],
    replacements: &[
        ("ases", "asas"),
        ("ees", "es"),
        ("aes", "as"),
        ("heses", "heses"),
        ("hes", "b"),
    ],
};

const SVENSKA: LangData = LangData {
    note_names: ["c", "d", "e", "f", "g", "a", "h"],
    alterations: ["essess", "", "ess", "", "", "", "iss", "", "ississ"],
    replacements: &[
        ("ees", "es"),
        ("aes", "as"),
        ("hessess", "hessess"),
        ("hess", "b"),
    ],
};

const ITALIANO: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "bsb", "b", "sb", "", "sd", "d", "dsd", "dd"],
    replacements: &[],
};

const ESPANOL: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "", "b", "", "", "", "s", "", "ss"],
    replacements: &[],
};

const PORTUGUES: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "btqt", "b", "bqt", "", "sqt", "s", "stqt", "ss"],
    replacements: &[],
};

const VLAAMS: LangData = LangData {
    note_names: ["do", "re", "mi", "fa", "sol", "la", "si"],
    alterations: ["bb", "", "b", "", "", "", "k", "", "kk"],
    replacements: &[],
};

fn lang_data(lang: PitchLanguage) -> &'static LangData {
    match lang {
        PitchLanguage::Nederlands => &NEDERLANDS,
        PitchLanguage::English => &ENGLISH,
        PitchLanguage::Deutsch | PitchLanguage::Norsk | PitchLanguage::Suomi => &DEUTSCH,
        PitchLanguage::Svenska => &SVENSKA,
        PitchLanguage::Italiano | PitchLanguage::Catalan => &ITALIANO,
        PitchLanguage::Espanol => &ESPANOL,
        PitchLanguage::Portugues => &PORTUGUES,
        PitchLanguage::Vlaams => &VLAAMS,
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

/// Get the LilyPond note name for a `(step, alter)` pair in the given language.
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
    let base = data.note_names[step as usize];
    Some(format!("{}{}", base, suffix))
}

/// Get the shortest (alternative) LilyPond note name if one exists,
/// otherwise the canonical form.
///
/// For example, in Nederlands: `e flat` → canonical `"ees"` but alternative `"es"`.
pub fn pitch_name_short(step: PitchStep, alter: Alter, lang: PitchLanguage) -> Option<String> {
    let canonical = pitch_name(step, alter, lang)?;
    let data = lang_data(lang);
    for &(long, short) in data.replacements {
        if canonical == long {
            return Some(short.to_string());
        }
    }
    Some(canonical)
}

/// Parse a LilyPond note name into `(PitchStep, Alter)`.
///
/// Tries all note names and alteration suffixes for the given language.
/// Returns `None` if the string doesn't match any known pitch.
pub fn parse_pitch_name(name: &str, lang: PitchLanguage) -> Option<(PitchStep, Alter)> {
    let data = lang_data(lang);
    let name_lower = name.to_ascii_lowercase();

    // First try replacement (short) forms.
    for &(canonical, alt) in data.replacements {
        for (step_idx, &base) in data.note_names.iter().enumerate() {
            // Build the full short note name and check.
            // Replacements in python-ly are full note names, not just suffixes.
            if name_lower == alt {
                // Find which step+alter produces the canonical form.
                let step = PitchStep::from_index(step_idx as i32);
                for (alt_idx, &suffix) in data.alterations.iter().enumerate() {
                    let candidate = format!("{}{}", base, suffix);
                    if candidate == canonical {
                        return Some((step, index_to_alter(alt_idx)));
                    }
                }
            }
        }
    }

    // Try canonical forms: note_name + alteration_suffix.
    // Try longest match first to avoid ambiguity (e.g. "cisis" vs "cis").
    for (step_idx, &base) in data.note_names.iter().enumerate() {
        if !name_lower.starts_with(base) {
            continue;
        }
        let suffix = &name_lower[base.len()..];
        let step = PitchStep::from_index(step_idx as i32);

        for (alt_idx, &alt_suffix) in data.alterations.iter().enumerate().rev() {
            // Skip empty suffixes for non-natural (they mean "not supported").
            if alt_idx != 4 && alt_suffix.is_empty() {
                continue;
            }
            if suffix == alt_suffix {
                return Some((step, index_to_alter(alt_idx)));
            }
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

        assert_eq!(pitch_name(c, nat, PitchLanguage::Nederlands), Some("c".into()));
        assert_eq!(pitch_name(c, sharp, PitchLanguage::Nederlands), Some("cis".into()));
        assert_eq!(pitch_name(c, flat, PitchLanguage::Nederlands), Some("ces".into()));
    }

    #[test]
    fn test_pitch_name_english() {
        let a = PitchStep::A;
        let sharp = Alter::from_integer(1);
        let flat = Alter::from_integer(-1);
        let dsharp = Alter::from_integer(2);

        assert_eq!(pitch_name(a, sharp, PitchLanguage::English), Some("as".into()));
        assert_eq!(pitch_name(a, flat, PitchLanguage::English), Some("af".into()));
        assert_eq!(pitch_name(a, dsharp, PitchLanguage::English), Some("ass".into()));
    }

    #[test]
    fn test_pitch_name_italiano() {
        let c = PitchStep::C;
        let g = PitchStep::G;
        let nat = Alter::from_integer(0);
        let sharp = Alter::from_integer(1);

        assert_eq!(pitch_name(c, nat, PitchLanguage::Italiano), Some("do".into()));
        assert_eq!(pitch_name(g, sharp, PitchLanguage::Italiano), Some("sold".into()));
    }

    #[test]
    fn test_pitch_name_deutsch_b_note() {
        // In Deutsch, B natural is written "h", B flat is "b".
        let b = PitchStep::B;
        let nat = Alter::from_integer(0);
        let flat = Alter::from_integer(-1);

        assert_eq!(pitch_name(b, nat, PitchLanguage::Deutsch), Some("h".into()));
        assert_eq!(pitch_name(b, flat, PitchLanguage::Deutsch), Some("hes".into()));
        // The short form of "hes" is "b" in Deutsch:
        assert_eq!(pitch_name_short(b, flat, PitchLanguage::Deutsch), Some("b".into()));
    }

    #[test]
    fn test_pitch_name_short_nederlands() {
        let e = PitchStep::E;
        let a = PitchStep::A;
        let flat = Alter::from_integer(-1);

        // "ees" → short "es", "aes" → short "as"
        assert_eq!(pitch_name_short(e, flat, PitchLanguage::Nederlands), Some("es".into()));
        assert_eq!(pitch_name_short(a, flat, PitchLanguage::Nederlands), Some("as".into()));
    }

    #[test]
    fn test_pitch_name_quarter_tone() {
        let c = PitchStep::C;
        let qsharp = Ratio::new(1, 2); // quarter-sharp

        assert_eq!(pitch_name(c, qsharp, PitchLanguage::Nederlands), Some("cih".into()));
        assert_eq!(pitch_name(c, qsharp, PitchLanguage::English), Some("cqs".into()));
        // Espanol doesn't support quarter-tones:
        assert_eq!(pitch_name(c, qsharp, PitchLanguage::Espanol), None);
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
            pitch_name(PitchStep::C, Alter::from_integer(1), PitchLanguage::Italiano),
        );
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
            PitchStep::C, PitchStep::D, PitchStep::E, PitchStep::F,
            PitchStep::G, PitchStep::A, PitchStep::B,
        ];

        for lang in PitchLanguage::ALL {
            for &step in &steps {
                for &alter in &standard_alters {
                    if let Some(name) = pitch_name(step, alter, lang) {
                        let parsed = parse_pitch_name(&name, lang);
                        assert!(
                            parsed.is_some(),
                            "Failed to parse '{}' back in {:?}",
                            name, lang
                        );
                        let (ps, pa) = parsed.unwrap();
                        assert_eq!(
                            (ps, pa),
                            (step, alter),
                            "Roundtrip mismatch for '{}' in {:?}: got ({:?}, {})",
                            name, lang, ps, pa
                        );
                    }
                }
            }
        }
    }
}
