//! A small ABC 2.1 player used only as a test oracle. It is written from the
//! standard, independently of lytk's reader, and turns ABC text into the notes
//! a standard player sounds: key signatures and bar-scoped accidentals applied,
//! repeats and endings unfolded, tied notes merged.
//!
//! It covers what lytk's ABC writer produces plus the constructs the reader
//! tests need. Anything else is an error, so a fixture is skipped rather than
//! misjudged.

use std::collections::HashMap;

use _core::ir::duration::Frac;

/// How far an accidental carries within a bar (`%%propagate-accidentals`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Propagation {
    /// Same letter, same octave (abcm2ps, abc2svg, engraving practice).
    Octave,
    /// Same letter, every octave (the ABC 2.1 text's default).
    Pitch,
}

/// One sounding note. Grace notes sound at the onset of the note they
/// precede, one after another, like lytk's note arrays.
#[derive(Clone, Debug, PartialEq)]
pub struct OracleNote {
    pub voice: String,
    pub onset: Frac,
    pub duration: Frac,
    pub pitch: i32,
}

fn f(n: i64, d: i64) -> Frac {
    Frac::new(n, d)
}

/// Written items of one voice, before repeats are unfolded.
#[derive(Clone, Debug)]
enum Item {
    Notes {
        pitches: Vec<(i32, bool)>, // (MIDI pitch, tie to the next note)
        dur: Frac,
        grace: bool,
    },
    Rest(Frac),
    Bar {
        start_repeat: bool,
        end_repeat: bool,
        /// `||`, `|]`, `[|`: a later `:|` repeats from here.
        thick: bool,
    },
    Ending(Vec<u32>),
    /// Start of a `{…}` group: its grace notes sound from the main note's
    /// onset, one after another (as in lytk's note arrays).
    GraceGroup,
    /// `&`: what follows sounds from the start of the bar, over what came
    /// before (ABC 2.1 §7.4). The next bar line starts where the longest of
    /// them ends.
    Overlay,
}

#[derive(Clone, Debug)]
struct Voice {
    id: String,
    key: [i32; 7], // alteration per letter C..B
    octave_shift: i32,
    /// `transpose=N`: played `N` semitones from the written notes (§4.6).
    transpose: i32,
    unit: Frac,
    meter: Option<Frac>,
    compound: bool,
    bar_acc: HashMap<(usize, i32), i32>, // (letter, octave or 0) -> alteration
    items: Vec<Item>,
    tuplet: Option<(Frac, u32)>, // (factor, notes left)
    /// (letter, octave, alteration) of the last note or chord's pitches, and
    /// of those tied into the next note: a tied note keeps its pitch over the
    /// bar line (abc2svg, engraving practice).
    spelled: Vec<(usize, i32, i32, bool)>,
    tied: Vec<(usize, i32, i32)>,
}

const SEMIS: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

fn letter_index(c: char) -> Option<usize> {
    "CDEFGAB".find(c.to_ascii_uppercase())
}

/// Parse `n/d`, `C`, `C|`, `(2+3+2)/8`, `2+3+2/8` or `none` into (length,
/// compound).
fn parse_meter(s: &str) -> Result<Option<(Frac, bool)>, String> {
    let s = s.trim();
    match s {
        "C" => return Ok(Some((f(1, 1), false))),
        "C|" => return Ok(Some((f(1, 1), false))),
        "none" | "" => return Ok(None),
        _ => {}
    }
    let (n, d) = s.split_once('/').ok_or(format!("meter {s}"))?;
    let n = n.trim().trim_start_matches('(').trim_end_matches(')');
    let num: i64 = n
        .split('+')
        .map(|x| x.trim().parse::<i64>())
        .sum::<Result<i64, _>>()
        .map_err(|_| format!("meter {s}"))?;
    let den: i64 = d.trim().parse().map_err(|_| format!("meter {s}"))?;
    if num <= 0 || den <= 0 {
        return Err(format!("meter {s}"));
    }
    Ok(Some((f(num, den), num % 3 == 0 && num > 3)))
}

fn parse_len(s: &str) -> Result<Frac, String> {
    let (n, d) = s.trim().split_once('/').ok_or(format!("L:{s}"))?;
    Ok(f(
        n.trim().parse().map_err(|_| format!("L:{s}"))?,
        d.trim().parse().map_err(|_| format!("L:{s}"))?,
    ))
}

/// Key field: the alterations per letter (`None` when the field sets no key,
/// e.g. only a clef), the octave shift (`None` unless `octave=` or a
/// `-8`/`+8` clef gives one; ABC 2.1 §4.6: "the player will transpose") and
/// `transpose=` in semitones.
type KeyField = (Option<[i32; 7]>, Option<i32>, Option<i32>);

fn parse_key(s: &str) -> Result<KeyField, String> {
    let mut key = [0i32; 7];
    let mut shift = None;
    let mut clef_shift = None;
    let mut transpose = None;
    let mut toks = s.split_whitespace().peekable();
    let mut have_key = false;
    if let Some(&first) = toks.peek() {
        if first.eq_ignore_ascii_case("none") {
            toks.next();
            have_key = true;
        } else if first == "HP" || first == "Hp" {
            // Highland pipes: HP shows no signature, Hp shows F# C# with G natural.
            toks.next();
            have_key = true;
            if first == "Hp" {
                key[3] = 1; // F
                key[0] = 1; // C
            }
        } else if let Some(c) = first.chars().next().filter(|c| "ABCDEFG".contains(*c)) {
            if !first.contains('=') {
                toks.next();
                have_key = true;
                let mut rest = &first[1..];
                let mut tonic = major_fifths(c);
                if let Some(r) = rest.strip_prefix('#') {
                    tonic += 7;
                    rest = r;
                } else if let Some(r) = rest.strip_prefix('b') {
                    tonic -= 7;
                    rest = r;
                }
                let mut mode = rest.to_ascii_lowercase();
                if mode.is_empty() {
                    if let Some(&next) = toks.peek() {
                        let m = next.to_ascii_lowercase();
                        if mode_offset(&m).is_some() {
                            mode = m;
                            toks.next();
                        }
                    }
                }
                let off = if mode.is_empty() {
                    0
                } else {
                    mode_offset(&mode).ok_or(format!("mode {mode}"))?
                };
                key = signature(tonic + off);
            }
        }
    }
    for t in toks {
        if t == "exp" {
            key = [0; 7];
            continue;
        }
        if let Some(v) = t.strip_prefix("octave=") {
            shift = Some(v.parse::<i32>().map_err(|_| format!("octave {v}"))?);
            continue;
        }
        if let Some(v) = t.strip_prefix("transpose=") {
            transpose = Some(v.parse::<i32>().map_err(|_| format!("transpose {v}"))?);
            continue;
        }
        let clef = t.strip_prefix("clef=").unwrap_or(t);
        if ["treble", "bass", "alto", "tenor"]
            .iter()
            .any(|c| clef.starts_with(c))
            || clef == "perc"
            || clef == "none"
        {
            if clef.ends_with("-8") {
                clef_shift = Some(-1);
            } else if clef.ends_with("+8") {
                clef_shift = Some(1);
            }
            continue;
        }
        if t.contains('=') && !t.starts_with('=') {
            continue; // middle=, stafflines=, …
        }
        // Explicit accidentals: ^f _b =c ^^g __e
        let acc: String = t.chars().take_while(|c| "^_=".contains(*c)).collect();
        let letter = t[acc.len()..]
            .chars()
            .next()
            .ok_or(format!("key token {t}"))?;
        let i = letter_index(letter).ok_or(format!("key token {t}"))?;
        key[i] = accidental_value(&acc).ok_or(format!("key token {t}"))?;
        have_key = true;
    }
    Ok((have_key.then_some(key), shift.or(clef_shift), transpose))
}

fn major_fifths(c: char) -> i32 {
    match c {
        'C' => 0,
        'G' => 1,
        'D' => 2,
        'A' => 3,
        'E' => 4,
        'B' => 5,
        _ => -1, // F
    }
}

fn mode_offset(m: &str) -> Option<i32> {
    let m3: String = m.chars().take(3).collect();
    Some(match (m, m3.as_str()) {
        ("m", _) => -3,
        (_, "maj" | "ion") => 0,
        (_, "min" | "aeo") => -3,
        (_, "mix") => -1,
        (_, "dor") => -2,
        (_, "phr") => -4,
        (_, "lyd") => 1,
        (_, "loc") => -5,
        _ => return None,
    })
}

/// Alterations per letter C..B for a key with `fifths` sharps (negative: flats).
fn signature(fifths: i32) -> [i32; 7] {
    let mut key = [0i32; 7];
    let sharps = [3usize, 0, 4, 1, 5, 2, 6]; // F C G D A E B
    if fifths >= 0 {
        for k in 0..fifths {
            key[sharps[(k % 7) as usize]] += 1;
        }
    } else {
        for k in 0..(-fifths) {
            key[sharps[6 - (k % 7) as usize]] -= 1;
        }
    }
    key
}

fn accidental_value(acc: &str) -> Option<i32> {
    Some(match acc {
        "" => return None,
        "^" => 1,
        "^^" => 2,
        "_" => -1,
        "__" => -2,
        "=" => 0,
        _ => return None,
    })
}

struct Tune {
    voices: Vec<Voice>,
    current: usize,
    propagation: Propagation,
    default_key: [i32; 7],
    default_shift: i32,
    default_transpose: i32,
    default_unit: Option<Frac>,
    default_meter: Option<(Frac, bool)>,
}

impl Tune {
    fn voice(&mut self, id: &str) -> usize {
        if let Some(i) = self.voices.iter().position(|v| v.id == id) {
            return i;
        }
        let (meter, compound) = match self.default_meter {
            Some((m, c)) => (Some(m), c),
            None => (None, false),
        };
        let unit = self.default_unit.unwrap_or_else(|| match meter {
            Some(m) if m < f(3, 4) => f(1, 16),
            _ => f(1, 8),
        });
        self.voices.push(Voice {
            id: id.to_string(),
            key: self.default_key,
            octave_shift: self.default_shift,
            transpose: self.default_transpose,
            unit,
            meter,
            compound,
            bar_acc: HashMap::new(),
            items: Vec::new(),
            tuplet: None,
            spelled: Vec::new(),
            tied: Vec::new(),
        });
        self.voices.len() - 1
    }

    fn field(&mut self, name: char, value: &str, in_body: bool) -> Result<(), String> {
        let value = value.trim();
        match name {
            'K' => {
                let (key, shift, transpose) = parse_key(value)?;
                if in_body {
                    let v = &mut self.voices[self.current];
                    if let Some(k) = key {
                        v.key = k;
                    }
                    if let Some(s) = shift {
                        v.octave_shift = s;
                    }
                    if let Some(t) = transpose {
                        v.transpose = t;
                    }
                } else {
                    if let Some(k) = key {
                        self.default_key = k;
                    }
                    if let Some(s) = shift {
                        self.default_shift = s;
                    }
                    if let Some(t) = transpose {
                        self.default_transpose = t;
                    }
                }
            }
            'M' => {
                let m = parse_meter(value)?;
                if in_body {
                    let v = &mut self.voices[self.current];
                    v.meter = m.map(|x| x.0);
                    v.compound = m.is_some_and(|x| x.1);
                } else {
                    self.default_meter = m;
                }
            }
            'L' => {
                let l = parse_len(value)?;
                if in_body {
                    self.voices[self.current].unit = l;
                } else {
                    self.default_unit = Some(l);
                }
            }
            'V' => {
                let mut toks = value.split_whitespace();
                let id = toks.next().unwrap_or("1").to_string();
                let i = self.voice(&id);
                for t in toks {
                    if let Some(o) = t.strip_prefix("octave=") {
                        self.voices[i].octave_shift =
                            o.parse().map_err(|_| format!("octave {o}"))?;
                    } else if let Some(n) = t.strip_prefix("transpose=") {
                        self.voices[i].transpose =
                            n.parse().map_err(|_| format!("transpose {n}"))?;
                    } else if t.ends_with("-8") {
                        self.voices[i].octave_shift = -1;
                    } else if t.ends_with("+8") {
                        self.voices[i].octave_shift = 1;
                    }
                }
                if in_body {
                    self.current = i;
                }
            }
            _ => {} // T: C: Q: X: W: w: … carry no notes
        }
        Ok(())
    }
}

/// Play an ABC tune (the first one in the text).
pub fn play(abc: &str, propagation: Propagation) -> Result<Vec<OracleNote>, String> {
    let mut tune = Tune {
        voices: Vec::new(),
        current: 0,
        propagation,
        default_key: [0; 7],
        default_shift: 0,
        default_transpose: 0,
        default_unit: None,
        default_meter: None,
    };
    let mut in_body = false;
    let mut seen_x = false;
    for raw in abc.lines() {
        let line = raw.trim_end();
        if let Some(d) = line.strip_prefix("%%") {
            let mut t = d.split_whitespace();
            if t.next() == Some("propagate-accidentals") {
                tune.propagation = match t.next() {
                    Some("octave") => Propagation::Octave,
                    Some("pitch") => Propagation::Pitch,
                    other => return Err(format!("propagate-accidentals {other:?}")),
                };
            }
            continue;
        }
        if line.trim().is_empty() {
            // An empty line ends the tune body (ABC 2.1 §2.2.1).
            if in_body {
                break;
            }
            continue;
        }
        let line = strip_comment(line);
        if line.trim().is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            let name = bytes[0] as char;
            if name == 'X' {
                if seen_x {
                    break; // only the first tune
                }
                seen_x = true;
                continue;
            }
            if !in_body && name == 'K' {
                tune.field('K', &line[2..], false)?;
                in_body = true;
                // Header V: lines define voices; the body starts in the first
                // one. The header's K:, L: and M: apply to every voice.
                tune.current = 0;
                let (meter, compound) = match tune.default_meter {
                    Some((m, c)) => (Some(m), c),
                    None => (None, false),
                };
                let unit = tune.default_unit.unwrap_or(match meter {
                    Some(m) if m < f(3, 4) => f(1, 16),
                    _ => f(1, 8),
                });
                for v in &mut tune.voices {
                    v.key = tune.default_key;
                    v.octave_shift += tune.default_shift;
                    if v.transpose == 0 {
                        v.transpose = tune.default_transpose;
                    }
                    v.meter = meter;
                    v.compound = compound;
                    v.unit = unit;
                }
                if tune.voices.is_empty() {
                    tune.voice("1");
                }
                continue;
            }
            if name == 'I' {
                // `I:propagate-accidentals …`, the field form of the directive.
                let mut t = line[2..].split_whitespace();
                if t.next() == Some("propagate-accidentals") {
                    tune.propagation = match t.next() {
                        Some("pitch") => Propagation::Pitch,
                        _ => Propagation::Octave,
                    };
                }
                continue;
            }
            if name.is_ascii_lowercase() && !matches!(name, 'w' | 's' | 'r' | 'm') {
                return Err(format!("field {name}:"));
            }
            if matches!(name, 'K' | 'M' | 'L' | 'V') {
                tune.field(name, &line[2..], in_body)?;
            }
            continue;
        }
        if line.starts_with("+:") {
            continue; // continues the previous field line
        }
        if !in_body {
            continue; // free text before the tune
        }
        parse_music(&mut tune, line)?;
    }
    let mut out = Vec::new();
    for v in &tune.voices {
        unfold(v, &mut out)?;
    }
    Ok(out)
}

fn strip_comment(line: &str) -> &str {
    let b = line.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'%' && (i == 0 || b[i - 1] != b'\\') {
            return &line[..i];
        }
    }
    line
}

/// Read an ABC length multiplier (`2`, `/2`, `/`, `//`, `3/2`) at `i`.
fn read_length(c: &[char], i: &mut usize) -> Frac {
    let mut num = String::new();
    while *i < c.len() && c[*i].is_ascii_digit() {
        num.push(c[*i]);
        *i += 1;
    }
    let mut len = f(num.parse().unwrap_or(1), 1);
    while *i < c.len() && c[*i] == '/' {
        *i += 1;
        let mut den = String::new();
        while *i < c.len() && c[*i].is_ascii_digit() {
            den.push(c[*i]);
            *i += 1;
        }
        len /= f(den.parse().unwrap_or(2), 1);
    }
    len
}

/// Read accidental + letter + octave marks at `i`, returning (letter, octave
/// number, explicit alteration).
fn read_pitch(c: &[char], i: &mut usize) -> Option<(usize, i32, Option<i32>)> {
    let start = *i;
    let mut acc = String::new();
    while *i < c.len() && "^_=".contains(c[*i]) {
        acc.push(c[*i]);
        *i += 1;
    }
    let Some(&l) = c.get(*i).filter(|l| "ABCDEFGabcdefg".contains(**l)) else {
        *i = start;
        return None;
    };
    *i += 1;
    let mut oct = if l.is_ascii_lowercase() { 5 } else { 4 };
    while *i < c.len() && (c[*i] == ',' || c[*i] == '\'') {
        oct += if c[*i] == ',' { -1 } else { 1 };
        *i += 1;
    }
    let explicit = if acc.is_empty() {
        None
    } else {
        Some(accidental_value(&acc)?)
    };
    Some((letter_index(l)?, oct, explicit))
}

impl Voice {
    fn sound(
        &mut self,
        letter: usize,
        oct: i32,
        explicit: Option<i32>,
        prop: Propagation,
        grace: bool,
    ) -> i32 {
        let slot = (letter, if prop == Propagation::Octave { oct } else { 0 });
        let alter = match explicit {
            Some(a) => {
                if !grace {
                    self.bar_acc.insert(slot, a);
                }
                a
            }
            None => match self.tied.iter().find(|t| t.0 == letter && t.1 == oct) {
                Some(t) => t.2,
                None => *self.bar_acc.get(&slot).unwrap_or(&self.key[letter]),
            },
        };
        if !grace {
            self.spelled.push((letter, oct, alter, false));
        }
        (oct + 1 + self.octave_shift) * 12 + SEMIS[letter] + alter + self.transpose
    }

    /// A note or chord has sounded: its ties are used up, and any written
    /// inside a chord (`[C-E]`) carry on.
    fn sounded(&mut self) {
        self.tied = self
            .spelled
            .iter()
            .filter(|s| s.3)
            .map(|s| (s.0, s.1, s.2))
            .collect();
    }

    fn push_timed(&mut self, mut item: Item) {
        if let Some((factor, left)) = self.tuplet {
            match &mut item {
                Item::Notes {
                    dur, grace: false, ..
                }
                | Item::Rest(dur) => *dur *= factor,
                _ => {}
            }
            let left = left - 1;
            self.tuplet = (left > 0).then_some((factor, left));
        }
        self.items.push(item);
    }
}

fn parse_music(tune: &mut Tune, line: &str) -> Result<(), String> {
    let c: Vec<char> = line.chars().collect();
    let prop = tune.propagation;
    let mut i = 0;
    // Pending broken-rhythm factor for the next timed item.
    let mut broken_next: Option<Frac> = None;
    while i < c.len() {
        let ch = c[i];
        let v = tune.current;
        match ch {
            ' ' | '\t' | '`' | '\\' | ')' | 'y' | '$' => i += 1,
            '"' => {
                i += 1;
                while i < c.len() && c[i] != '"' {
                    i += 1;
                }
                i += 1;
            }
            '!' | '+' => {
                // `!deco!` / legacy `+deco+`. A lone `!` (ABC 2.0 line break)
                // has no decoration name before the next mark: skip it alone.
                let name = |k: usize| {
                    let t = &c[i + 1..i + 1 + k];
                    !t.is_empty()
                        && t.iter()
                            .all(|x| !x.is_whitespace() && !"|[]{}\"".contains(*x))
                };
                match c[i + 1..].iter().position(|x| *x == ch) {
                    Some(k) if name(k) => i += k + 2,
                    _ => i += 1,
                }
            }
            '.' | '~' | 'H' | 'L' | 'M' | 'O' | 'P' | 'S' | 'T' | 'u' | 'v' => i += 1,
            '(' => {
                if c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                    i += 1;
                    let mut nums: Vec<Option<i64>> = Vec::new();
                    loop {
                        let mut n = String::new();
                        while i < c.len() && c[i].is_ascii_digit() {
                            n.push(c[i]);
                            i += 1;
                        }
                        nums.push(n.parse().ok());
                        if i < c.len() && c[i] == ':' && nums.len() < 3 {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    let p = nums[0].ok_or("tuplet")?;
                    let compound = tune.voices[v].compound;
                    let q = nums.get(1).copied().flatten().unwrap_or(match p {
                        2 | 4 | 8 => 3,
                        3 | 6 => 2,
                        _ => {
                            if compound {
                                3
                            } else {
                                2
                            }
                        }
                    });
                    let r = nums.get(2).copied().flatten().unwrap_or(p);
                    tune.voices[v].tuplet = Some((f(q, p), r as u32));
                } else {
                    i += 1; // slur
                }
            }
            '{' => {
                tune.voices[v].items.push(Item::GraceGroup);
                let close = c[i..].iter().position(|x| *x == '}').ok_or("unclosed {")?;
                let inner: Vec<char> = c[i + 1..i + close].to_vec();
                let mut j = 0;
                while j < inner.len() {
                    if inner[j] == '/' || inner[j] == ' ' {
                        j += 1;
                        continue;
                    }
                    let (letter, oct, explicit) =
                        read_pitch(&inner, &mut j).ok_or(format!("grace {:?}", inner))?;
                    let len = read_length(&inner, &mut j);
                    let tie = inner.get(j) == Some(&'-');
                    if tie {
                        j += 1;
                    }
                    let voice = &mut tune.voices[v];
                    let pitch = voice.sound(letter, oct, explicit, prop, true);
                    let dur = voice.unit * len;
                    voice.items.push(Item::Notes {
                        pitches: vec![(pitch, tie)],
                        dur,
                        grace: true,
                    });
                }
                i += close + 1;
            }
            '[' => {
                // Inline field, ending, bar line or chord.
                if c.get(i + 2) == Some(&':')
                    && c.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic())
                {
                    let close = c[i..].iter().position(|x| *x == ']').ok_or("unclosed [")?;
                    let field: String = c[i + 3..i + close].iter().collect();
                    tune.field(c[i + 1], &field, true)?;
                    i += close + 1;
                } else if c.get(i + 1).is_some_and(|x| x.is_ascii_digit()) {
                    i += 1;
                    let nums = read_ending(&c, &mut i)?;
                    tune.voices[v].items.push(Item::Ending(nums));
                } else if c.get(i + 1) == Some(&'|') {
                    i += 2;
                    if c.get(i) == Some(&']') {
                        i += 1;
                    }
                    bar(&mut tune.voices[v], false, false, true);
                } else {
                    i += 1;
                    let mut pitches = Vec::new();
                    tune.voices[v].spelled.clear();
                    let mut first_len = None;
                    while i < c.len() && c[i] != ']' {
                        if matches!(c[i], ' ' | '.' | '~') {
                            i += 1;
                            continue;
                        }
                        if c[i] == '!' {
                            let close = c[i + 1..]
                                .iter()
                                .position(|x| *x == '!')
                                .ok_or("unclosed !")?;
                            i += close + 2;
                            continue;
                        }
                        let (letter, oct, explicit) =
                            read_pitch(&c, &mut i).ok_or(format!("chord at {i} in {line}"))?;
                        let len = read_length(&c, &mut i);
                        first_len.get_or_insert(len);
                        let tie = c.get(i) == Some(&'-');
                        if tie {
                            i += 1;
                        }
                        let voice = &mut tune.voices[v];
                        let pitch = voice.sound(letter, oct, explicit, prop, false);
                        if let Some(s) = voice.spelled.last_mut() {
                            s.3 = tie;
                        }
                        pitches.push((pitch, tie));
                    }
                    i += 1; // ]
                    tune.voices[v].sounded();
                    let mult = read_length(&c, &mut i);
                    if c.get(i) == Some(&'-') {
                        i += 1;
                        for p in &mut pitches {
                            p.1 = true;
                        }
                        let voice = &mut tune.voices[v];
                        voice.tied = voice.spelled.iter().map(|s| (s.0, s.1, s.2)).collect();
                    }
                    let voice = &mut tune.voices[v];
                    let mut dur = voice.unit * first_len.unwrap_or(f(1, 1)) * mult;
                    if let Some(b) = broken_next.take() {
                        dur *= b;
                    }
                    voice.push_timed(Item::Notes {
                        pitches,
                        dur,
                        grace: false,
                    });
                }
            }
            '|' | ':' => {
                let start = i;
                while i < c.len() && matches!(c[i], '|' | ':' | ']' | '[') {
                    // `[` only as part of `|[`? No: `[` opens chords/endings; stop.
                    if c[i] == '[' {
                        break;
                    }
                    i += 1;
                }
                let tok: String = c[start..i].iter().collect();
                if tok == ":" {
                    return Err(format!("stray ':' in {line}"));
                }
                // `::` (and `:|:`, `:||:`) ends one repeat and starts the next.
                let end_repeat = tok.starts_with(':');
                let start_repeat = tok.ends_with(':');
                let thick = tok.contains("||") || tok.contains(']');
                bar(&mut tune.voices[v], start_repeat, end_repeat, thick);
                if c.get(i).is_some_and(|x| x.is_ascii_digit()) {
                    let nums = read_ending(&c, &mut i)?;
                    tune.voices[v].items.push(Item::Ending(nums));
                }
            }
            '>' | '<' => {
                let mut n = 0;
                while i < c.len() && c[i] == ch {
                    n += 1;
                    i += 1;
                }
                let short = f(1, 1 << n);
                let long = f(2, 1) - short;
                let (prev, next) = if ch == '>' {
                    (long, short)
                } else {
                    (short, long)
                };
                let voice = &mut tune.voices[v];
                match voice
                    .items
                    .iter_mut()
                    .rev()
                    .find(|it| matches!(it, Item::Notes { grace: false, .. } | Item::Rest(_)))
                {
                    Some(Item::Notes { dur, .. }) | Some(Item::Rest(dur)) => *dur *= prev,
                    _ => return Err("broken rhythm without a note".into()),
                }
                broken_next = Some(next);
            }
            'z' | 'x' => {
                i += 1;
                let len = read_length(&c, &mut i);
                let voice = &mut tune.voices[v];
                voice.tied.clear();
                let mut dur = voice.unit * len;
                if let Some(b) = broken_next.take() {
                    dur *= b;
                }
                voice.push_timed(Item::Rest(dur));
            }
            'Z' | 'X' => {
                i += 1;
                let mut n = String::new();
                while i < c.len() && c[i].is_ascii_digit() {
                    n.push(c[i]);
                    i += 1;
                }
                let bars: i64 = n.parse().unwrap_or(1);
                let voice = &mut tune.voices[v];
                let meter = voice.meter.ok_or("Z without a meter")?;
                voice.items.push(Item::Rest(meter * f(bars, 1)));
            }
            '-' => {
                // Tie after a note or chord: all its pitches.
                let voice = &mut tune.voices[v];
                match voice.items.last_mut() {
                    Some(Item::Notes { pitches, .. }) => {
                        for p in pitches {
                            p.1 = true;
                        }
                    }
                    _ => return Err("tie without a note".into()),
                }
                voice.tied = voice.spelled.iter().map(|s| (s.0, s.1, s.2)).collect();
                i += 1;
            }
            '&' => {
                tune.voices[v].items.push(Item::Overlay);
                i += 1;
            }
            _ => {
                let Some((letter, oct, explicit)) = read_pitch(&c, &mut i) else {
                    return Err(format!("unexpected {ch:?} in {line}"));
                };
                let len = read_length(&c, &mut i);
                let voice = &mut tune.voices[v];
                voice.spelled.clear();
                let pitch = voice.sound(letter, oct, explicit, prop, false);
                voice.sounded();
                let mut dur = voice.unit * len;
                if let Some(b) = broken_next.take() {
                    dur *= b;
                }
                voice.push_timed(Item::Notes {
                    pitches: vec![(pitch, false)],
                    dur,
                    grace: false,
                });
            }
        }
    }
    Ok(())
}

fn read_ending(c: &[char], i: &mut usize) -> Result<Vec<u32>, String> {
    let mut s = String::new();
    while *i < c.len() && (c[*i].is_ascii_digit() || c[*i] == ',' || c[*i] == '-') {
        s.push(c[*i]);
        *i += 1;
    }
    let mut nums = Vec::new();
    for part in s.split(',').filter(|p| !p.is_empty()) {
        if let Some((a, b)) = part.split_once('-') {
            let (a, b): (u32, u32) = (
                a.parse().map_err(|_| format!("ending {s}"))?,
                b.parse().map_err(|_| format!("ending {s}"))?,
            );
            nums.extend(a..=b);
        } else {
            nums.push(part.parse().map_err(|_| format!("ending {s}"))?);
        }
    }
    Ok(nums)
}

fn bar(v: &mut Voice, start_repeat: bool, end_repeat: bool, thick: bool) {
    v.bar_acc.clear();
    v.items.push(Item::Bar {
        start_repeat,
        end_repeat,
        thick,
    });
}

/// Unfold repeats and endings, then merge ties and emit notes.
fn unfold(v: &Voice, out: &mut Vec<OracleNote>) -> Result<(), String> {
    let items = &v.items;
    let mut t = f(0, 1);
    // Where the bar started and how far its overlays reach.
    let (mut bar_start, mut bar_end) = (t, t);
    // (pitch, duration, tie, first of its group)
    let mut pending_grace: Vec<(i32, Frac, bool, bool)> = Vec::new();
    let mut group_start = false;
    // pitch -> index into `out` of a note whose tie is still open
    let mut open: HashMap<i32, usize> = HashMap::new();
    let mut i = 0;
    let mut pass = 1u32;
    // Where a `:|` goes back to: the last `|:`, thick bar or finished repeat.
    let mut section_start = 0usize;
    let mut jumps = 0;
    // How many times the section from `from` plays: the largest ending number
    // (2 without endings), up to the `:|` no ending follows — that repeat is
    // over, and later endings are another section's.
    let passes_for = |from: usize| -> u32 {
        let mut max = 2;
        for (k, it) in items.iter().enumerate().skip(from) {
            match it {
                Item::Ending(n) => max = max.max(*n.iter().max().unwrap_or(&2)),
                Item::Bar {
                    start_repeat: true, ..
                } => break,
                Item::Bar {
                    end_repeat: true, ..
                } if !matches!(items.get(k + 1), Some(Item::Ending(_))) => break,
                _ => {}
            }
        }
        max
    };
    while i < items.len() {
        match &items[i] {
            Item::Bar {
                start_repeat,
                end_repeat,
                thick,
            } => {
                t = t.max(bar_end);
                (bar_start, bar_end) = (t, t);
                if *end_repeat && pass < passes_for(section_start) {
                    pass += 1;
                    i = section_start;
                    jumps += 1;
                    if jumps > 1000 {
                        return Err("repeat loop".into());
                    }
                    continue;
                }
                if *end_repeat || *start_repeat || *thick {
                    // A finished repeat, a new one, or a thick bar: the next
                    // `:|` goes back to here.
                    section_start = i + 1;
                    pass = 1;
                }
                i += 1;
            }
            Item::Ending(nums) => {
                if nums.contains(&pass) {
                    i += 1;
                } else {
                    // Skip this ending: up to the ending for this pass, or past
                    // the `:|` (or thick bar) that closes the skipped one.
                    i += 1;
                    while i < items.len() {
                        match &items[i] {
                            Item::Ending(n) if n.contains(&pass) => break,
                            Item::Bar {
                                start_repeat: true, ..
                            } => break,
                            Item::Bar {
                                end_repeat, thick, ..
                            } if *end_repeat || *thick => {
                                i += 1;
                                if !matches!(items.get(i), Some(Item::Ending(_))) {
                                    pass = 1;
                                    section_start = i;
                                }
                                break;
                            }
                            _ => i += 1,
                        }
                    }
                }
            }
            Item::GraceGroup => {
                group_start = true;
                i += 1;
            }
            Item::Overlay => {
                flush_graces(&mut pending_grace, t, v, out);
                open.clear();
                bar_end = bar_end.max(t);
                t = bar_start;
                i += 1;
            }
            Item::Rest(d) => {
                open.clear();
                flush_graces(&mut pending_grace, t, v, out);
                t += *d;
                i += 1;
            }
            Item::Notes {
                pitches,
                dur,
                grace: true,
            } => {
                pending_grace.push((pitches[0].0, *dur, pitches[0].1, group_start));
                group_start = false;
                i += 1;
            }
            Item::Notes {
                pitches,
                dur,
                grace: false,
            } => {
                // A tie reaches only the next note (or chord) of the voice. A
                // grace tied to its main note fuses with it, as in note arrays.
                let mut prev_open = std::mem::take(&mut open);
                prev_open.extend(flush_graces(&mut pending_grace, t, v, out));
                for &(p, tie) in pitches {
                    if let Some(k) = prev_open.remove(&p) {
                        out[k].duration += *dur;
                        if tie {
                            open.insert(p, k);
                        }
                    } else {
                        out.push(OracleNote {
                            voice: v.id.clone(),
                            onset: t,
                            duration: *dur,
                            pitch: p,
                        });
                        if tie {
                            open.insert(p, out.len() - 1);
                        }
                    }
                }
                t += *dur;
                i += 1;
            }
        }
    }
    flush_graces(&mut pending_grace, t, v, out);
    Ok(())
}

/// Sound pending grace notes at `t`: each `{…}` group from `t`, its notes one
/// after another (as lytk's note arrays place them). Returns the grace notes
/// tied into the main note, by pitch.
fn flush_graces(
    pending: &mut Vec<(i32, Frac, bool, bool)>,
    t: Frac,
    v: &Voice,
    out: &mut Vec<OracleNote>,
) -> Vec<(i32, usize)> {
    let mut tied = Vec::new();
    let mut g = t;
    for (p, d, tie, first) in pending.drain(..) {
        if first {
            g = t;
        }
        out.push(OracleNote {
            voice: v.id.clone(),
            onset: g,
            duration: d,
            pitch: p,
        });
        if tie {
            tied.push((p, out.len() - 1));
        }
        g += d;
    }
    tied
}
