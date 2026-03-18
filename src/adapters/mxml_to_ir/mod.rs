//! MusicXML → IR adapter.
//!
//! Parses MusicXML (both `.xml` and `.mxl`) into the lytk IR tree.
//!
//! # Reference
//! Ported from `lytk-py/converters/mxml_to_ir.py`.

mod direction;
mod helpers;
mod note;
mod part;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::collections::HashMap;
use std::path::Path;

use num::rational::Ratio;

use super::mxl_zip;
use super::{AdapterError, Result, ToIrAdapter};
use crate::ir::duration::Duration;
use crate::ir::note::*;
use crate::ir::score::*;

use helpers::{parse_xml, XmlNode};
use part::parse_part;

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

impl super::ToMusicAdapter for MxmlToIrAdapter {
    fn convert_file_to_music(
        &self,
        path: &Path,
    ) -> Result<crate::ir::music::MusicDocument> {
        let score = self.convert_file(path)?;
        Ok(crate::ir::lift::lift_to_music(&score))
    }

    fn convert_str_to_music(
        &self,
        text: &str,
    ) -> Result<crate::ir::music::MusicDocument> {
        let score = self.convert_str(text)?;
        Ok(crate::ir::lift::lift_to_music(&score))
    }
}

// ---------------------------------------------------------------------------
// Part-list helper struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(super) struct PartInfo {
    pub id: String,
    pub name: String,
    pub abbreviation: String,
    pub midi_instrument: String,
    pub midi_channel: u8,
    pub midi_program: u8,
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
    let mut parsed_parts: HashMap<String, crate::ir::part::Part> = HashMap::new();
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
// Part-list parsing
// ---------------------------------------------------------------------------

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
