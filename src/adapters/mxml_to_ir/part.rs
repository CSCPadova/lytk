//! Part-level and measure-level conversion from musicxml crate types to IR.

use std::collections::{BTreeMap, HashMap};

use crate::ir::direction::{Barline, BarlineType, Direction, RepeatDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::measure::*;
use crate::ir::note::*;
use crate::ir::part::Part;
use crate::ir::timeline::{Timeline, OFFSET_DIVISIONS};
use crate::ir::voice::Voice;

use super::direction::{convert_direction, convert_figured_bass_elem, convert_harmony_elem};
use super::note::{convert_note, NoteOrRest};
use super::PartInfo;
use super::Result;

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

// ---------------------------------------------------------------------------
// Part & measure conversion
// ---------------------------------------------------------------------------

pub(super) fn convert_part(mxml_part: &mxml::Part, info: &PartInfo) -> Result<Part> {
    let mut part = Part {
        name: info.name.clone(),
        abbreviation: info.abbreviation.clone(),
        part_id: info.id.clone(),
        midi_instrument: info.midi_instrument.clone(),
        midi_channel: info.midi_channel,
        midi_program: info.midi_program,
        staves: 1,
        measures: Vec::new(),
    };

    let mut divisions: i64 = undeclared_divisions(mxml_part).unwrap_or(1);

    for part_elem in &mxml_part.content {
        if let mxml::PartElement::Measure(mxml_measure) = part_elem {
            let (measure, new_divisions) = convert_measure(mxml_measure, divisions)?;
            divisions = new_divisions;
            if let Some(ref attrs) = measure.attributes {
                if let Some(s) = attrs.staves {
                    part.staves = s;
                }
            }
            part.measures.push(measure);
        }
    }

    super::note::number_verses(&mut part);
    Ok(part)
}

/// The divisions of a part that times notes before declaring any (lytk 0.4.0
/// wrote such files): taken from its first timed note, its `<duration>` over
/// its written value. `None` when `<divisions>` comes first, as it must.
fn undeclared_divisions(mxml_part: &mxml::Part) -> Option<i64> {
    for part_elem in &mxml_part.content {
        let mxml::PartElement::Measure(m) = part_elem else {
            continue;
        };
        for elem in &m.content {
            match elem {
                mxml::MeasureElement::Attributes(a) if a.content.divisions.is_some() => {
                    return None
                }
                mxml::MeasureElement::Note(n) => {
                    let d = note_duration_divisions(n);
                    let written = match convert_note(n, 1) {
                        Some(NoteOrRest::Note(note)) if !note.is_grace => {
                            note.duration.actual_duration()
                        }
                        Some(NoteOrRest::Rest(r)) if !r.is_measure_rest => {
                            r.duration.actual_duration()
                        }
                        _ => continue,
                    };
                    if d <= 0 {
                        continue;
                    }
                    let per_quarter = Frac::from_integer(d) / (written * Frac::from_integer(4));
                    return per_quarter
                        .is_integer()
                        .then(|| per_quarter.to_integer())
                        .filter(|&v| (1..=u16::MAX as i64).contains(&v));
                }
                _ => {}
            }
        }
    }
    None
}

fn convert_measure(
    mxml_measure: &mxml::Measure,
    mut divisions: i64,
) -> Result<(crate::ir::measure::Measure, i64)> {
    let raw_number = mxml_measure.attributes.number.0.clone();
    let number: u32 = raw_number.parse().unwrap_or(0);
    // Preserve the original label whenever it isn't exactly the decimal form of
    // `number` (e.g. "3A", "X1", or "03"); a plain integer needs no label.
    let number_label = if number.to_string() == raw_number {
        None
    } else {
        Some(raw_number)
    };
    let implicit = mxml_measure.attributes.implicit == Some(mdt::YesNo::Yes);
    let width: Option<f32> = mxml_measure.attributes.width.as_ref().map(|w| w.0 as f32);

    let mut measure = crate::ir::measure::Measure {
        number,
        number_label,
        implicit,
        senza_misura: false,
        width,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: Vec::new(),
        harmonies: Vec::new(),
        figured_bass: Vec::new(),
        print_object: true,
        multi_measure_rest: None,
        measure_repeat: None,
        voices: Vec::new(),
    };

    // MusicXML places notes with a cursor that `<backup>` and `<forward>`
    // move: each element is kept at its onset in the bar (whole notes), and
    // the voices are laid out by onset at the end, not in file order.
    let mut placed: HashMap<u8, Vec<(Frac, VoiceElement)>> = HashMap::new();
    let mut pending_arpeggio: HashMap<u8, ArpeggioType> = HashMap::new();
    let mut cursor = Frac::from_integer(0);
    // Where the last non-chord note started: a `<chord/>` note sounds there.
    let mut last_onset = cursor;
    // The voice of the last note or rest and where it ended.
    let mut last_voice: Option<(u8, Frac)> = None;
    let whole = |d: i64, divisions: i64| Frac::new(d, 4 * divisions.max(1));
    // Harmonies written one after another at the cursor without `<offset>`
    // change during the note that follows (MusicXML suite 71g): their
    // indices in `measure.harmonies`, spread over that note when it comes.
    let mut stacked: Vec<usize> = Vec::new();

    for elem in &mxml_measure.content {
        match elem {
            mxml::MeasureElement::Attributes(attrs) => {
                let (mut ir_attrs, new_div) = convert_attributes(attrs, divisions);
                divisions = new_div;
                // A clef inside the bar keeps its place, as a direction.
                if cursor > Frac::from_integer(0) {
                    for (staff, clef) in std::mem::take(&mut ir_attrs.clefs) {
                        measure.directions.push(Direction {
                            offset: (cursor * Frac::from_integer(4 * divisions)).to_integer()
                                as i32,
                            offset_frac: cursor,
                            staff,
                            clef: Some(clef),
                            ..Default::default()
                        });
                    }
                }
                // A later `<attributes>` in the bar (a clef change between
                // notes) adds to the bar's, it doesn't replace its key or time.
                measure.attributes = Some(match measure.attributes.take() {
                    Some(earlier) => merge_attributes(earlier, ir_attrs),
                    None => ir_attrs,
                });
                // <measure-style> for multiple-rest and measure-repeat
                for ms in &attrs.content.measure_style {
                    match &ms.content {
                        mxml::MeasureStyleContents::MultipleRest(mr) => {
                            measure.multi_measure_rest = Some(mr.content.0 as u16);
                        }
                        // Only the "start" carries the repeated-measure count
                        // ("stop" marks the end and its content is ignored).
                        mxml::MeasureStyleContents::MeasureRepeat(mrep)
                            if mrep.attributes.r#type != Some(mdt::StartStop::Stop) =>
                        {
                            if let mdt::PositiveIntegerOrEmpty::Integer(n) = mrep.content {
                                measure.measure_repeat = Some(n as u8);
                            }
                        }
                        _ => {}
                    }
                }
            }
            mxml::MeasureElement::Print(print) => {
                if print.attributes.new_page == Some(mdt::YesNo::Yes) {
                    measure.directions.push(Direction {
                        layout_break: Some(crate::ir::direction::LayoutBreakType::Page),
                        ..Default::default()
                    });
                } else if print.attributes.new_system == Some(mdt::YesNo::Yes) {
                    measure.directions.push(Direction {
                        layout_break: Some(crate::ir::direction::LayoutBreakType::System),
                        ..Default::default()
                    });
                }
            }
            mxml::MeasureElement::Note(mxml_note) => {
                let is_chord = note_is_chord(mxml_note);
                let is_grace = note_is_grace(mxml_note);

                // Detect arpeggio from notations
                let arpeggio = detect_arpeggio(mxml_note);

                let result = convert_note(mxml_note, divisions);

                // A chord note sounds with the note before it; a grace note
                // takes no time. A `<duration>` of 0 (malformed) falls back to
                // the written value.
                let dur = match note_duration_divisions(mxml_note) {
                    d if d > 0 => whole(d, divisions),
                    _ => match &result {
                        Some(NoteOrRest::Note(n)) if !n.is_grace => n.duration.actual_duration(),
                        Some(NoteOrRest::Rest(r)) => r.duration.actual_duration(),
                        _ => Frac::from_integer(0),
                    },
                };
                let onset = if is_chord { last_onset } else { cursor };
                if !is_chord {
                    if stacked.len() > 1 && !is_grace {
                        let n = stacked.len() as i64;
                        for (k, &i) in stacked.iter().enumerate() {
                            let at = cursor + dur * Frac::new(k as i64, n);
                            measure.harmonies[i].offset = ir_offset(at);
                        }
                    }
                    stacked.clear();
                    last_onset = cursor;
                    if !is_grace {
                        cursor += dur;
                    }
                }

                match result {
                    Some(NoteOrRest::Note(note)) => {
                        let voice_num = note.voice;
                        let elements = placed.entry(voice_num).or_default();
                        if is_chord {
                            let pending = pending_arpeggio.remove(&voice_num);
                            let arp = arpeggio.or(pending);
                            merge_chord(elements, onset, *note, arp);
                        } else {
                            elements.push((onset, VoiceElement::Note(note)));
                            if let Some(arp) = arpeggio {
                                pending_arpeggio.insert(voice_num, arp);
                            }
                        }
                        last_voice = Some((voice_num, cursor));
                    }
                    Some(NoteOrRest::Rest(rest)) => {
                        let voice_num = rest.voice;
                        placed
                            .entry(voice_num)
                            .or_default()
                            .push((onset, VoiceElement::Rest(rest)));
                        last_voice = Some((voice_num, cursor));
                    }
                    None => {}
                }
            }
            mxml::MeasureElement::Forward(fwd) => {
                let dur_val = fwd.content.duration.content.0 as i64;
                if dur_val > 0 {
                    let dur = whole(dur_val, divisions);
                    // A `<forward>` of a voice is that voice's hidden rest, and
                    // so is one without a voice that carries on the last
                    // note's voice. Any other only moves the cursor: the next
                    // note's voice starts later (`<backup/><forward/>` before a
                    // second voice that enters mid-bar).
                    let tagged: Option<u8> = fwd
                        .content
                        .voice
                        .as_ref()
                        .and_then(|v| v.content.parse().ok());
                    let voice = tagged.or(match last_voice {
                        Some((v, end)) if end == cursor => Some(v),
                        _ => None,
                    });
                    if let Some(voice_num) = voice {
                        let staff_num: u8 = fwd
                            .content
                            .staff
                            .as_ref()
                            .map(|s| s.content.0 as u8)
                            .unwrap_or(1);
                        let mut rest = Rest::new(Duration::from_divisions(dur_val, divisions, 0));
                        rest.is_spacer = true;
                        rest.voice = voice_num;
                        rest.staff = staff_num;
                        placed
                            .entry(voice_num)
                            .or_default()
                            .push((cursor, VoiceElement::Rest(rest)));
                        last_voice = Some((voice_num, cursor + dur));
                    }
                    cursor += dur;
                }
            }
            mxml::MeasureElement::Backup(bak) => {
                let dur_val = bak.content.duration.content.0 as i64;
                if dur_val > 0 {
                    cursor = (cursor - whole(dur_val, divisions)).max(Frac::from_integer(0));
                }
            }
            mxml::MeasureElement::Direction(dir) => {
                if let Some(mut ir_dir) = convert_direction(dir) {
                    ir_dir.offset =
                        (cursor * Frac::from_integer(4 * divisions)).to_integer() as i32;
                    ir_dir.offset_frac = cursor;
                    measure.directions.push(ir_dir);
                }
            }
            mxml::MeasureElement::Harmony(harm) => {
                if let Some(mut harmony) = convert_harmony_elem(harm) {
                    // At the cursor, displaced by its `<offset>` (divisions).
                    let at = cursor + whole(harmony.offset as i64, divisions);
                    if harmony.offset != 0 {
                        stacked.clear();
                    } else if stacked
                        .last()
                        .is_some_and(|&i| measure.harmonies[i].offset != ir_offset(at))
                    {
                        stacked = vec![measure.harmonies.len()];
                    } else {
                        stacked.push(measure.harmonies.len());
                    }
                    harmony.offset = ir_offset(at);
                    measure.harmonies.push(harmony);
                }
            }
            mxml::MeasureElement::FiguredBass(fb) => {
                let mut figures = convert_figured_bass_elem(fb, divisions);
                figures.offset = ir_offset(cursor);
                measure.figured_bass.push(figures);
            }
            mxml::MeasureElement::Barline(bl) => {
                let barline = convert_barline(bl);
                if barline.location == "left" {
                    measure.left_barline = Some(barline);
                } else {
                    measure.right_barline = Some(barline);
                }
            }
            _ => {} // Sound, Listening, Grouping, Link, Bookmark
        }
    }

    // Lay the voices out by onset. Music that overlaps its own voice (a
    // `<backup>` within one voice) moves to a free voice number, as the
    // LilyPond reader's lanes do.
    let mut tl = Timeline::default();
    let mut voice_nums: Vec<u8> = placed.keys().copied().collect();
    voice_nums.sort();
    for vn in voice_nums {
        let mut elems = placed.remove(&vn).unwrap_or_default();
        // Stable: a grace note keeps its place before its main note.
        elems.sort_by_key(|(on, _)| *on);
        tl.place_voice(vn, &elems);
    }
    measure.voices = voices_from_lanes(tl.lanes);

    Ok((measure, divisions))
}

/// A position in the bar in the IR's harmony and figured-bass offset units
/// ([`OFFSET_DIVISIONS`] per quarter note).
// ponytail: rounds to 16ths (a chord symbol on a triplet moves a little);
// the upgrade path is a `Frac` position on `Harmony`/`FiguredBass` (0.6.0).
fn ir_offset(at: Frac) -> i32 {
    let at = at.max(Frac::from_integer(0)) * Frac::from_integer(4 * OFFSET_DIVISIONS);
    at.round().to_integer() as i32
}

/// A measure's voices from positioned lanes: a gap before or between
/// elements becomes a spacer rest.
fn voices_from_lanes(lanes: BTreeMap<u8, Vec<(Frac, VoiceElement)>>) -> Vec<Voice> {
    lanes
        .into_iter()
        .filter(|(_, elems)| !elems.is_empty())
        .map(|(number, elems)| {
            let mut elements = Vec::with_capacity(elems.len());
            let mut end = Frac::from_integer(0);
            for (on, e) in elems {
                if on > end {
                    elements.push(VoiceElement::spacer(on - end, number, e.staff()));
                }
                end = end.max(on + e.metric_duration());
                elements.push(e);
            }
            Voice { number, elements }
        })
        .collect()
}

/// `later`'s attributes added to `earlier`'s: what `later` sets wins.
fn merge_attributes(earlier: MeasureAttributes, later: MeasureAttributes) -> MeasureAttributes {
    let mut clefs = earlier.clefs;
    clefs.extend(later.clefs);
    MeasureAttributes {
        divisions: later.divisions,
        key: later.key.or(earlier.key),
        time: later.time.or(earlier.time),
        clefs,
        transpose: later.transpose.or(earlier.transpose),
        staves: later.staves.or(earlier.staves),
        staff_lines: later.staff_lines.or(earlier.staff_lines),
    }
}

// ---------------------------------------------------------------------------
// Note helpers
// ---------------------------------------------------------------------------

fn note_is_chord(note: &mxml::Note) -> bool {
    match &note.content.info {
        mxml::NoteType::Normal(info) => info.chord.is_some(),
        mxml::NoteType::Grace(info) => match &info.info {
            mxml::GraceType::Normal(n) => n.chord.is_some(),
            mxml::GraceType::Cue(c) => c.chord.is_some(),
        },
        mxml::NoteType::Cue(info) => info.chord.is_some(),
    }
}

fn note_is_grace(note: &mxml::Note) -> bool {
    matches!(&note.content.info, mxml::NoteType::Grace(_))
}

fn note_duration_divisions(note: &mxml::Note) -> i64 {
    match &note.content.info {
        mxml::NoteType::Normal(info) => info.duration.content.0 as i64,
        mxml::NoteType::Cue(info) => info.duration.content.0 as i64,
        mxml::NoteType::Grace(_) => 0,
    }
}

fn detect_arpeggio(note: &mxml::Note) -> Option<ArpeggioType> {
    for notations in &note.content.notations {
        for notation in &notations.content.notations {
            match notation {
                mxml::NotationContentTypes::Arpeggiate(arp) => {
                    return Some(match arp.attributes.direction {
                        Some(mdt::UpDown::Up) => ArpeggioType::Up,
                        Some(mdt::UpDown::Down) => ArpeggioType::Down,
                        None => ArpeggioType::Up,
                    });
                }
                mxml::NotationContentTypes::NonArpeggiate(_) => {
                    return Some(ArpeggioType::NonArpeggio);
                }
                _ => {}
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Chord merging
// ---------------------------------------------------------------------------

fn merge_chord(
    elements: &mut Vec<(Frac, VoiceElement)>,
    onset: Frac,
    mut note: Note,
    arpeggio: Option<ArpeggioType>,
) {
    // A chord sings on its first note.
    let lyrics = std::mem::take(&mut note.lyrics);
    if let Some(VoiceElement::Note(first)) = elements.last_mut().map(|(_, e)| e) {
        first.lyrics.extend(lyrics);
    } else if let Some(VoiceElement::Chord(c)) = elements.last_mut().map(|(_, e)| e) {
        if let Some(first) = c.notes.first_mut() {
            first.lyrics.extend(lyrics);
        }
    }
    if let Some((_, last)) = elements.last_mut() {
        match last {
            VoiceElement::Chord(chord) => {
                chord.notes.push(note);
                if let Some(arp) = arpeggio {
                    if chord.arpeggio.is_none() {
                        chord.arpeggio = Some(arp);
                    }
                }
            }
            VoiceElement::Note(prev_note) => {
                let prev = std::mem::replace(
                    prev_note,
                    Box::new(Note::new(note.pitch, note.duration.clone())),
                );
                let chord = Chord {
                    duration: prev.duration.clone(),
                    voice: prev.voice,
                    staff: prev.staff,
                    notes: vec![*prev, note],
                    arpeggio,
                };
                *last = VoiceElement::Chord(chord);
            }
            _ => {
                elements.push((onset, VoiceElement::Note(Box::new(note))));
            }
        }
    } else {
        elements.push((onset, VoiceElement::Note(Box::new(note))));
    }
}

// ---------------------------------------------------------------------------
// Attributes conversion
// ---------------------------------------------------------------------------

fn convert_attributes(
    attrs: &mxml::Attributes,
    current_divisions: i64,
) -> (MeasureAttributes, i64) {
    // Clamp instead of truncating through u16: divisions >= 65536 would wrap
    // to 0 and panic in Frac::new (zero denominator) downstream.
    let new_divisions = attrs
        .content
        .divisions
        .as_ref()
        .map(|d| d.content.0 as i64)
        .unwrap_or(current_divisions)
        .clamp(1, u16::MAX as i64);
    let divisions = new_divisions as u16;

    let key = attrs.content.key.first().and_then(|k| {
        match &k.content {
            mxml::KeyContents::Explicit(explicit) => {
                let fifths = explicit.fifths.content.0;
                let mode = explicit
                    .mode
                    .as_ref()
                    .map(|m| convert_mode(&m.content))
                    .unwrap_or(KeyMode::Major);
                Some(KeySignature { fifths, mode })
            }
            mxml::KeyContents::Relative(_) => {
                // Non-traditional key signatures — not yet supported in our IR
                None
            }
        }
    });

    let time = attrs.content.time.first().map(|t| {
        // Collect all beats parts (for compound signatures like "3+2")
        let beats_parts: Vec<String> = t
            .content
            .beats
            .iter()
            .map(|b| b.beats.content.clone())
            .collect();
        let beats = if beats_parts.is_empty() {
            "4".to_string()
        } else {
            beats_parts.join("+")
        };
        let beat_type: u8 = t
            .content
            .beats
            .first()
            .map(|b| b.beat_type.content.parse().unwrap_or(4))
            .unwrap_or(4);
        let symbol = t.attributes.symbol.as_ref().map(|s| {
            use mdt::TimeSymbol;
            match s {
                TimeSymbol::Common => "common".to_string(),
                TimeSymbol::Cut => "cut".to_string(),
                TimeSymbol::SingleNumber => "single-number".to_string(),
                TimeSymbol::Normal => "normal".to_string(),
                TimeSymbol::Note => "note".to_string(),
                TimeSymbol::DottedNote => "dotted-note".to_string(),
            }
        });
        TimeSignature {
            beats,
            beat_type,
            symbol,
        }
    });

    let mut clefs: HashMap<u8, Clef> = HashMap::new();
    for clef_elem in &attrs.content.clef {
        let staff_num: u8 = clef_elem
            .attributes
            .number
            .as_ref()
            .map(|n| n.0)
            .unwrap_or(1);
        let sign = convert_clef_sign(&clef_elem.content.sign.content);
        let line = clef_elem
            .content
            .line
            .as_ref()
            .map(|l| l.content.0 as u8)
            .unwrap_or(2);
        let octave_change = clef_elem
            .content
            .clef_octave_change
            .as_ref()
            .map(|o| o.content)
            .unwrap_or(0);
        clefs.insert(
            staff_num,
            Clef {
                sign,
                line,
                octave_change,
            },
        );
    }

    let transpose = attrs.content.transpose.first().map(|t| {
        let diatonic = t
            .content
            .diatonic
            .as_ref()
            .map(|d| d.content as i8)
            .unwrap_or(0);
        let chromatic = t.content.chromatic.content.0 as i8;
        let octave_change = t
            .content
            .octave_change
            .as_ref()
            .map(|o| o.content)
            .unwrap_or(0);
        Transpose {
            diatonic,
            chromatic,
            octave_change,
        }
    });

    let staves = attrs.content.staves.as_ref().map(|s| s.content.0 as u8);

    let staff_lines = attrs
        .content
        .staff_details
        .first()
        .and_then(|sd| sd.content.staff_lines.as_ref())
        .map(|sl| sl.content.0 as u8);

    (
        MeasureAttributes {
            divisions,
            key,
            time,
            clefs,
            transpose,
            staves,
            staff_lines,
        },
        new_divisions,
    )
}

// ---------------------------------------------------------------------------
// Barline conversion
// ---------------------------------------------------------------------------

fn convert_barline(bl: &mxml::Barline) -> Barline {
    let style = bl
        .content
        .bar_style
        .as_ref()
        .map(|bs| convert_bar_style(&bs.content))
        .unwrap_or(BarlineType::Regular);

    let repeat_direction = bl
        .content
        .repeat
        .as_ref()
        .map(|r| match r.attributes.direction {
            mdt::BackwardForward::Forward => RepeatDirection::Forward,
            mdt::BackwardForward::Backward => RepeatDirection::Backward,
        });
    let repeat_times = bl
        .content
        .repeat
        .as_ref()
        .and_then(|r| r.attributes.times.as_ref())
        .map(|t| t.0 as u8);

    let ending_number = bl
        .content
        .ending
        .as_ref()
        .and_then(|e| e.attributes.number.0.parse::<u8>().ok());
    let ending_type = bl.content.ending.as_ref().map(|e| {
        use mdt::StartStopDiscontinue;
        match e.attributes.r#type {
            StartStopDiscontinue::Start => "start".to_string(),
            StartStopDiscontinue::Stop => "stop".to_string(),
            StartStopDiscontinue::Discontinue => "discontinue".to_string(),
        }
    });

    let location = bl
        .attributes
        .location
        .as_ref()
        .map(|l| {
            use mdt::RightLeftMiddle;
            match l {
                RightLeftMiddle::Right => "right",
                RightLeftMiddle::Left => "left",
                RightLeftMiddle::Middle => "middle",
            }
        })
        .unwrap_or("right")
        .to_string();

    Barline {
        style,
        location,
        repeat_direction,
        ending_number,
        ending_type,
        repeat_times,
    }
}

fn convert_bar_style(bs: &mdt::BarStyle) -> BarlineType {
    match bs {
        mdt::BarStyle::Regular => BarlineType::Regular,
        mdt::BarStyle::LightLight => BarlineType::Double,
        mdt::BarStyle::LightHeavy => BarlineType::Final,
        mdt::BarStyle::Dashed => BarlineType::Dashed,
        mdt::BarStyle::Dotted => BarlineType::Dotted,
        mdt::BarStyle::Tick => BarlineType::Tick,
        mdt::BarStyle::Short => BarlineType::Short,
        mdt::BarStyle::None => BarlineType::None,
        mdt::BarStyle::Heavy | mdt::BarStyle::HeavyHeavy | mdt::BarStyle::HeavyLight => {
            BarlineType::Final
        }
    }
}

// ---------------------------------------------------------------------------
// Shared type conversions
// ---------------------------------------------------------------------------

pub(super) fn convert_mode(mode: &mdt::Mode) -> KeyMode {
    match mode {
        mdt::Mode::Major => KeyMode::Major,
        mdt::Mode::Minor => KeyMode::Minor,
        mdt::Mode::Dorian => KeyMode::Dorian,
        mdt::Mode::Phrygian => KeyMode::Phrygian,
        mdt::Mode::Lydian => KeyMode::Lydian,
        mdt::Mode::Mixolydian => KeyMode::Mixolydian,
        mdt::Mode::Aeolian => KeyMode::Aeolian,
        mdt::Mode::Ionian => KeyMode::Ionian,
        mdt::Mode::Locrian => KeyMode::Locrian,
        mdt::Mode::None => KeyMode::Major,
    }
}

pub(super) fn convert_clef_sign(sign: &mdt::ClefSign) -> ClefSign {
    match sign {
        mdt::ClefSign::G => ClefSign::G,
        mdt::ClefSign::F => ClefSign::F,
        mdt::ClefSign::C => ClefSign::C,
        mdt::ClefSign::Percussion => ClefSign::Percussion,
        mdt::ClefSign::TAB => ClefSign::Tab,
        mdt::ClefSign::Jianpu | mdt::ClefSign::None => ClefSign::G,
    }
}
