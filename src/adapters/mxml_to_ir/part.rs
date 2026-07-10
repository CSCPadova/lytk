//! Part-level and measure-level conversion from musicxml crate types to IR.

use std::collections::HashMap;

use crate::ir::direction::{Barline, BarlineType, Direction, RepeatDirection};
use crate::ir::duration::Duration;
use crate::ir::measure::*;
use crate::ir::note::*;
use crate::ir::part::Part;
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

    let mut divisions: i64 = 1;

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

    Ok(part)
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

    let mut voice_elements: HashMap<u8, Vec<VoiceElement>> = HashMap::new();
    let mut pending_arpeggio: HashMap<u8, ArpeggioType> = HashMap::new();
    let mut forward_position: i64 = 0;

    for elem in &mxml_measure.content {
        match elem {
            mxml::MeasureElement::Attributes(attrs) => {
                let (ir_attrs, new_div) = convert_attributes(attrs, divisions);
                divisions = new_div;
                measure.attributes = Some(ir_attrs);
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

                // Advance forward position for non-chord, non-grace notes
                let dur_val = note_duration_divisions(mxml_note);
                if !is_chord && !is_grace && dur_val > 0 {
                    forward_position += dur_val;
                }

                match result {
                    Some(NoteOrRest::Note(note)) => {
                        let voice_num = note.voice;
                        let elements = voice_elements.entry(voice_num).or_default();
                        if is_chord {
                            let pending = pending_arpeggio.remove(&voice_num);
                            let arp = arpeggio.or(pending);
                            merge_chord(elements, *note, arp);
                        } else {
                            elements.push(VoiceElement::Note(note));
                            if let Some(arp) = arpeggio {
                                pending_arpeggio.insert(voice_num, arp);
                            }
                        }
                    }
                    Some(NoteOrRest::Rest(rest)) => {
                        let voice_num = rest.voice;
                        let elements = voice_elements.entry(voice_num).or_default();
                        elements.push(VoiceElement::Rest(rest));
                    }
                    None => {}
                }
            }
            mxml::MeasureElement::Forward(fwd) => {
                let dur_val = fwd.content.duration.content.0 as i64;
                if dur_val > 0 {
                    forward_position += dur_val;
                    let duration = Duration::from_divisions(dur_val, divisions, 0);
                    let voice_num: u8 = fwd
                        .content
                        .voice
                        .as_ref()
                        .and_then(|v| v.content.parse().ok())
                        .unwrap_or(1);
                    let staff_num: u8 = fwd
                        .content
                        .staff
                        .as_ref()
                        .map(|s| s.content.0 as u8)
                        .unwrap_or(1);
                    let mut rest = Rest::new(duration);
                    rest.is_spacer = true;
                    rest.voice = voice_num;
                    rest.staff = staff_num;
                    voice_elements
                        .entry(voice_num)
                        .or_default()
                        .push(VoiceElement::Rest(rest));
                }
            }
            mxml::MeasureElement::Backup(bak) => {
                let dur_val = bak.content.duration.content.0 as i64;
                if dur_val > 0 {
                    forward_position -= dur_val;
                }
            }
            mxml::MeasureElement::Direction(dir) => {
                if let Some(mut ir_dir) = convert_direction(dir) {
                    ir_dir.offset = forward_position as i32;
                    // The exporters position directions via offset_frac
                    // (whole notes); without it a mid-measure direction is
                    // re-emitted at the start of the measure.
                    if forward_position > 0 && divisions > 0 {
                        ir_dir.offset_frac =
                            crate::ir::duration::Frac::new(forward_position, 4 * divisions);
                    }
                    measure.directions.push(ir_dir);
                }
            }
            mxml::MeasureElement::Harmony(harm) => {
                if let Some(harmony) = convert_harmony_elem(harm) {
                    measure.harmonies.push(harmony);
                }
            }
            mxml::MeasureElement::FiguredBass(fb) => {
                measure
                    .figured_bass
                    .push(convert_figured_bass_elem(fb, divisions));
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

    // Build voice nodes from collected elements.
    let mut voice_nums: Vec<u8> = voice_elements.keys().copied().collect();
    voice_nums.sort();
    for vn in voice_nums {
        if let Some(elements) = voice_elements.remove(&vn) {
            if !elements.is_empty() {
                measure.voices.push(Voice {
                    number: vn,
                    elements,
                });
            }
        }
    }

    Ok((measure, divisions))
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

fn merge_chord(elements: &mut Vec<VoiceElement>, note: Note, arpeggio: Option<ArpeggioType>) {
    if let Some(last) = elements.last_mut() {
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
                elements.push(VoiceElement::Note(Box::new(note)));
            }
        }
    } else {
        elements.push(VoiceElement::Note(Box::new(note)));
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
