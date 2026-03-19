//! Part-level and measure-level parsing, attributes, barlines, chord merging.

use std::collections::HashMap;

use crate::ir::direction::{Barline, BarlineType, Direction, RepeatDirection};
use crate::ir::duration::Duration;
use crate::ir::measure::*;
use crate::ir::note::*;
use crate::ir::part::Part;
use crate::ir::voice::Voice;

use super::direction::{parse_direction, parse_figured_bass_elem, parse_harmony_elem};
use super::helpers::XmlNode;
use super::note::{parse_note, NoteOrRest};
use super::PartInfo;
use super::Result;

// ---------------------------------------------------------------------------
// Part & measure parsing
// ---------------------------------------------------------------------------

pub(super) fn parse_part(elem: &XmlNode, info: &PartInfo) -> Result<Part> {
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

    for measure_elem in elem.find_all("measure") {
        let (measure, new_divisions) = parse_measure(measure_elem, divisions)?;
        divisions = new_divisions;
        // Update staves count from attributes.
        if let Some(ref attrs) = measure.attributes {
            if let Some(s) = attrs.staves {
                part.staves = s;
            }
        }
        part.measures.push(measure);
    }

    Ok(part)
}

fn parse_measure(elem: &XmlNode, mut divisions: i64) -> Result<(Measure, i64)> {
    let number: u32 = elem
        .attr("number")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let implicit = elem.attr("implicit") == Some("yes");
    let width: Option<f32> = elem.attr("width").and_then(|s| s.parse().ok());

    let mut measure = Measure {
        number,
        implicit,
        width,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: Vec::new(),
        harmonies: Vec::new(),
        figured_bass: Vec::new(),
        print_object: true,
        multi_measure_rest: None,
        voices: Vec::new(),
    };

    let mut voice_elements: HashMap<u8, Vec<VoiceElement>> = HashMap::new();
    let mut pending_arpeggio: HashMap<u8, ArpeggioType> = HashMap::new();
    // Running forward position in divisions — used to attach directions to the
    // correct voice element based on document order.
    let mut forward_position: i64 = 0;

    for child in &elem.children {
        match child.tag.as_str() {
            "attributes" => {
                let (attrs, new_div) = parse_attributes(child, divisions);
                divisions = new_div;
                measure.attributes = Some(attrs);
                // <measure-style> lives inside <attributes>
                if let Some(ms) = child.find("measure-style") {
                    if let Some(mr) = ms.find("multiple-rest") {
                        measure.multi_measure_rest = Some(mr.text_i64(1) as u16);
                    }
                }
            }
            "print" => {
                if child.attr("new-page") == Some("yes") {
                    measure.directions.push(Direction {
                        layout_break: Some(crate::ir::direction::LayoutBreakType::Page),
                        ..Default::default()
                    });
                } else if child.attr("new-system") == Some("yes") {
                    measure.directions.push(Direction {
                        layout_break: Some(crate::ir::direction::LayoutBreakType::System),
                        ..Default::default()
                    });
                }
            }
            "note" => {
                let is_chord = child.find("chord").is_some();
                let is_grace = child.find("grace").is_some();
                // Detect arpeggio from notations (applies to chord)
                let arpeggio = child.find("notations").and_then(|n| {
                    if let Some(arp) = n.find("arpeggiate") {
                        Some(match arp.attr("direction").unwrap_or("") {
                            "up" => ArpeggioType::Up,
                            "down" => ArpeggioType::Down,
                            _ => ArpeggioType::Up,
                        })
                    } else if n.find("non-arpeggiate").is_some() {
                        Some(ArpeggioType::NonArpeggio)
                    } else {
                        None
                    }
                });
                let result = parse_note(child, divisions);
                // Advance forward position for non-chord, non-grace notes
                let dur_val = child.child_i64("duration", 0);
                if !is_chord && !is_grace && dur_val > 0 {
                    forward_position += dur_val;
                }
                match result {
                    Some(NoteOrRest::Note(note)) => {
                        let voice_num = note.voice;
                        let elements = voice_elements.entry(voice_num).or_default();
                        if is_chord {
                            // Get pending arpeggio from the first note of this chord
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
            "forward" => {
                let dur_val = child.child_i64("duration", 0);
                if dur_val > 0 {
                    forward_position += dur_val;
                    let dots = child.find_all("dot").len() as u8;
                    let duration = Duration::from_divisions(dur_val, divisions, dots);
                    let voice_num = child.child_i64("voice", 1) as u8;
                    let staff_num = child.child_i64("staff", 1) as u8;
                    // Convert Forward to spacer rest
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
            "backup" => {
                let dur_val = child.child_i64("duration", 0);
                if dur_val > 0 {
                    forward_position -= dur_val;
                    // Backup is a MusicXML time-positioning concept. The voice
                    // separation logic already handles positioning, so we
                    // simply adjust forward_position and drop the element.
                }
            }
            "direction" => {
                if let Some(mut dir) = parse_direction(child) {
                    // Store the current forward position so ir_to_ly can
                    // attach this direction to the correct voice element.
                    dir.offset = forward_position as i32;
                    measure.directions.push(dir);
                }
            }
            "harmony" => {
                if let Some(harmony) = parse_harmony_elem(child) {
                    measure.harmonies.push(harmony);
                }
            }
            "figured-bass" => {
                measure
                    .figured_bass
                    .push(parse_figured_bass_elem(child, divisions));
            }
            "barline" => {
                let barline = parse_barline(child);
                if barline.location == "left" {
                    measure.left_barline = Some(barline);
                } else {
                    measure.right_barline = Some(barline);
                }
            }
            _ => {}
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
// Chord merging
// ---------------------------------------------------------------------------

/// When a `<chord/>` flag is present, merge the note into the previous note
/// or chord in the voice element list. If `arpeggio` is provided, it is
/// applied when a new Chord is formed from Note→Chord conversion.
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
                // Convert the previous Note into a Chord.
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
                // If previous element isn't a note/chord, just append as note.
                elements.push(VoiceElement::Note(Box::new(note)));
            }
        }
    } else {
        // Empty list — just push as a note.
        elements.push(VoiceElement::Note(Box::new(note)));
    }
}

// ---------------------------------------------------------------------------
// Attributes parsing
// ---------------------------------------------------------------------------

fn parse_attributes(elem: &XmlNode, current_divisions: i64) -> (MeasureAttributes, i64) {
    let divisions = elem
        .find("divisions")
        .map(|d| d.text_i64(current_divisions) as u16)
        .unwrap_or(current_divisions as u16);
    let new_divisions = divisions as i64;

    let key = elem.find("key").map(|k| {
        let fifths = k.child_i64("fifths", 0) as i8;
        let mode_str = k.child_text("mode").unwrap_or("major");
        KeySignature {
            fifths,
            mode: KeyMode::from_str_loose(mode_str),
        }
    });

    let time = elem.find("time").map(|t| {
        let beats_parts: Vec<&str> = t
            .find_all("beats")
            .iter()
            .map(|b| b.text_content())
            .collect();
        let beats = if beats_parts.is_empty() {
            "4".to_string()
        } else {
            beats_parts.join("+")
        };
        let beat_type = t.child_i64("beat-type", 4) as u8;
        let symbol = t.attr("symbol").map(|s| s.to_string());
        TimeSignature {
            beats,
            beat_type,
            symbol,
        }
    });

    let mut clefs: HashMap<u8, Clef> = HashMap::new();
    for clef_elem in elem.find_all("clef") {
        let staff_num: u8 = clef_elem
            .attr("number")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let sign_str = clef_elem.child_text("sign").unwrap_or("G");
        let sign = ClefSign::from_str_loose(sign_str);
        let line = clef_elem.child_i64("line", 2) as u8;
        let octave_change = clef_elem
            .find("clef-octave-change")
            .map(|o| o.text_i64(0) as i8)
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

    let transpose = elem.find("transpose").map(|t| {
        let diatonic = t.child_i64("diatonic", 0) as i8;
        let chromatic = t.child_i64("chromatic", 0) as i8;
        let octave_change = t.child_i64("octave-change", 0) as i8;
        Transpose {
            diatonic,
            chromatic,
            octave_change,
        }
    });

    let staves = elem.find("staves").map(|s| s.text_i64(1) as u8);

    let staff_lines = elem
        .find("staff-details")
        .and_then(|sd| sd.find("staff-lines"))
        .map(|sl| sl.text_i64(5) as u8);

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
// Barline parsing
// ---------------------------------------------------------------------------

fn parse_barline(elem: &XmlNode) -> Barline {
    let bar_style = elem.child_text("bar-style").unwrap_or("regular");
    let style = match bar_style {
        "regular" => BarlineType::Regular,
        "light-light" | "double" => BarlineType::Double,
        "light-heavy" | "final" => BarlineType::Final,
        "dashed" => BarlineType::Dashed,
        "dotted" => BarlineType::Dotted,
        "tick" => BarlineType::Tick,
        "short" => BarlineType::Short,
        "none" => BarlineType::None,
        _ => BarlineType::Regular,
    };

    let repeat_direction =
        elem.find("repeat")
            .and_then(|r| match r.attr("direction").unwrap_or("") {
                "forward" => Some(RepeatDirection::Forward),
                "backward" => Some(RepeatDirection::Backward),
                _ => None,
            });

    let ending_number = elem
        .find("ending")
        .and_then(|e| e.attr("number").and_then(|s| s.parse::<u8>().ok()));
    let ending_type = elem
        .find("ending")
        .and_then(|e| e.attr("type").map(|s| s.to_string()));

    let location = elem.attr("location").unwrap_or("right").to_string();

    Barline {
        style,
        location,
        repeat_direction,
        ending_number,
        ending_type,
    }
}
