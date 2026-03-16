//! MusicXML → IR adapter.
//!
//! Parses MusicXML (both `.xml` and `.mxl`) into the lytk IR tree.
//!
//! # Reference
//! Ported from `lytk-py/converters/mxml_to_ir.py`.

use std::collections::HashMap;
use std::path::Path;

use num::rational::Ratio;
use quick_xml::events::Event;
use quick_xml::Reader;

use super::mxl_zip;
use super::{AdapterError, Result, ToIrAdapter};
use crate::ir::articulation::*;
use crate::ir::direction::*;
use crate::ir::duration::Duration;
use crate::ir::harmony::{ChordDegree, ChordPitch, FiguredBass, Figure, Harmony};
use crate::ir::measure::*;
use crate::ir::note::*;
use crate::ir::part::Part;
use crate::ir::pitch::{AccidentalDisplay, Alter, Pitch, PitchStep};
use crate::ir::score::*;
use crate::ir::voice::Voice;

// ---------------------------------------------------------------------------
// Adapter struct
// ---------------------------------------------------------------------------

/// Adapter that reads MusicXML and produces an IR [`Score`].
pub struct MxmlToIrAdapter;

impl MxmlToIrAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MxmlToIrAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ToIrAdapter for MxmlToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score> {
        let xml = mxl_zip::read_musicxml(path)?;
        self.convert_str(&xml)
    }

    fn convert_str(&self, text: &str) -> Result<Score> {
        parse_score_partwise(text)
    }
}

// ---------------------------------------------------------------------------
// XML helper: a minimal DOM-like tree built from quick-xml events
// ---------------------------------------------------------------------------

/// A simple in-memory XML element for easy traversal.
/// We build this from quick-xml events to avoid repeated streaming.
#[derive(Debug, Clone)]
struct XmlNode {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

impl XmlNode {
    fn new(tag: String) -> Self {
        Self {
            tag,
            attrs: Vec::new(),
            children: Vec::new(),
            text: String::new(),
        }
    }

    /// Get attribute value by name.
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Find first child element by tag name.
    fn find(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// Find all child elements by tag name.
    fn find_all(&self, tag: &str) -> Vec<&XmlNode> {
        self.children.iter().filter(|c| c.tag == tag).collect()
    }

    /// Get text content, trimmed.
    fn text_content(&self) -> &str {
        self.text.trim()
    }

    /// Parse text content as i64, with a default.
    fn text_i64(&self, default: i64) -> i64 {
        self.text_content().parse().unwrap_or(default)
    }

    /// Find a child and get its text as i64.
    fn child_i64(&self, tag: &str, default: i64) -> i64 {
        self.find(tag)
            .map(|n| n.text_i64(default))
            .unwrap_or(default)
    }

    /// Find a child and get its text content.
    fn child_text(&self, tag: &str) -> Option<&str> {
        self.find(tag).map(|n| {
            let t = n.text_content();
            if t.is_empty() {
                return "";
            }
            t
        })
    }
}

/// Parse XML string into a tree of XmlNodes.
fn parse_xml(xml: &str) -> Result<XmlNode> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<XmlNode> = vec![XmlNode::new("__root__".into())];
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut node = XmlNode::new(tag);
                for attr in e.attributes() {
                    let attr = attr?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let val = String::from_utf8_lossy(&attr.value).into_owned();
                    node.attrs.push((key, val));
                }
                stack.push(node);
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().unwrap();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    return Ok(node);
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut node = XmlNode::new(tag);
                for attr in e.attributes() {
                    let attr = attr?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let val = String::from_utf8_lossy(&attr.value).into_owned();
                    node.attrs.push((key, val));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e
                    .unescape()
                    .map_err(|err| AdapterError::Parse(err.to_string()))?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {} // comments, PI, CDATA, etc.
            Err(e) => return Err(AdapterError::Xml(e)),
        }
        buf.clear();
    }

    // Return the root's first real child (skip __root__ wrapper).
    let root = stack.pop().unwrap();
    root.children
        .into_iter()
        .next()
        .ok_or_else(|| AdapterError::MissingElement("root element".into()))
}

// ---------------------------------------------------------------------------
// Score-level parsing
// ---------------------------------------------------------------------------

fn parse_score_partwise(xml: &str) -> Result<Score> {
    let root = parse_xml(xml)?;
    if root.tag != "score-partwise" {
        return Err(AdapterError::MissingElement("score-partwise".into()));
    }

    let metadata = parse_metadata(&root);
    let page_layout = parse_defaults(&root);
    let mut score = Score {
        metadata,
        page_layout,
        children: Vec::new(),
    };

    // Parse part-list for part info and part groups.
    let mut part_info: HashMap<String, PartInfo> = HashMap::new();
    let mut part_order: Vec<String> = Vec::new();
    let mut completed_groups: Vec<(PartGroup, Vec<String>)> = Vec::new();
    let mut active_groups: Vec<(PartGroup, Vec<String>)> = Vec::new();

    if let Some(part_list) = root.find("part-list") {
        for child in &part_list.children {
            match child.tag.as_str() {
                "score-part" => {
                    let info = parse_score_part(child);
                    let id = info.id.clone();
                    part_info.insert(id.clone(), info);
                    part_order.push(id.clone());
                    for (_, parts) in &mut active_groups {
                        parts.push(id.clone());
                    }
                }
                "part-group" => {
                    let group_type = child.attr("type").unwrap_or("");
                    if group_type == "start" {
                        let group = parse_part_group(child);
                        active_groups.push((group, Vec::new()));
                    } else if group_type == "stop" {
                        if let Some(completed) = active_groups.pop() {
                            completed_groups.push(completed);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Parse each <part> element.
    let mut parsed_parts: HashMap<String, Part> = HashMap::new();
    for part_elem in root.find_all("part") {
        let id = part_elem.attr("id").unwrap_or("").to_string();
        let info = if id.is_empty() {
            // No id — if there's exactly one score-part, use that.
            if part_info.len() == 1 {
                part_info.values().next().cloned().unwrap_or_default()
            } else {
                PartInfo::default()
            }
        } else {
            part_info.get(&id).cloned().unwrap_or_default()
        };
        let resolved_id = if id.is_empty() { info.id.clone() } else { id };
        let part = parse_part(part_elem, &info)?;
        parsed_parts.insert(resolved_id, part);
    }

    // Build the score tree: group parts into PartGroups where declared.
    let grouped_ids: std::collections::HashSet<String> = completed_groups
        .iter()
        .flat_map(|(_, ids)| ids.iter().cloned())
        .collect();

    for (group, member_ids) in &completed_groups {
        let mut pg = group.clone();
        for id in member_ids {
            if let Some(part) = parsed_parts.remove(id) {
                pg.children.push(ScoreChild::Part(part));
            }
        }
        if !pg.children.is_empty() {
            score.children.push(ScoreChild::PartGroup(pg));
        }
    }

    // Add ungrouped parts in original order.
    for id in &part_order {
        if !grouped_ids.contains(id) {
            if let Some(part) = parsed_parts.remove(id) {
                score.children.push(ScoreChild::Part(part));
            }
        }
    }

    // Detect anacrusis: check if the first measure is implicit (pickup)
    detect_anacrusis(&mut score);

    Ok(score)
}

/// Detect anacrusis (pickup measure) and set `score.metadata.partial_duration`.
///
/// A measure is an anacrusis if it has `implicit="yes"`. We compute the
/// pickup duration as the sum of actual note/rest durations in the first
/// voice of the first part's first measure.
fn detect_anacrusis(score: &mut Score) {
    let first_measure = score
        .parts()
        .first()
        .and_then(|p| p.measures.first());

    if let Some(measure) = first_measure {
        if !measure.implicit {
            return;
        }
        // Sum the durations in the first voice
        if let Some(voice) = measure.voices.first() {
            let mut total = Ratio::new(0i64, 1);
            for elem in &voice.elements {
                let dur = match elem {
                    VoiceElement::Note(n) => &n.duration,
                    VoiceElement::Rest(r) => &r.duration,
                    VoiceElement::Chord(c) => &c.duration,
                    VoiceElement::Forward(f) => &f.duration,
                    VoiceElement::Backup(_) => continue,
                };
                total += dur.actual_duration();
            }
            if total > Ratio::new(0i64, 1) {
                // Find the score as mutable to set partial_duration.
                // We need to find the first part mutably.
                let partial = Duration::new(total);
                // Since we have &mut Score, set it directly.
                score.metadata.partial_duration = Some(partial);
            }
        }
    }
}

fn parse_metadata(root: &XmlNode) -> ScoreMetadata {
    let mut meta = ScoreMetadata::default();

    if let Some(work) = root.find("work") {
        meta.title = work.child_text("work-title").map(|s| s.to_string());
    }
    if meta.title.is_none() {
        meta.title = root.child_text("movement-title").map(|s| s.to_string());
    }

    if let Some(ident) = root.find("identification") {
        for creator in ident.find_all("creator") {
            let creator_type = creator.attr("type").unwrap_or("");
            let text = creator.text_content().to_string();
            match creator_type {
                "composer" => meta.composer = Some(text),
                "arranger" => meta.arranger = Some(text),
                "lyricist" => meta.lyricist = Some(text),
                _ => {
                    meta.extra.insert(creator_type.to_string(), text);
                }
            }
        }
        for rights in ident.find_all("rights") {
            let rights_type = rights.attr("type").unwrap_or("").to_string();
            meta.rights
                .push((rights_type, rights.text_content().to_string()));
        }
    }

    // Fallback: <credit> elements (MuseScore exports title/composer here)
    for credit in root.find_all("credit") {
        let credit_type = credit
            .child_text("credit-type")
            .unwrap_or("")
            .to_lowercase();
        let words = credit
            .child_text("credit-words")
            .unwrap_or("")
            .to_string();
        if words.is_empty() {
            continue;
        }
        match credit_type.as_str() {
            "title" => {
                if meta.title.is_none() {
                    meta.title = Some(words);
                }
            }
            "subtitle" => {
                if meta.subtitle.is_none() {
                    meta.subtitle = Some(words);
                }
            }
            "composer" => {
                if meta.composer.is_none() {
                    meta.composer = Some(words);
                }
            }
            "arranger" => {
                if meta.arranger.is_none() {
                    meta.arranger = Some(words);
                }
            }
            "lyricist" | "poet" => {
                if meta.lyricist.is_none() {
                    meta.lyricist = Some(words);
                }
            }
            _ => {}
        }
    }

    meta
}

// ---------------------------------------------------------------------------
// Defaults / page layout parsing
// ---------------------------------------------------------------------------

/// Parse `<defaults>` element for page layout and staff sizing.
///
/// MusicXML dimensions are in "tenths" (1/10 of a staff space). The
/// `<scaling>` element provides the ratio: millimeters / tenths. We convert
/// to cm and points for the IR.
fn parse_defaults(root: &XmlNode) -> Option<PageLayout> {
    let defaults = root.find("defaults")?;

    // Scaling factor: millimeters per tenth
    let (mm, tenths) = if let Some(scaling) = defaults.find("scaling") {
        let mm_val: f64 = scaling
            .child_text("millimeters")
            .and_then(|s| s.parse().ok())
            .unwrap_or(7.056);
        let tenths_val: f64 = scaling
            .child_text("tenths")
            .and_then(|s| s.parse().ok())
            .unwrap_or(40.0);
        (mm_val, tenths_val)
    } else {
        return None;
    };

    let scale = mm / tenths; // mm per tenth
    let to_cm = |val: f64| val * scale / 10.0;

    // Staff size: 40 tenths (one staff height) converted to points
    let staff_size = 40.0 * scale * 72.27 / 25.4;

    let mut layout = PageLayout {
        page_height: None,
        page_width: None,
        left_margin: None,
        right_margin: None,
        top_margin: None,
        bottom_margin: None,
        system_distance: None,
        top_system_distance: None,
        staff_size: Some(staff_size),
    };

    if let Some(pl) = defaults.find("page-layout") {
        layout.page_height = pl
            .child_text("page-height")
            .and_then(|s| s.parse::<f64>().ok())
            .map(&to_cm);
        layout.page_width = pl
            .child_text("page-width")
            .and_then(|s| s.parse::<f64>().ok())
            .map(&to_cm);

        // Page margins (use first <page-margins> element)
        if let Some(margins) = pl.find("page-margins") {
            layout.left_margin = margins
                .child_text("left-margin")
                .and_then(|s| s.parse::<f64>().ok())
                .map(&to_cm);
            layout.right_margin = margins
                .child_text("right-margin")
                .and_then(|s| s.parse::<f64>().ok())
                .map(&to_cm);
            layout.top_margin = margins
                .child_text("top-margin")
                .and_then(|s| s.parse::<f64>().ok())
                .map(&to_cm);
            layout.bottom_margin = margins
                .child_text("bottom-margin")
                .and_then(|s| s.parse::<f64>().ok())
                .map(&to_cm);
        }
    }

    if let Some(sl) = defaults.find("system-layout") {
        layout.system_distance = sl
            .child_text("system-distance")
            .and_then(|s| s.parse::<f64>().ok())
            .map(&to_cm);
        layout.top_system_distance = sl
            .child_text("top-system-distance")
            .and_then(|s| s.parse::<f64>().ok())
            .map(&to_cm);
    }

    Some(layout)
}

// ---------------------------------------------------------------------------
// Harmony / chord symbol parsing
// ---------------------------------------------------------------------------

fn parse_harmony_elem(elem: &XmlNode) -> Option<Harmony> {
    let root_elem = elem.find("root")?;
    let root_step = root_elem.child_text("root-step")?.to_string();
    let root_alter: f64 = root_elem
        .child_text("root-alter")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let kind = elem
        .child_text("kind")
        .unwrap_or("major")
        .to_string();

    let bass = elem.find("bass").and_then(|b| {
        let step = b.child_text("bass-step")?.to_string();
        let alter: f64 = b
            .child_text("bass-alter")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        Some(ChordPitch { step, alter })
    });

    let degrees: Vec<ChordDegree> = elem
        .find_all("degree")
        .iter()
        .filter_map(|d| {
            let value: u8 = d.child_text("degree-value")?.parse().ok()?;
            let alter: f64 = d
                .child_text("degree-alter")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let degree_type = d
                .child_text("degree-type")
                .unwrap_or("add")
                .to_string();
            Some(ChordDegree {
                value,
                alter,
                degree_type,
            })
        })
        .collect();

    let offset: i32 = elem
        .child_text("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Some(Harmony {
        root: ChordPitch {
            step: root_step,
            alter: root_alter,
        },
        kind,
        bass,
        degrees,
        offset,
    })
}

// ---------------------------------------------------------------------------
// Figured bass parsing
// ---------------------------------------------------------------------------

fn parse_figured_bass_elem(elem: &XmlNode, divisions: i64) -> FiguredBass {
    let figures: Vec<Figure> = elem
        .find_all("figure")
        .iter()
        .map(|f| {
            let number: Option<u8> = f
                .child_text("figure-number")
                .and_then(|s| s.parse().ok());
            let prefix = f.child_text("prefix").map(|s| s.to_string());
            let suffix = f.child_text("suffix").map(|s| s.to_string());
            Figure {
                number,
                prefix,
                suffix,
            }
        })
        .collect();

    let dur_val = elem.child_i64("duration", 0);
    let duration = if dur_val > 0 {
        Duration::from_divisions(dur_val, divisions, 0)
    } else {
        Duration::default()
    };
    let parentheses = elem.attr("parentheses") == Some("yes");
    let offset: i32 = elem
        .child_text("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    FiguredBass {
        figures,
        duration,
        parentheses,
        offset,
    }
}

// ---------------------------------------------------------------------------
// Part-list parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct PartInfo {
    id: String,
    name: String,
    abbreviation: String,
    midi_instrument: String,
    midi_channel: u8,
    midi_program: u8,
}

fn parse_score_part(elem: &XmlNode) -> PartInfo {
    let mut info = PartInfo {
        id: elem.attr("id").unwrap_or("").to_string(),
        ..Default::default()
    };

    if let Some(name) = elem.child_text("part-name") {
        info.name = name.to_string();
    }
    if let Some(abbrev) = elem.child_text("part-abbreviation") {
        info.abbreviation = abbrev.to_string();
    }

    // MIDI instrument info (first <midi-instrument> child).
    if let Some(midi) = elem.find("midi-instrument") {
        if let Some(ch) = midi.find("midi-channel") {
            info.midi_channel = ch.text_i64(1) as u8;
        }
        if let Some(prog) = midi.find("midi-program") {
            info.midi_program = prog.text_i64(1) as u8;
        }
        if let Some(name) = midi.child_text("midi-name") {
            info.midi_instrument = name.to_string();
        }
    }

    info
}

fn parse_part_group(elem: &XmlNode) -> PartGroup {
    let mut group = PartGroup::new("StaffGroup");

    if let Some(name) = elem.child_text("group-name") {
        group.name = name.to_string();
    }
    if let Some(sym) = elem.child_text("group-symbol") {
        group.bracket = sym.to_string();
        // Map bracket style to LilyPond context type.
        group.group_type = match sym {
            "brace" => "PianoStaff",
            "bracket" => "StaffGroup",
            "line" => "ChoirStaff",
            _ => "StaffGroup",
        }
        .to_string();
    }
    if let Some(num) = elem.attr("number") {
        group.number = num.parse().unwrap_or(1);
    }

    group
}

// ---------------------------------------------------------------------------
// Part & measure parsing
// ---------------------------------------------------------------------------

fn parse_part(elem: &XmlNode, info: &PartInfo) -> Result<Part> {
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
            }
            "note" => {
                let is_chord = child.find("chord").is_some();
                let is_grace = child.find("grace").is_some();
                // Detect arpeggio from notations (applies to chord)
                let arpeggio = child
                    .find("notations")
                    .and_then(|n| {
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
                    let fwd = Forward {
                        duration,
                        voice: voice_num,
                        staff: staff_num,
                    };
                    voice_elements
                        .entry(voice_num)
                        .or_default()
                        .push(VoiceElement::Forward(fwd));
                }
            }
            "backup" => {
                let dur_val = child.child_i64("duration", 0);
                if dur_val > 0 {
                    forward_position -= dur_val as i64;
                    let dots = child.find_all("dot").len() as u8;
                    let duration = Duration::from_divisions(dur_val, divisions, dots);
                    let backup = Backup::new(duration);
                    // Backup is not assigned to a voice — store in voice 0 as sentinel.
                    voice_elements
                        .entry(0)
                        .or_default()
                        .push(VoiceElement::Backup(backup));
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
fn merge_chord(
    elements: &mut Vec<VoiceElement>,
    note: Note,
    arpeggio: Option<ArpeggioType>,
) {
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

    (
        MeasureAttributes {
            divisions,
            key,
            time,
            clefs,
            transpose,
            staves,
        },
        new_divisions,
    )
}

// ---------------------------------------------------------------------------
// Note parsing
// ---------------------------------------------------------------------------

enum NoteOrRest {
    Note(Box<Note>),
    Rest(Rest),
}

fn parse_note(elem: &XmlNode, divisions: i64) -> Option<NoteOrRest> {
    let is_rest = elem.find("rest").is_some();
    let is_grace = elem.find("grace").is_some();
    let voice_num = elem.child_i64("voice", 1) as u8;
    let staff_num = elem.child_i64("staff", 1) as u8;

    // Duration
    let dots = elem.find_all("dot").len() as u8;
    let type_name = elem.child_text("type");

    let duration = if is_grace {
        let tn = type_name.unwrap_or("eighth");
        Duration::from_musicxml_type(tn, dots).unwrap_or_default()
    } else if let Some(dur_elem) = elem.find("duration") {
        let dur_val = dur_elem.text_i64(0);
        if let Some(tn) = type_name {
            Duration::from_musicxml_type(tn, dots)
                .unwrap_or_else(|| Duration::from_divisions(dur_val, divisions, dots))
        } else {
            Duration::from_divisions(dur_val, divisions, dots)
        }
    } else {
        Duration::default()
    };

    // Tuplet scaling
    let duration = if let Some(time_mod) = elem.find("time-modification") {
        let actual = time_mod.child_i64("actual-notes", 1) as u8;
        let normal = time_mod.child_i64("normal-notes", 1) as u8;
        Duration {
            tuplet_normal: normal,
            tuplet_actual: actual,
            ..duration
        }
    } else {
        duration
    };

    if is_rest {
        let rest_elem = elem.find("rest").unwrap();
        let display_step = rest_elem.child_text("display-step").map(|s| s.to_string());
        let display_octave = rest_elem
            .find("display-octave")
            .map(|o| o.text_i64(0) as i32);
        let is_measure_rest = rest_elem.attr("measure") == Some("yes");

        let mut rest = Rest {
            duration,
            voice: voice_num,
            staff: staff_num,
            display_step,
            display_octave,
            is_measure_rest,
            is_spacer: false,
            fermata: None,
            tuplet: None,
        };

        // Check for fermata and tuplet display in notations.
        if let Some(notations) = elem.find("notations") {
            rest.fermata = parse_fermata(notations);
            if let Some(tuplet) = notations.find("tuplet") {
                let tuplet_type = match tuplet.attr("type").unwrap_or("start") {
                    "start" => StartStop::Start,
                    "stop" => StartStop::Stop,
                    _ => StartStop::Start,
                };
                let bracket = tuplet.attr("bracket") == Some("yes");
                let show_number =
                    tuplet.attr("show-number").unwrap_or("actual").to_string();
                rest.tuplet = Some(TupletDisplay {
                    tuplet_type,
                    bracket,
                    show_number,
                });
            }
        }

        return Some(NoteOrRest::Rest(rest));
    }

    // Pitched note
    let pitch = if let Some(pitch_elem) = elem.find("pitch") {
        parse_pitch(pitch_elem)?
    } else if let Some(unpitched) = elem.find("unpitched") {
        // Percussion: use display-step/display-octave.
        let step_str = unpitched.child_text("display-step").unwrap_or("C");
        let step = PitchStep::from_name(step_str)?;
        let octave = unpitched.child_i64("display-octave", 4) as i32;
        Pitch::new(step, octave)
    } else {
        return None;
    };

    // Accidental display
    let accidental = if let Some(acc_elem) = elem.find("accidental") {
        if acc_elem.attr("cautionary") == Some("yes") {
            AccidentalDisplay::Cautionary
        } else if acc_elem.attr("editorial") == Some("yes") {
            AccidentalDisplay::Editorial
        } else {
            AccidentalDisplay::Forced
        }
    } else {
        AccidentalDisplay::None
    };

    let mut note = Note::new(
        Pitch {
            accidental,
            ..pitch
        },
        duration,
    );
    note.voice = voice_num;
    note.staff = staff_num;
    note.is_grace = is_grace;
    note.grace_slash = elem
        .find("grace")
        .and_then(|g| g.attr("slash"))
        == Some("yes");
    note.after_grace = elem
        .find("grace")
        .and_then(|g| g.attr("steal-time-previous"))
        .is_some();
    note.is_cue = elem.find("cue").is_some();

    // Stem direction
    if let Some(stem) = elem.child_text("stem") {
        note.stem_direction = stem.to_string();
    }

    // Notehead
    if let Some(nh) = elem.child_text("notehead") {
        note.notehead = nh.to_string();
    }

    // print-object attribute
    if elem.attr("print-object") == Some("no") {
        note.print_object = false;
    }

    // Notations
    if let Some(notations) = elem.find("notations") {
        parse_notations(notations, &mut note);
    }

    // Lyrics
    for lyric_elem in elem.find_all("lyric") {
        if let Some(syllable) = parse_lyric(lyric_elem) {
            note.lyrics.push(syllable);
        }
    }

    // Beams
    for beam_elem in elem.find_all("beam") {
        let number: u8 = beam_elem
            .attr("number")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let beam_type = beam_elem.text_content().to_string();
        if !matches!(
            beam_type.as_str(),
            "begin" | "continue" | "end" | "forward hook" | "backward hook"
        ) {
            continue;
        }
        note.beams.push(BeamEvent { beam_type, number });
    }

    Some(NoteOrRest::Note(Box::new(note)))
}

fn parse_pitch(elem: &XmlNode) -> Option<Pitch> {
    let step_str = elem.child_text("step")?;
    let step = PitchStep::from_name(step_str)?;
    let octave = elem.child_i64("octave", 4) as i32;
    let alter = elem
        .find("alter")
        .map(|a| {
            let text = a.text_content();
            // Parse as float first to handle "0.5", "-0.5", etc., then convert to Ratio.
            if let Ok(f) = text.parse::<f64>() {
                // Convert to Ratio: multiply by 2 to get integer half-semitones.
                let half_semitones = (f * 2.0).round() as i32;
                Alter::new(half_semitones, 2)
            } else {
                Alter::from_integer(0)
            }
        })
        .unwrap_or_else(|| Alter::from_integer(0));

    Some(Pitch::with_alter(step, alter, octave))
}

fn parse_notations(notations: &XmlNode, note: &mut Note) {
    // Ties
    for tied in notations.find_all("tied") {
        let tie_type = tied.attr("type").unwrap_or("");
        let event = match tie_type {
            "start" => TieEvent {
                tie_type: StartStop::Start,
            },
            "stop" => TieEvent {
                tie_type: StartStop::Stop,
            },
            _ => continue,
        };
        note.ties.push(event);
    }

    // Slurs
    for slur in notations.find_all("slur") {
        let slur_type = slur.attr("type").unwrap_or("");
        let number: u8 = slur
            .attr("number")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let placement = slur
            .attr("placement")
            .map(|p| match p {
                "above" => Placement::Above,
                "below" => Placement::Below,
                _ => Placement::Unspecified,
            })
            .unwrap_or(Placement::Unspecified);
        let event = match slur_type {
            "start" => SlurEvent {
                slur_type: StartStop::Start,
                number,
                placement,
            },
            "stop" => SlurEvent {
                slur_type: StartStop::Stop,
                number,
                placement: Placement::Unspecified,
            },
            _ => continue,
        };
        note.slurs.push(event);
    }

    // Articulations
    if let Some(arts) = notations.find("articulations") {
        for child in &arts.children {
            let name = match child.tag.as_str() {
                "staccato" | "staccatissimo" | "accent" | "strong-accent" | "marcato"
                | "tenuto" | "detached-legato" | "stress" | "spiccato" | "breath-mark"
                | "caesura" | "portato" => child.tag.as_str(),
                _ => continue,
            };
            let placement = child
                .attr("placement")
                .map(|p| match p {
                    "above" => Placement::Above,
                    "below" => Placement::Below,
                    _ => Placement::Unspecified,
                })
                .unwrap_or(Placement::Unspecified);
            note.articulations.push(Articulation {
                name: name.to_string(),
                placement,
            });
        }
    }

    // Ornaments
    if let Some(orns) = notations.find("ornaments") {
        for child in &orns.children {
            let name = match child.tag.as_str() {
                "trill-mark" | "mordent" | "inverted-mordent" | "turn" | "inverted-turn"
                | "tremolo" => child.tag.as_str(),
                _ => continue,
            };
            let placement = child
                .attr("placement")
                .map(|p| match p {
                    "above" => Placement::Above,
                    "below" => Placement::Below,
                    _ => Placement::Unspecified,
                })
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: name.to_string(),
                placement,
            });
        }
    }

    // Technicals
    if let Some(techs) = notations.find("technical") {
        for child in &techs.children {
            let (name, value) = match child.tag.as_str() {
                "up-bow" | "down-bow" | "harmonic" | "open-string" | "stopped"
                | "snap-pizzicato" => (child.tag.as_str(), ""),
                "fingering" | "fret" | "string" => (child.tag.as_str(), child.text_content()),
                _ => continue,
            };
            note.technicals.push(Technical {
                name: name.to_string(),
                value: value.to_string(),
            });
        }
    }

    // Dynamics (inside notations)
    if let Some(dyn_elem) = notations.find("dynamics") {
        let placement = dyn_elem
            .attr("placement")
            .map(|p| match p {
                "above" => Placement::Above,
                "below" => Placement::Below,
                _ => Placement::Unspecified,
            })
            .unwrap_or(Placement::Unspecified);
        for child in &dyn_elem.children {
            let sign = match child.tag.as_str() {
                "ppp" | "pp" | "p" | "mp" | "mf" | "f" | "ff" | "fff" | "sf" | "sfz" | "fp" => {
                    child.tag.as_str()
                }
                _ => continue,
            };
            note.dynamics.push(DynamicMark {
                sign: sign.to_string(),
                placement,
            });
        }
    }

    // Tuplet display
    if let Some(tuplet) = notations.find("tuplet") {
        let tuplet_type = match tuplet.attr("type").unwrap_or("start") {
            "start" => StartStop::Start,
            "stop" => StartStop::Stop,
            _ => StartStop::Start,
        };
        let bracket = tuplet.attr("bracket") == Some("yes");
        let show_number = tuplet.attr("show-number").unwrap_or("actual").to_string();
        note.tuplet = Some(TupletDisplay {
            tuplet_type,
            bracket,
            show_number,
        });
    }

    // Fermata
    note.fermata = parse_fermata(notations);

    // Glissando
    if let Some(gliss) = notations.find("glissando") {
        let gliss_type = match gliss.attr("type").unwrap_or("") {
            "start" => Some(StartStop::Start),
            "stop" => Some(StartStop::Stop),
            _ => None,
        };
        note.glissando = gliss_type;
        note.glissando_line_type = gliss.attr("line-type").map(|s| s.to_string());
    }

    // Slide (portamento)
    if let Some(slide) = notations.find("slide") {
        let slide_type = match slide.attr("type").unwrap_or("") {
            "start" => Some(StartStop::Start),
            "stop" => Some(StartStop::Stop),
            _ => None,
        };
        note.slide = slide_type;
    }
}

fn parse_fermata(notations: &XmlNode) -> Option<Fermata> {
    let fermata_elem = notations.find("fermata")?;
    let shape = match fermata_elem.text_content() {
        "normal" | "" => "normal",
        "angled" => "angled",
        "square" => "square",
        other => other,
    };
    let inverted = fermata_elem.attr("type") == Some("inverted");
    Some(Fermata {
        shape: shape.to_string(),
        inverted,
    })
}

fn parse_lyric(elem: &XmlNode) -> Option<LyricSyllable> {
    let text = elem.child_text("text")?.to_string();
    let syllabic = elem.child_text("syllabic").unwrap_or("single");
    let syllabic_type = match syllabic {
        "single" => SyllabicType::Single,
        "begin" => SyllabicType::Begin,
        "middle" => SyllabicType::Middle,
        "end" => SyllabicType::End,
        _ => SyllabicType::Single,
    };
    let number: u8 = elem
        .attr("number")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let extend = elem.find("extend").is_some();
    let elision = elem.find("elision").is_some();

    Some(LyricSyllable {
        text,
        syllabic: syllabic_type,
        number,
        extend,
        elision,
    })
}

// ---------------------------------------------------------------------------
// Direction parsing
// ---------------------------------------------------------------------------

fn parse_direction(elem: &XmlNode) -> Option<Direction> {
    let mut dir = Direction::default();
    let placement_str = elem.attr("placement");
    if let Some(p) = placement_str {
        dir.placement = match p {
            "above" => Placement::Above,
            "below" => Placement::Below,
            _ => Placement::Unspecified,
        };
    }

    for dir_type in elem.find_all("direction-type") {
        for child in &dir_type.children {
            match child.tag.as_str() {
                "dynamics" => {
                    // Take the first dynamic child element.
                    for dyn_child in &child.children {
                        let sign = match dyn_child.tag.as_str() {
                            "ppp" | "pp" | "p" | "mp" | "mf" | "f" | "ff" | "fff" | "sf"
                            | "sfz" | "fp" => dyn_child.tag.as_str(),
                            _ => continue,
                        };
                        dir.dynamic = Some(DynamicMark {
                            sign: sign.to_string(),
                            placement: dir.placement,
                        });
                        break;
                    }
                }
                "wedge" => {
                    let wedge_type = child.attr("type").unwrap_or("").to_string();
                    if !wedge_type.is_empty() {
                        dir.wedge = Some(Wedge {
                            wedge_type,
                            placement: dir.placement,
                        });
                    }
                }
                "words" => {
                    let text = child.text_content().to_string();
                    if !text.is_empty() {
                        dir.text = Some(TextDirection {
                            text,
                            placement: dir.placement,
                            font_style: child.attr("font-style").map(|s| s.to_string()),
                            font_weight: child.attr("font-weight").map(|s| s.to_string()),
                        });
                    }
                }
                "rehearsal" => {
                    let text = child.text_content().to_string();
                    dir.rehearsal = Some(RehearsalMark { text });
                }
                "metronome" => {
                    let beat_unit = child.child_text("beat-unit").map(|s| s.to_string());
                    let per_minute = child
                        .find("per-minute")
                        .and_then(|pm| pm.text_content().parse::<f64>().ok());
                    let dots = child.find_all("beat-unit-dot").len() as u8;
                    dir.tempo = Some(TempoDirection {
                        text: None,
                        beat_unit,
                        per_minute,
                        dots,
                        placement: dir.placement,
                    });
                }
                "octave-shift" => {
                    let shift_type = child.attr("type").unwrap_or("up").to_string();
                    let size = child
                        .attr("size")
                        .and_then(|s| s.parse::<i8>().ok())
                        .unwrap_or(8);
                    dir.octave_shift = Some(OctaveShift { shift_type, size });
                }
                "pedal" => {
                    let pedal_type = child.attr("type").unwrap_or("").to_string();
                    let line = child.attr("line") == Some("yes");
                    if !pedal_type.is_empty() {
                        dir.pedal = Some(PedalEvent { pedal_type, line });
                    }
                }
                "coda" => {
                    dir.coda = true;
                }
                "segno" => {
                    dir.segno = true;
                }
                _ => {}
            }
        }
    }

    // Check <sound> element for dacapo/dalsegno attributes
    if let Some(sound) = elem.find("sound") {
        if let Some(dc) = sound.attr("dacapo") {
            if dc == "yes" {
                dir.da_capo = Some("D.C.".to_string());
            }
        }
        if let Some(ds) = sound.attr("dalsegno") {
            if !ds.is_empty() {
                dir.dal_segno = Some("D.S.".to_string());
            }
        }
    }

    // Check if any content was parsed.
    if dir.dynamic.is_none()
        && dir.wedge.is_none()
        && dir.text.is_none()
        && dir.rehearsal.is_none()
        && dir.tempo.is_none()
        && dir.octave_shift.is_none()
        && dir.pedal.is_none()
        && !dir.coda
        && !dir.segno
        && dir.da_capo.is_none()
        && dir.dal_segno.is_none()
    {
        return None;
    }

    Some(dir)
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_score() {
        let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1">
      <part-name>Piano</part-name>
    </score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <key><fifths>0</fifths><mode>major</mode></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>1</duration>
        <voice>1</voice>
        <type>quarter</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();

        assert_eq!(score.parts().len(), 1);
        let part = &score.parts()[0];
        assert_eq!(part.name, "Piano");
        assert_eq!(part.measures.len(), 1);

        let measure = &part.measures[0];
        assert_eq!(measure.number, 1);
        assert!(measure.attributes.is_some());

        let attrs = measure.attributes.as_ref().unwrap();
        assert_eq!(attrs.divisions, 1);
        assert!(attrs.key.is_some());
        assert!(attrs.time.is_some());
        assert_eq!(attrs.time.as_ref().unwrap().beats, "4");
        assert_eq!(attrs.time.as_ref().unwrap().beat_type, 4);

        // One voice with one note
        assert_eq!(measure.voices.len(), 1);
        assert_eq!(measure.voices[0].elements.len(), 1);
        match &measure.voices[0].elements[0] {
            VoiceElement::Note(n) => {
                assert_eq!(n.pitch.step, PitchStep::C);
                assert_eq!(n.pitch.octave, 4);
                assert_eq!(n.voice, 1);
            }
            other => panic!("Expected Note, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_chord() {
        let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions></attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>1</duration><voice>1</voice><type>quarter</type>
      </note>
      <note>
        <chord/>
        <pitch><step>E</step><octave>4</octave></pitch>
        <duration>1</duration><voice>1</voice><type>quarter</type>
      </note>
      <note>
        <chord/>
        <pitch><step>G</step><octave>4</octave></pitch>
        <duration>1</duration><voice>1</voice><type>quarter</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
        let measure = &score.parts()[0].measures[0];
        assert_eq!(measure.voices.len(), 1);
        assert_eq!(measure.voices[0].elements.len(), 1);

        match &measure.voices[0].elements[0] {
            VoiceElement::Chord(chord) => {
                assert_eq!(chord.notes.len(), 3);
                assert_eq!(chord.notes[0].pitch.step, PitchStep::C);
                assert_eq!(chord.notes[1].pitch.step, PitchStep::E);
                assert_eq!(chord.notes[2].pitch.step, PitchStep::G);
            }
            other => panic!("Expected Chord, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_rest() {
        let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions></attributes>
      <note>
        <rest measure="yes"/>
        <duration>4</duration><voice>1</voice><type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
        let measure = &score.parts()[0].measures[0];
        match &measure.voices[0].elements[0] {
            VoiceElement::Rest(r) => {
                assert!(r.is_measure_rest);
            }
            other => panic!("Expected Rest, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_accidentals() {
        let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions></attributes>
      <note>
        <pitch><step>F</step><alter>1</alter><octave>4</octave></pitch>
        <duration>1</duration><voice>1</voice><type>quarter</type>
        <accidental>sharp</accidental>
      </note>
      <note>
        <pitch><step>B</step><alter>-1</alter><octave>3</octave></pitch>
        <duration>1</duration><voice>1</voice><type>quarter</type>
        <accidental cautionary="yes">flat</accidental>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
        let elems = &score.parts()[0].measures[0].voices[0].elements;

        match &elems[0] {
            VoiceElement::Note(n) => {
                assert_eq!(n.pitch.step, PitchStep::F);
                assert_eq!(n.pitch.alter, Alter::from_integer(1));
                assert_eq!(n.pitch.accidental, AccidentalDisplay::Forced);
            }
            other => panic!("Expected Note, got {:?}", other),
        }
        match &elems[1] {
            VoiceElement::Note(n) => {
                assert_eq!(n.pitch.step, PitchStep::B);
                assert_eq!(n.pitch.alter, Alter::from_integer(-1));
                assert_eq!(n.pitch.accidental, AccidentalDisplay::Cautionary);
            }
            other => panic!("Expected Note, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_multivoice() {
        let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions></attributes>
      <note>
        <pitch><step>C</step><octave>5</octave></pitch>
        <duration>4</duration><voice>1</voice><type>whole</type>
      </note>
      <backup><duration>4</duration></backup>
      <note>
        <pitch><step>E</step><octave>3</octave></pitch>
        <duration>4</duration><voice>2</voice><type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
        let measure = &score.parts()[0].measures[0];

        // Voice 0 gets the backup, voices 1 and 2 get notes.
        let voice_numbers: Vec<u8> = measure.voices.iter().map(|v| v.number).collect();
        assert!(voice_numbers.contains(&1));
        assert!(voice_numbers.contains(&2));

        // Each voice has exactly one note.
        for voice in &measure.voices {
            if voice.number == 1 || voice.number == 2 {
                assert_eq!(voice.elements.len(), 1);
            }
        }
    }

    #[test]
    fn test_parse_directions() {
        let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions></attributes>
      <direction>
        <direction-type>
          <dynamics><f/></dynamics>
        </direction-type>
      </direction>
      <direction>
        <direction-type>
          <metronome>
            <beat-unit>quarter</beat-unit>
            <per-minute>120</per-minute>
          </metronome>
        </direction-type>
      </direction>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration><voice>1</voice><type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
        let measure = &score.parts()[0].measures[0];

        assert_eq!(measure.directions.len(), 2);

        // First direction: dynamics f
        let dyn_mark = measure.directions[0].dynamic.as_ref().unwrap();
        assert_eq!(dyn_mark.sign, "f");

        // Second direction: tempo
        assert!(measure.directions[1].tempo.is_some());
        let tempo = measure.directions[1].tempo.as_ref().unwrap();
        assert_eq!(tempo.beat_unit.as_deref(), Some("quarter"));
        assert_eq!(tempo.per_minute, Some(120.0));
    }

    #[test]
    fn test_parse_all_fixtures() {
        // Parse all 143 MusicXML fixture files. If any fail to parse, the
        // file name is collected and reported.
        let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("xml");
        let adapter = MxmlToIrAdapter::new();

        let mut failures: Vec<(String, String)> = Vec::new();
        let mut success_count = 0;

        for entry in std::fs::read_dir(&fixture_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "xml") {
                let xml = std::fs::read_to_string(&path).unwrap();
                match adapter.convert_str(&xml) {
                    Ok(score) => {
                        // Sanity: the score should have at least one part.
                        assert!(!score.parts().is_empty(), "no parts in {}", path.display());
                        success_count += 1;
                    }
                    Err(e) => {
                        failures.push((
                            path.file_name().unwrap().to_string_lossy().to_string(),
                            e.to_string(),
                        ));
                    }
                }
            }
        }

        if !failures.is_empty() {
            let report: Vec<String> = failures
                .iter()
                .map(|(f, e)| format!("  {f}: {e}"))
                .collect();
            panic!(
                "{} of {} fixture files failed to parse:\n{}",
                failures.len(),
                success_count + failures.len(),
                report.join("\n")
            );
        }

        assert!(
            success_count >= 100,
            "expected at least 100 fixtures, got {success_count}"
        );
    }

    #[test]
    fn test_parse_credit_metadata() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <credit page="1">
    <credit-type>title</credit-type>
    <credit-words>My Title</credit-words>
  </credit>
  <credit page="1">
    <credit-type>composer</credit-type>
    <credit-words>A. Composer</credit-words>
  </credit>
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        assert_eq!(score.metadata.title.as_deref(), Some("My Title"));
        assert_eq!(score.metadata.composer.as_deref(), Some("A. Composer"));
    }

    #[test]
    fn test_parse_grace_slash() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <grace slash="yes"/>
        <pitch><step>E</step><octave>5</octave></pitch>
        <type>16th</type>
      </note>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let elems: Vec<_> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert!(n.is_grace, "should be grace note");
                assert!(n.grace_slash, "slash=yes should set grace_slash");
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn test_parse_harmony() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <harmony>
        <root><root-step>C</root-step></root>
        <kind>major</kind>
        <bass><bass-step>E</bass-step></bass>
      </harmony>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let m = &score.parts()[0].measures[0];
        assert_eq!(m.harmonies.len(), 1);
        assert_eq!(m.harmonies[0].root.step, "C");
        assert_eq!(m.harmonies[0].kind, "major");
        assert_eq!(m.harmonies[0].bass.as_ref().unwrap().step, "E");
    }

    #[test]
    fn test_parse_figured_bass() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
    <score-part id="P1"><part-name>Bass</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <figured-bass>
        <figure><figure-number>6</figure-number></figure>
        <figure><figure-number>4</figure-number></figure>
        <duration>4</duration>
      </figured-bass>
      <note>
        <pitch><step>C</step><octave>3</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let m = &score.parts()[0].measures[0];
        assert_eq!(m.figured_bass.len(), 1);
        assert_eq!(m.figured_bass[0].figures.len(), 2);
        assert_eq!(m.figured_bass[0].figures[0].number, Some(6));
        assert_eq!(m.figured_bass[0].figures[1].number, Some(4));
    }

    #[test]
    fn test_parse_glissando() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>2</duration>
        <type>half</type>
        <notations>
          <glissando type="start" line-type="dashed">gliss.</glissando>
        </notations>
      </note>
      <note>
        <pitch><step>E</step><octave>4</octave></pitch>
        <duration>2</duration>
        <type>half</type>
        <notations>
          <glissando type="stop"/>
        </notations>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let elems: Vec<_> = score.parts()[0].measures.iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .collect();
        match &elems[0] {
            VoiceElement::Note(n) => {
                assert_eq!(n.glissando, Some(StartStop::Start));
                assert_eq!(n.glissando_line_type.as_deref(), Some("dashed"));
            }
            _ => panic!("expected Note"),
        }
        match &elems[1] {
            VoiceElement::Note(n) => {
                assert_eq!(n.glissando, Some(StartStop::Stop));
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn test_parse_coda_segno() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <direction>
        <direction-type><coda/></direction-type>
      </direction>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
    <measure number="2">
      <direction>
        <direction-type><segno/></direction-type>
        <sound dalsegno="D.S. al Coda"/>
      </direction>
      <note>
        <pitch><step>D</step><octave>4</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let m1 = &score.parts()[0].measures[0];
        assert!(m1.directions.iter().any(|d| d.coda), "measure 1 should have coda");
        let m2 = &score.parts()[0].measures[1];
        assert!(m2.directions.iter().any(|d| d.segno), "measure 2 should have segno");
        assert!(
            m2.directions.iter().any(|d| d.dal_segno.is_some()),
            "measure 2 should have dal segno"
        );
    }

    #[test]
    fn test_parse_defaults_page_layout() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <defaults>
    <scaling>
      <millimeters>7.05556</millimeters>
      <tenths>40</tenths>
    </scaling>
    <page-layout>
      <page-height>1683.36</page-height>
      <page-width>1190.88</page-width>
    </page-layout>
  </defaults>
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>4</duration>
        <type>whole</type>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let layout = score.page_layout.as_ref().expect("should have page layout");
        assert!(layout.staff_size.is_some(), "should have staff size");
        assert!(layout.page_height.is_some(), "should have page height");
        assert!(layout.page_width.is_some(), "should have page width");
        // 7.05556mm / 40 tenths = 0.1763889 mm/tenth
        // page_height = 1683.36 * 0.1763889 / 10 ≈ 29.7 cm
        let h = layout.page_height.unwrap();
        assert!((h - 29.7).abs() < 0.1, "page height should be ~29.7cm, got {h}");
    }

    #[test]
    fn test_parse_anacrusis() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
    <score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="0" implicit="yes">
      <attributes>
        <divisions>1</divisions>
        <time><beats>3</beats><beat-type>4</beat-type></time>
      </attributes>
      <note>
        <pitch><step>G</step><octave>4</octave></pitch>
        <duration>1</duration>
        <type>quarter</type>
      </note>
    </measure>
    <measure number="1">
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>3</duration>
        <type>half</type>
        <dot/>
      </note>
    </measure>
  </part>
</score-partwise>"#;

        let adapter = MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).unwrap();
        let partial = score.metadata.partial_duration.as_ref()
            .expect("should detect anacrusis");
        // 1 quarter note in a 3/4 measure → partial duration = 1/4
        assert_eq!(partial.base, Ratio::new(1, 4), "partial should be a quarter note");
    }
}
