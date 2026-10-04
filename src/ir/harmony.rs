//! Harmony (chord symbol) and figured bass IR types.
//!
//! These represent chord symbols (`Cmaj7`, `Dm/F`) and figured bass notation
//! at the measure level, parallel to notes.

use serde::{Deserialize, Serialize};

use super::duration::Duration;

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
const KIND_STEPS: &[(&str, &[(u8, i8)])] = &[
    ("major", &[(3, 0), (5, 0)]),
    ("minor", &[(3, -1), (5, 0)]),
    ("augmented", &[(3, 0), (5, 1)]),
    ("diminished", &[(3, -1), (5, -1)]),
    ("suspended-fourth", &[(4, 0), (5, 0)]),
    ("suspended-second", &[(2, 0), (5, 0)]),
    ("power", &[(5, 0)]),
    ("dominant", &[(3, 0), (5, 0), (7, 0)]),
    ("major-seventh", &[(3, 0), (5, 0), (7, 1)]),
    ("minor-seventh", &[(3, -1), (5, 0), (7, 0)]),
    ("diminished-seventh", &[(3, -1), (5, -1), (7, -1)]),
    ("half-diminished", &[(3, -1), (5, -1), (7, 0)]),
    ("augmented-seventh", &[(3, 0), (5, 1), (7, 0)]),
    ("major-minor", &[(3, -1), (5, 0), (7, 1)]),
    ("major-sixth", &[(3, 0), (5, 0), (6, 0)]),
    ("minor-sixth", &[(3, -1), (5, 0), (6, 0)]),
    ("dominant-ninth", &[(3, 0), (5, 0), (7, 0), (9, 0)]),
    ("major-ninth", &[(3, 0), (5, 0), (7, 1), (9, 0)]),
    ("minor-ninth", &[(3, -1), (5, 0), (7, 0), (9, 0)]),
    ("dominant-11th", &[(3, 0), (5, 0), (7, 0), (9, 0), (11, 0)]),
    ("major-11th", &[(3, 0), (5, 0), (7, 1), (9, 0), (11, 0)]),
    ("minor-11th", &[(3, -1), (5, 0), (7, 0), (9, 0), (11, 0)]),
    // A 13th chord leaves out the 11th (it clashes with the 3rd), as
    // LilyPond and players do; a minor one keeps it.
    ("dominant-13th", &[(3, 0), (5, 0), (7, 0), (9, 0), (13, 0)]),
    ("major-13th", &[(3, 0), (5, 0), (7, 1), (9, 0), (13, 0)]),
    (
        "minor-13th",
        &[(3, -1), (5, 0), (7, 0), (9, 0), (11, 0), (13, 0)],
    ),
];

/// The steps of a MusicXML chord kind, `None` for a kind without them
/// (`none`, `other`, `pedal`, the augmented sixths …).
pub fn kind_steps(kind: &str) -> Option<ChordSteps> {
    KIND_STEPS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, steps)| steps.iter().copied().collect())
}

/// The steps of a kind with its degree changes applied.
pub fn chord_steps(kind: &str, degrees: &[ChordDegree]) -> Option<ChordSteps> {
    let mut steps = kind_steps(kind)?;
    for d in degrees {
        let alter = d.alter.round() as i8;
        match d.degree_type.as_str() {
            "subtract" => {
                steps.remove(&d.value);
            }
            "alter" => {
                let base = steps.get(&d.value).copied().unwrap_or(0);
                steps.insert(d.value, base + alter);
            }
            _ => {
                steps.insert(d.value, alter);
            }
        }
    }
    Some(steps)
}

/// The MusicXML kind nearest to some steps, and the degrees that make the
/// difference (added, altered or removed), lowest step first.
pub fn kind_and_degrees(steps: &ChordSteps) -> (&'static str, Vec<ChordDegree>) {
    let diff = |template: &ChordSteps| -> Vec<ChordDegree> {
        let mut out = Vec::new();
        for (&step, &alter) in steps {
            match template.get(&step) {
                None => out.push(degree(step, alter, "add")),
                Some(&t) if t != alter => out.push(degree(step, alter - t, "alter")),
                Some(_) => {}
            }
        }
        for &step in template.keys() {
            if !steps.contains_key(&step) {
                out.push(degree(step, 0, "subtract"));
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
            let removed = d.iter().filter(|d| d.degree_type == "subtract").count();
            (d.len(), removed, *misses)
        })
        .map(|(kind, d, _)| (kind, d))
        .unwrap_or(("major", Vec::new()))
}

fn degree(value: u8, alter: i8, degree_type: &str) -> ChordDegree {
    ChordDegree {
        value,
        alter: f64::from(alter),
        degree_type: degree_type.to_string(),
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
pub fn lead_sheet_suffix(kind: &str, degrees: &[ChordDegree]) -> String {
    let mut out = suffix_of_kind(kind).to_string();
    for d in degrees {
        let sign = match d.alter.round() as i32 {
            a if a < 0 => "b",
            a if a > 0 => "#",
            _ => "",
        };
        match d.degree_type.as_str() {
            "subtract" => out.push_str(&format!("no{}", d.value)),
            "add" if sign.is_empty() => out.push_str(&format!("add{}", d.value)),
            _ => out.push_str(&format!("{sign}{}", d.value)),
        }
    }
    out
}

/// LilyPond chord modifiers for a kind and its degrees, without the `:`
/// (`m7.5-`, `7.9-`, `3.5.9`, `sus4.7`); empty for a plain major triad.
pub fn ly_chord_modifiers(kind: &str, degrees: &[ChordDegree]) -> String {
    let base = match kind {
        "major" => "",
        "minor" => "m",
        "augmented" => "aug",
        "diminished" => "dim",
        "suspended-fourth" => "sus4",
        "suspended-second" => "sus2",
        "power" => "5",
        "dominant" => "7",
        "major-seventh" => "maj7",
        "minor-seventh" => "m7",
        "diminished-seventh" => "dim7",
        "half-diminished" => "m7.5-",
        "augmented-seventh" => "aug7",
        "major-minor" => "m7+",
        "major-sixth" => "6",
        "minor-sixth" => "m6",
        "dominant-ninth" => "9",
        "major-ninth" => "maj9",
        "minor-ninth" => "m9",
        "dominant-11th" => "11",
        "major-11th" => "maj11",
        "minor-11th" => "m11",
        "dominant-13th" => "13",
        "major-13th" => "maj13",
        "minor-13th" => "m13",
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
        match d.degree_type.as_str() {
            "subtract" => removals.push(d.value.to_string()),
            _ => adds.push(format!("{}{sign}", d.value)),
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
pub fn parse_chord_suffix(suffix: &str) -> Option<(&'static str, Vec<ChordDegree>)> {
    lead_sheet_chord_steps(suffix).map(|s| kind_and_degrees(&s))
}

/// The lead-sheet suffix of a MusicXML chord kind (`minor-seventh` → `m7`).
pub fn suffix_of_kind(kind: &str) -> &'static str {
    match kind {
        "minor" => "m",
        "dominant" => "7",
        "major-seventh" => "maj7",
        "minor-seventh" => "m7",
        "diminished" => "dim",
        "diminished-seventh" => "dim7",
        "augmented" => "aug",
        "augmented-seventh" => "aug7",
        "half-diminished" => "m7b5",
        "major-minor" => "m(maj7)",
        "major-sixth" => "6",
        "minor-sixth" => "m6",
        "dominant-ninth" => "9",
        "major-ninth" => "maj9",
        "minor-ninth" => "m9",
        "dominant-11th" => "11",
        "major-11th" => "maj11",
        "minor-11th" => "m11",
        "dominant-13th" => "13",
        "major-13th" => "maj13",
        "minor-13th" => "m13",
        "suspended-second" => "sus2",
        "suspended-fourth" => "sus4",
        "power" => "5",
        _ => "",
    }
}

/// A pitch used in chord symbol descriptions (root or bass).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordPitch {
    /// Note step: C, D, E, F, G, A, B.
    pub step: String,
    /// Chromatic alteration in semitones (-2.0 to 2.0).
    pub alter: f64,
}

/// A chord degree modification (add, subtract, or alter a scale degree).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordDegree {
    /// Scale degree number (1–13).
    pub value: u8,
    /// Alteration in semitones (-2.0 to 2.0).
    pub alter: f64,
    /// Type: "add", "subtract", or "alter".
    pub degree_type: String,
}

/// A harmony / chord symbol.
///
/// Corresponds to MusicXML `<harmony>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Harmony {
    /// Root pitch of the chord.
    pub root: ChordPitch,
    /// Chord quality: "major", "minor", "dominant", "diminished",
    /// "augmented", "half-diminished", "major-seventh", etc.
    pub kind: String,
    /// Optional bass note for inversions (e.g. C/E).
    pub bass: Option<ChordPitch>,
    /// Degree modifications.
    pub degrees: Vec<ChordDegree>,
    /// Position in the measure (offset from measure start in divisions).
    pub offset: i32,
    /// Optional functional-harmony Roman numeral (MusicXML `<function>`, e.g.
    /// `"V"`, `"ii"`). Supplements the chord symbol; `None` for a plain chord
    /// symbol. Omitted from serialization when absent for JSON back-compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

/// A single figure in a figured bass indication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Figure {
    /// The interval number (e.g. 6, 4, 3). None for an empty figure slot.
    pub number: Option<u8>,
    /// Prefix accidental: "sharp", "flat", "natural", "double-sharp", etc.
    pub prefix: Option<String>,
    /// Suffix accidental.
    pub suffix: Option<String>,
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
    pub parentheses: bool,
    /// Position in the measure (offset from measure start in divisions).
    pub offset: i32,
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
                .map(|d| (d.value, d.alter as i8, d.degree_type))
                .collect(),
        )
    }

    #[test]
    fn every_kind_reads_back_from_its_own_spellings() {
        for (kind, _) in KIND_STEPS {
            let ly = ly_chord_modifiers(kind, &[]);
            assert_eq!(read(ly_chord_steps(&ly)).0, *kind, "LilyPond {ly}");
            let lead = suffix_of_kind(kind);
            assert_eq!(
                read(lead_sheet_chord_steps(lead)).0,
                *kind,
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
            ("dominant", vec![degree(9, -1, "add"), degree(11, 1, "add")]),
            ("major", vec![degree(9, 0, "add")]),
            ("minor", vec![degree(9, 0, "add")]),
            ("dominant-ninth", vec![degree(5, 1, "alter")]),
            ("dominant", vec![degree(5, 0, "subtract")]),
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
