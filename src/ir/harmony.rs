//! Harmony (chord symbol) and figured bass IR types.
//!
//! These represent chord symbols (`Cmaj7`, `Dm/F`) and figured bass notation
//! at the measure level, parallel to notes.

use super::serde_defaults::{is_default, reduced};
use serde::{Deserialize, Serialize};

use super::duration::{Duration, Frac};
use super::pitch::PitchStep;

named_enum! {
    /// A chord symbol's quality (MusicXML's `<kind>`).
    pub enum ChordKind {
        Major => "major",
        Minor => "minor",
        Augmented => "augmented",
        Diminished => "diminished",
        Dominant => "dominant",
        MajorSeventh => "major-seventh",
        MinorSeventh => "minor-seventh",
        DiminishedSeventh => "diminished-seventh",
        AugmentedSeventh => "augmented-seventh",
        HalfDiminished => "half-diminished",
        MajorMinor => "major-minor",
        MajorSixth => "major-sixth",
        MinorSixth => "minor-sixth",
        DominantNinth => "dominant-ninth",
        MajorNinth => "major-ninth",
        MinorNinth => "minor-ninth",
        Dominant11th => "dominant-11th",
        Major11th => "major-11th",
        Minor11th => "minor-11th",
        Dominant13th => "dominant-13th",
        Major13th => "major-13th",
        Minor13th => "minor-13th",
        SuspendedSecond => "suspended-second",
        SuspendedFourth => "suspended-fourth",
        Neapolitan => "Neapolitan",
        Italian => "Italian",
        French => "French",
        German => "German",
        Pedal => "pedal",
        Power => "power",
        Tristan => "Tristan",
        Other => "other",
        /// No chord (N.C.).
        NoChord => "none",
    }
}

named_enum! {
    /// What a chord degree does to its chord (MusicXML's `<degree-type>`).
    pub enum DegreeType {
        Add => "add",
        Alter => "alter",
        Subtract => "subtract",
    }
}

open_named_enum! {
    /// A figure's accidental or stroke (MusicXML's `<prefix>`, `<suffix>`).
    pub enum FigureAccidental {
        Sharp => "sharp",
        Flat => "flat",
        Natural => "natural",
        DoubleSharp => "double-sharp",
        SharpSharp => "sharp-sharp",
        FlatFlat => "flat-flat",
        Slash => "slash",
        BackSlash => "back-slash",
        Vertical => "vertical",
    }
}

// ---------------------------------------------------------------------------
// Chord qualities as steps
// ---------------------------------------------------------------------------

/// A chord as its steps above the root (3, 5, 7, 9, 11, 13; 2, 4, 6 too),
/// each with its alteration in semitones from the step's usual interval:
/// major (2, 3, 6, 9, 13), perfect (4, 5, 11) and, for the seventh, minor —
/// a dominant seventh's, as LilyPond and lead sheets have it (a seventh
/// altered by 1 is a major seventh). The root is implied.
pub type ChordSteps = std::collections::BTreeMap<u8, i8>;

/// MusicXML chord kinds as steps, simplest first (a tie picks the first).
const KIND_STEPS: &[(ChordKind, &[(u8, i8)])] = &[
    (ChordKind::Major, &[(3, 0), (5, 0)]),
    (ChordKind::Minor, &[(3, -1), (5, 0)]),
    (ChordKind::Augmented, &[(3, 0), (5, 1)]),
    (ChordKind::Diminished, &[(3, -1), (5, -1)]),
    (ChordKind::SuspendedFourth, &[(4, 0), (5, 0)]),
    (ChordKind::SuspendedSecond, &[(2, 0), (5, 0)]),
    (ChordKind::Power, &[(5, 0)]),
    (ChordKind::Dominant, &[(3, 0), (5, 0), (7, 0)]),
    (ChordKind::MajorSeventh, &[(3, 0), (5, 0), (7, 1)]),
    (ChordKind::MinorSeventh, &[(3, -1), (5, 0), (7, 0)]),
    (ChordKind::DiminishedSeventh, &[(3, -1), (5, -1), (7, -1)]),
    (ChordKind::HalfDiminished, &[(3, -1), (5, -1), (7, 0)]),
    (ChordKind::AugmentedSeventh, &[(3, 0), (5, 1), (7, 0)]),
    (ChordKind::MajorMinor, &[(3, -1), (5, 0), (7, 1)]),
    (ChordKind::MajorSixth, &[(3, 0), (5, 0), (6, 0)]),
    (ChordKind::MinorSixth, &[(3, -1), (5, 0), (6, 0)]),
    (ChordKind::DominantNinth, &[(3, 0), (5, 0), (7, 0), (9, 0)]),
    (ChordKind::MajorNinth, &[(3, 0), (5, 0), (7, 1), (9, 0)]),
    (ChordKind::MinorNinth, &[(3, -1), (5, 0), (7, 0), (9, 0)]),
    (
        ChordKind::Dominant11th,
        &[(3, 0), (5, 0), (7, 0), (9, 0), (11, 0)],
    ),
    (
        ChordKind::Major11th,
        &[(3, 0), (5, 0), (7, 1), (9, 0), (11, 0)],
    ),
    (
        ChordKind::Minor11th,
        &[(3, -1), (5, 0), (7, 0), (9, 0), (11, 0)],
    ),
    // A 13th chord leaves out the 11th (it clashes with the 3rd), as
    // LilyPond and players do; a minor one keeps it.
    (
        ChordKind::Dominant13th,
        &[(3, 0), (5, 0), (7, 0), (9, 0), (13, 0)],
    ),
    (
        ChordKind::Major13th,
        &[(3, 0), (5, 0), (7, 1), (9, 0), (13, 0)],
    ),
    (
        ChordKind::Minor13th,
        &[(3, -1), (5, 0), (7, 0), (9, 0), (11, 0), (13, 0)],
    ),
];

/// The steps of a MusicXML chord kind, `None` for a kind without them
/// (`none`, `other`, `pedal`, the augmented sixths …).
pub fn kind_steps(kind: ChordKind) -> Option<ChordSteps> {
    KIND_STEPS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, steps)| steps.iter().copied().collect())
}

/// The steps of a kind with its degree changes applied.
pub fn chord_steps(kind: ChordKind, degrees: &[ChordDegree]) -> Option<ChordSteps> {
    let mut steps = kind_steps(kind)?;
    for d in degrees {
        let alter = d.alter.round() as i8;
        match d.degree_type {
            DegreeType::Subtract => {
                steps.remove(&d.value);
            }
            DegreeType::Alter => {
                let base = steps.get(&d.value).copied().unwrap_or(0);
                steps.insert(d.value, base + alter);
            }
            DegreeType::Add => {
                steps.insert(d.value, alter);
            }
        }
    }
    Some(steps)
}

/// The MusicXML kind nearest to some steps, and the degrees that make the
/// difference (added, altered or removed), lowest step first.
pub fn kind_and_degrees(steps: &ChordSteps) -> (ChordKind, Vec<ChordDegree>) {
    let diff = |template: &ChordSteps| -> Vec<ChordDegree> {
        let mut out = Vec::new();
        for (&step, &alter) in steps {
            match template.get(&step) {
                None => out.push(degree(step, alter, DegreeType::Add)),
                Some(&t) if t != alter => out.push(degree(step, alter - t, DegreeType::Alter)),
                Some(_) => {}
            }
        }
        for &step in template.keys() {
            if !steps.contains_key(&step) {
                out.push(degree(step, 0, DegreeType::Subtract));
            }
        }
        out.sort_by_key(|d| d.value);
        out
    };
    // Fewest changes; on a tie, the kind that reaches the chord's top
    // unaltered step (`13.11` is a 13th chord with an 11th added; `7b9` a
    // seventh chord with a flat ninth added, not an altered ninth chord).
    let top = steps
        .iter()
        .filter(|(&s, &a)| a == 0 || s <= 7)
        .map(|(&s, _)| s)
        .max();
    KIND_STEPS
        .iter()
        .map(|(kind, t)| {
            let reaches = t.iter().map(|s| s.0).max() == top;
            (*kind, diff(&t.iter().copied().collect()), !reaches)
        })
        // A step removed reads worse than one added (`add9`, not a ninth
        // chord without its seventh).
        .min_by_key(|(_, d, misses)| {
            let removed = d
                .iter()
                .filter(|d| d.degree_type == DegreeType::Subtract)
                .count();
            (d.len(), removed, *misses)
        })
        .map(|(kind, d, _)| (kind, d))
        .unwrap_or((ChordKind::Major, Vec::new()))
}

fn degree(value: u8, alter: i8, degree_type: DegreeType) -> ChordDegree {
    ChordDegree {
        value,
        alter: f64::from(alter),
        degree_type,
    }
}

/// Stack thirds up to `extent` as LilyPond does (`c:9` is 3, 5, 7, 9; an
/// even extent adds itself to the thirds below it: `c:6` is 3, 5, 6).
fn stack_to(steps: &mut ChordSteps, extent: u8) {
    let extent = extent.min(13);
    for s in [3, 5, 7, 9, 11, 13] {
        if s <= extent {
            steps.entry(s).or_insert(0);
        }
    }
    if extent % 2 == 0 {
        steps.insert(extent, 0);
    }
}

/// Split a leading number off `s`: `("13", "b9")` → `(13, "b9")`.
fn leading_number(s: &str) -> (Option<u8>, &str) {
    let n = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (s[..n].parse().ok(), &s[n..])
}

/// LilyPond chord modifiers (the text after `:`): `m7.5-`, `7.9-`, `maj9`,
/// `13.11`, `9^7`, `sus4.7`, `7sus4`, `m7+`, `5`. `None` when they aren't
/// LilyPond modifiers.
pub fn ly_chord_steps(modifiers: &str) -> Option<ChordSteps> {
    let (adds, removals) = modifiers.split_once('^').unwrap_or((modifiers, ""));
    let items: Vec<&str> = adds.split('.').collect();
    let mut steps: ChordSteps = [(3, 0), (5, 0)].into_iter().collect();
    let (mut minor, mut dim, mut aug, mut maj, mut sus) = (false, false, false, false, None);
    let mut first = items.first().copied().unwrap_or("");
    let mut quality = |rest: &mut &str| loop {
        let words: [(&str, u8); 8] = [
            ("maj", 1),
            ("min", 2),
            ("m", 2),
            ("dim", 3),
            ("aug", 4),
            ("sus2", 5),
            ("sus4", 6),
            ("sus", 6),
        ];
        let Some((w, k)) = words.iter().find(|(w, _)| rest.starts_with(w)) else {
            break;
        };
        *rest = &rest[w.len()..];
        match k {
            1 => maj = true,
            2 => minor = true,
            3 => dim = true,
            4 => aug = true,
            5 => sus = Some(2),
            _ => sus = Some(4),
        }
    };
    quality(&mut first);
    let (extent, mut rest) = leading_number(first);
    let extent_alter = if let Some(r) = rest.strip_prefix('+') {
        rest = r;
        1
    } else if let Some(r) = rest.strip_prefix('-') {
        rest = r;
        -1
    } else {
        0
    };
    quality(&mut rest);
    if !rest.is_empty() {
        return None;
    }
    let power = extent == Some(5) && items.len() == 1 && removals.is_empty();
    if let Some(n) = extent {
        stack_to(&mut steps, n);
        if n >= 13 && !minor {
            steps.remove(&11);
        }
        if extent_alter != 0 {
            steps.insert(n.min(13), extent_alter);
        }
    }
    if power {
        steps.remove(&3);
    }
    if minor {
        steps.insert(3, -1);
    }
    if dim {
        steps.insert(3, -1);
        steps.insert(5, -1);
        if let Some(s) = steps.get_mut(&7) {
            *s = -1;
        }
    }
    if aug {
        steps.insert(5, 1);
    }
    if maj {
        steps.insert(7, 1);
    }
    if let Some(n) = sus {
        steps.remove(&3);
        steps.insert(n, 0);
    }
    for item in &items[1..] {
        let (n, sign) = leading_number(item);
        let alter = match sign {
            "+" => 1,
            "-" => -1,
            "" => 0,
            _ => return None,
        };
        steps.insert(n?, alter);
    }
    for r in removals.split('.').filter(|r| !r.is_empty()) {
        steps.remove(&r.parse::<u8>().ok()?);
    }
    Some(steps)
}

/// A lead-sheet chord suffix: `m7b5`, `7b9`, `7(#11)`, `add9`, `6/9`,
/// `maj7`, `Δ`, `ø`, `°7`, `m(maj7)`, `7sus4`, `13`, `no3`. `None` when the
/// text is no chord suffix (`ine` of `"Fine"`).
pub fn lead_sheet_chord_steps(suffix: &str) -> Option<ChordSteps> {
    let mut rest = suffix.trim();
    let mut steps: ChordSteps = [(3, 0), (5, 0)].into_iter().collect();
    let mut minor = false;
    let take = |rest: &mut &str, words: &[&str]| -> bool {
        match words.iter().find(|w| rest.starts_with(**w)) {
            Some(w) => {
                *rest = &rest[w.len()..];
                true
            }
            None => false,
        }
    };
    let maj_words: &[&str] = &["maj", "Maj", "MAJ", "M", "Δ", "^"];
    let mut maj = take(&mut rest, maj_words);
    if !maj {
        minor = take(&mut rest, &["min", "mi", "m", "-"]);
        // `m(maj7)`, `mM7`, `m/maj7`
        let saved = rest;
        let open = take(&mut rest, &["(", "/"]);
        if take(&mut rest, maj_words) {
            maj = true;
        } else if open {
            rest = saved;
        }
    }
    let dim = take(&mut rest, &["dim", "°", "o"]);
    let half_dim = !dim && take(&mut rest, &["ø"]);
    let aug = take(&mut rest, &["aug", "+"]);
    let (extent, after) = leading_number(rest);
    rest = after;
    match extent {
        None => {}
        Some(69) => {
            steps.insert(6, 0);
            steps.insert(9, 0);
        }
        Some(5) => {
            steps.remove(&3);
        }
        Some(2) => {
            steps.insert(2, 0);
        }
        Some(n @ (6 | 7 | 9 | 11 | 13)) => {
            stack_to(&mut steps, n);
            if n == 13 && !minor {
                steps.remove(&11);
            }
        }
        Some(_) => return None,
    }
    if minor || dim || half_dim {
        steps.insert(3, -1);
    }
    if dim || half_dim {
        steps.insert(5, -1);
    }
    if dim {
        if let Some(s) = steps.get_mut(&7) {
            *s = -1;
        }
    }
    if half_dim {
        steps.insert(7, 0);
    }
    if aug {
        steps.insert(5, 1);
    }
    // `Δ`, `maj7`: the seventh is major; `maj` alone is a major triad.
    if maj && (steps.contains_key(&7) || suffix.trim().starts_with(['Δ', '^'])) {
        steps.insert(7, 1);
    }
    // What follows: `sus4`, `add9`, `no3`, `b9`, `#11`, `(13)`, `/9` …
    loop {
        rest = rest.trim_start_matches([' ', '(', ')', ',', '/', '.']);
        if rest.is_empty() {
            break;
        }
        if take(&mut rest, &["sus2"]) {
            steps.remove(&3);
            steps.insert(2, 0);
            continue;
        }
        if take(&mut rest, &["sus4", "sus"]) {
            steps.remove(&3);
            steps.insert(4, 0);
            continue;
        }
        if take(&mut rest, &["alt"]) {
            continue; // an altered dominant: its alterations aren't named
        }
        let remove = take(&mut rest, &["no", "omit"]);
        let _ = !remove && take(&mut rest, &["add"]);
        let alter = if take(&mut rest, &["b", "♭", "-"]) {
            -1
        } else if take(&mut rest, &["#", "♯", "+"]) {
            1
        } else {
            0
        };
        let (n, after) = leading_number(rest);
        let n = n.filter(|n| (2..=13).contains(n))?;
        rest = after;
        if remove {
            steps.remove(&n);
        } else {
            steps.insert(n, alter);
        }
    }
    Some(steps)
}

/// A lead-sheet suffix for a kind and its degrees: `m7`, `7b9`, `add9`,
/// `maj7#11`, `sus4` …
pub fn lead_sheet_suffix(kind: ChordKind, degrees: &[ChordDegree]) -> String {
    let mut out = suffix_of_kind(kind).to_string();
    for d in degrees {
        let sign = match d.alter.round() as i32 {
            a if a < 0 => "b",
            a if a > 0 => "#",
            _ => "",
        };
        match d.degree_type {
            DegreeType::Subtract => out.push_str(&format!("no{}", d.value)),
            DegreeType::Add if sign.is_empty() => out.push_str(&format!("add{}", d.value)),
            _ => out.push_str(&format!("{sign}{}", d.value)),
        }
    }
    out
}

/// LilyPond chord modifiers for a kind and its degrees, without the `:`
/// (`m7.5-`, `7.9-`, `3.5.9`, `sus4.7`); empty for a plain major triad.
pub fn ly_chord_modifiers(kind: ChordKind, degrees: &[ChordDegree]) -> String {
    use ChordKind as K;
    let base = match kind {
        K::Major => "",
        K::Minor => "m",
        K::Augmented => "aug",
        K::Diminished => "dim",
        K::SuspendedFourth => "sus4",
        K::SuspendedSecond => "sus2",
        K::Power => "5",
        K::Dominant => "7",
        K::MajorSeventh => "maj7",
        K::MinorSeventh => "m7",
        K::DiminishedSeventh => "dim7",
        K::HalfDiminished => "m7.5-",
        K::AugmentedSeventh => "aug7",
        K::MajorMinor => "m7+",
        K::MajorSixth => "6",
        K::MinorSixth => "m6",
        K::DominantNinth => "9",
        K::MajorNinth => "maj9",
        K::MinorNinth => "m9",
        K::Dominant11th => "11",
        K::Major11th => "maj11",
        K::Minor11th => "m11",
        K::Dominant13th => "13",
        K::Major13th => "maj13",
        K::Minor13th => "m13",
        _ => "",
    };
    let mut adds = Vec::new();
    let mut removals = Vec::new();
    for d in degrees {
        let sign = match d.alter.round() as i32 {
            a if a < 0 => "-",
            a if a > 0 => "+",
            _ => "",
        };
        match d.degree_type {
            DegreeType::Subtract => removals.push(d.value.to_string()),
            DegreeType::Add | DegreeType::Alter => adds.push(format!("{}{sign}", d.value)),
        }
    }
    // Added steps need the chord's extent before them: a triad's is 3.5.
    let base = match (base, adds.is_empty()) {
        ("", false) => "3.5",
        ("m", false) => "m3.5",
        ("aug", false) => "aug3.5",
        ("dim", false) => "dim3.5",
        (b, _) => b,
    };
    let mut out = base.to_string();
    for a in &adds {
        out.push('.');
        out.push_str(a);
    }
    if !removals.is_empty() {
        out.push('^');
        out.push_str(&removals.join("."));
    }
    out
}

/// The kind and degrees of a lead-sheet suffix (`None` when it is none):
/// [`lead_sheet_chord_steps`] read as the nearest MusicXML kind.
pub fn parse_chord_suffix(suffix: &str) -> Option<(ChordKind, Vec<ChordDegree>)> {
    lead_sheet_chord_steps(suffix).map(|s| kind_and_degrees(&s))
}

/// The lead-sheet suffix of a MusicXML chord kind (`minor-seventh` → `m7`).
pub fn suffix_of_kind(kind: ChordKind) -> &'static str {
    match kind {
        ChordKind::Minor => "m",
        ChordKind::Dominant => "7",
        ChordKind::MajorSeventh => "maj7",
        ChordKind::MinorSeventh => "m7",
        ChordKind::Diminished => "dim",
        ChordKind::DiminishedSeventh => "dim7",
        ChordKind::Augmented => "aug",
        ChordKind::AugmentedSeventh => "aug7",
        ChordKind::HalfDiminished => "m7b5",
        ChordKind::MajorMinor => "m(maj7)",
        ChordKind::MajorSixth => "6",
        ChordKind::MinorSixth => "m6",
        ChordKind::DominantNinth => "9",
        ChordKind::MajorNinth => "maj9",
        ChordKind::MinorNinth => "m9",
        ChordKind::Dominant11th => "11",
        ChordKind::Major11th => "maj11",
        ChordKind::Minor11th => "m11",
        ChordKind::Dominant13th => "13",
        ChordKind::Major13th => "maj13",
        ChordKind::Minor13th => "m13",
        ChordKind::SuspendedSecond => "sus2",
        ChordKind::SuspendedFourth => "sus4",
        ChordKind::Power => "5",
        _ => "",
    }
}

/// A pitch used in chord symbol descriptions (root or bass).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordPitch {
    pub step: PitchStep,
    /// Chromatic alteration in semitones (-2.0 to 2.0).
    #[serde(default, skip_serializing_if = "is_default")]
    pub alter: f64,
}

/// A chord degree modification (add, subtract, or alter a scale degree).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordDegree {
    /// Scale degree number (1–13).
    pub value: u8,
    /// Alteration in semitones (-2.0 to 2.0).
    #[serde(default, skip_serializing_if = "is_default")]
    pub alter: f64,
    pub degree_type: DegreeType,
}

/// A harmony / chord symbol.
///
/// Corresponds to MusicXML `<harmony>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Harmony {
    /// Root pitch of the chord.
    pub root: ChordPitch,
    /// Chord quality.
    pub kind: ChordKind,
    /// Optional bass note for inversions (e.g. C/E).
    #[serde(default, skip_serializing_if = "is_default")]
    pub bass: Option<ChordPitch>,
    /// Degree modifications.
    #[serde(default, skip_serializing_if = "is_default")]
    pub degrees: Vec<ChordDegree>,
    /// Where in the bar, in whole notes from its start.
    #[serde(
        default,
        skip_serializing_if = "is_default",
        deserialize_with = "reduced"
    )]
    pub offset: Frac,
    /// Optional functional-harmony Roman numeral (MusicXML `<function>`, e.g.
    /// `"V"`, `"ii"`). Supplements the chord symbol; `None` for a plain chord
    /// symbol. Omitted from serialization when absent for JSON back-compat.
    #[serde(default, skip_serializing_if = "is_default")]
    pub function: Option<String>,
}

/// A single figure in a figured bass indication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Figure {
    /// The interval number (e.g. 6, 4, 3). None for an empty figure slot.
    #[serde(default, skip_serializing_if = "is_default")]
    pub number: Option<u8>,
    /// Prefix accidental.
    #[serde(default, skip_serializing_if = "is_default")]
    pub prefix: Option<FigureAccidental>,
    /// Suffix accidental.
    #[serde(default, skip_serializing_if = "is_default")]
    pub suffix: Option<FigureAccidental>,
}

/// A figured bass indication.
///
/// Corresponds to MusicXML `<figured-bass>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FiguredBass {
    /// Individual figures (e.g. [6, 4] for a 6/4 chord).
    pub figures: Vec<Figure>,
    /// Duration of the figured bass indication.
    pub duration: Duration,
    /// Whether figures are enclosed in parentheses.
    #[serde(default, skip_serializing_if = "is_default")]
    pub parentheses: bool,
    /// Where in the bar, in whole notes from its start.
    #[serde(
        default,
        skip_serializing_if = "is_default",
        deserialize_with = "reduced"
    )]
    pub offset: Frac,
}

#[cfg(test)]
mod suffix_tests {
    use super::*;

    fn kd(kind: &str, degrees: &[(u8, i8, &str)]) -> (String, Vec<(u8, i8, String)>) {
        (
            kind.to_string(),
            degrees
                .iter()
                .map(|(v, a, t)| (*v, *a, t.to_string()))
                .collect(),
        )
    }

    fn read(steps: Option<ChordSteps>) -> (String, Vec<(u8, i8, String)>) {
        let (k, d) = kind_and_degrees(&steps.expect("a chord"));
        (
            k.to_string(),
            d.into_iter()
                .map(|d| (d.value, d.alter as i8, d.degree_type.to_string()))
                .collect(),
        )
    }

    #[test]
    fn every_kind_reads_back_from_its_own_spellings() {
        for &(kind, _) in KIND_STEPS {
            let ly = ly_chord_modifiers(kind, &[]);
            assert_eq!(read(ly_chord_steps(&ly)).0, kind.as_str(), "LilyPond {ly}");
            let lead = suffix_of_kind(kind);
            assert_eq!(
                read(lead_sheet_chord_steps(lead)).0,
                kind.as_str(),
                "lead sheet {lead}"
            );
        }
    }

    #[test]
    fn lilypond_modifiers_follow_lilypond() {
        assert_eq!(read(ly_chord_steps("maj")), kd("major-seventh", &[]));
        assert_eq!(read(ly_chord_steps("m7+")), kd("major-minor", &[]));
        assert_eq!(
            read(ly_chord_steps("7.9-")),
            kd("dominant", &[(9, -1, "add")])
        );
        assert_eq!(read(ly_chord_steps("9^7")), kd("major", &[(9, 0, "add")]));
        assert_eq!(
            read(ly_chord_steps("7sus4")),
            kd("suspended-fourth", &[(7, 0, "add")])
        );
        assert_eq!(
            read(ly_chord_steps("13.11")),
            kd("dominant-13th", &[(11, 0, "add")])
        );
        assert_eq!(
            read(ly_chord_steps("6.9")),
            kd("major-sixth", &[(9, 0, "add")])
        );
        assert_eq!(read(ly_chord_steps("5")), kd("power", &[]));
        assert!(ly_chord_steps("xyz").is_none());
    }

    #[test]
    fn lead_sheet_suffixes() {
        assert_eq!(
            read(lead_sheet_chord_steps("m7b5")),
            kd("half-diminished", &[])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("7b9")),
            kd("dominant", &[(9, -1, "add")])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("7(#11)")),
            kd("dominant", &[(11, 1, "add")])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("add9")),
            kd("major", &[(9, 0, "add")])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("6/9")),
            kd("major-sixth", &[(9, 0, "add")])
        );
        assert_eq!(read(lead_sheet_chord_steps("Δ")), kd("major-seventh", &[]));
        assert_eq!(
            read(lead_sheet_chord_steps("m(maj7)")),
            kd("major-minor", &[])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("7sus4")),
            kd("suspended-fourth", &[(7, 0, "add")])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("°7")),
            kd("diminished-seventh", &[])
        );
        assert_eq!(read(lead_sheet_chord_steps("13")), kd("dominant-13th", &[]));
        assert_eq!(
            read(lead_sheet_chord_steps("7#5")),
            kd("augmented-seventh", &[])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("7#9")),
            kd("dominant", &[(9, 1, "add")])
        );
        assert_eq!(
            read(lead_sheet_chord_steps("13b9")),
            kd("dominant-13th", &[(9, -1, "alter")])
        );
        assert!(lead_sheet_chord_steps("ine").is_none(), "Fine is no chord");
    }

    #[test]
    fn degrees_spell_back_in_both_syntaxes() {
        for (kind, degrees) in [
            (
                ChordKind::Dominant,
                vec![
                    degree(9, -1, DegreeType::Add),
                    degree(11, 1, DegreeType::Add),
                ],
            ),
            (ChordKind::Major, vec![degree(9, 0, DegreeType::Add)]),
            (ChordKind::Minor, vec![degree(9, 0, DegreeType::Add)]),
            (
                ChordKind::DominantNinth,
                vec![degree(5, 1, DegreeType::Alter)],
            ),
            (
                ChordKind::Dominant,
                vec![degree(5, 0, DegreeType::Subtract)],
            ),
        ] {
            let want = (kind, degrees.clone());
            let ly = ly_chord_modifiers(kind, &degrees);
            let got = kind_and_degrees(&ly_chord_steps(&ly).expect(&ly));
            assert_eq!(got, want, "LilyPond {ly}");
            let lead = lead_sheet_suffix(kind, &degrees);
            let got = kind_and_degrees(&lead_sheet_chord_steps(&lead).expect(&lead));
            assert_eq!(got, want, "lead sheet {lead}");
        }
    }
}
