//! MusicXML → IR adapter.
//!
//! Parses MusicXML (both `.xml` and `.mxl`) into the lytk IR tree,
//! using the `musicxml` crate for XML/MXL parsing.

mod direction;
mod note;
mod part;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};
use std::path::Path;

use num::rational::Ratio;

use super::{AdapterError, Result, ToIrAdapter};
use crate::ir::duration::Duration;
use crate::ir::note::*;
use crate::ir::score::*;

use musicxml::elements as mxml;

use part::convert_part;

// ---------------------------------------------------------------------------
// Adapter struct
// ---------------------------------------------------------------------------

/// Adapter that reads MusicXML and produces an IR [`Score`].
pub struct MxmlToIrAdapter;

impl MxmlToIrAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Parse MusicXML or compressed MXL from raw bytes into a [`Score`],
    /// auto-detecting an `.mxl` ZIP archive vs plain XML (with the same bomb /
    /// panic firewalls as the file/string paths).
    pub fn convert_bytes(&self, bytes: &[u8]) -> Result<Score> {
        read_partwise_bytes(bytes.to_vec())
    }
}

impl Default for MxmlToIrAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a `musicxml`-crate read, converting a panic into a normal error.
///
/// We feed the crate only plain XML (`.mxl` is decompressed by us first, see
/// [`xml_bytes_from_input`], which avoids its panic-prone ZIP reader entirely),
/// but its XML parser can still panic on malformed input. This firewall turns
/// such a panic into a clean `AdapterError::Parse` instead of letting it cross
/// the PyO3 boundary as an opaque `PanicException`.
fn catch_read<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(AdapterError::Parse(
            "malformed MusicXML/MXL: the reader panicked (rejected)".into(),
        )),
    }
}

/// Maximum accepted raw input size (compressed `.mxl` or plain `.xml`).
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
/// Maximum decompressed MusicXML size — caps a `.mxl` decompression bomb.
const MAX_XML_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB
/// ZIP local-file-header magic, identifying an `.mxl` archive.
const ZIP_MAGIC: &[u8; 4] = b"PK\x03\x04";

/// First `full-path="…"` value from an MXL `META-INF/container.xml`.
fn container_full_path(xml: &str) -> Option<String> {
    let rest = &xml[xml.find("full-path")?..];
    let qpos = rest.find(['"', '\''])?;
    let quote = rest.as_bytes()[qpos] as char;
    let after = &rest[qpos + 1..];
    let end = after.find(quote)?;
    Some(after[..end].to_string())
}

/// Name of the root MusicXML part inside an MXL archive: the `full-path` declared
/// in `META-INF/container.xml`, else the first non-`META-INF` `.xml`/`.musicxml`.
fn mxl_rootfile_name<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Result<String> {
    if let Ok(mut c) = zip.by_name("META-INF/container.xml") {
        let mut s = String::new();
        if c.by_ref().take(1 << 20).read_to_string(&mut s).is_ok() {
            if let Some(p) = container_full_path(&s) {
                return Ok(p);
            }
        }
    }
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    names
        .into_iter()
        .find(|n| !n.starts_with("META-INF/") && (n.ends_with(".xml") || n.ends_with(".musicxml")))
        .ok_or_else(|| AdapterError::Parse("MXL archive has no MusicXML part".into()))
}

/// If `bytes` is an MXL (ZIP) archive, decompress its root MusicXML part with a
/// bounded reader (rejecting a decompression bomb); otherwise return the bytes
/// unchanged (plain XML). This deliberately bypasses the vendored `musicxml`
/// crate's unbounded, panic-prone ZIP reader for `.mxl`.
fn xml_bytes_from_input(bytes: Vec<u8>) -> Result<Vec<u8>> {
    xml_bytes_from_input_capped(bytes, MAX_XML_BYTES)
}

/// [`xml_bytes_from_input`] with an explicit decompressed-size cap (parameterised
/// so tests can exercise bomb rejection without allocating the production cap).
fn xml_bytes_from_input_capped(bytes: Vec<u8>, max_xml: u64) -> Result<Vec<u8>> {
    if bytes.len() < 4 || &bytes[..4] != ZIP_MAGIC {
        return Ok(bytes); // plain XML
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| AdapterError::Parse(format!("invalid MXL archive: {e}")))?;
    let root = mxl_rootfile_name(&mut zip)?;
    let mut entry = zip
        .by_name(&root)
        .map_err(|e| AdapterError::Parse(format!("MXL root part '{root}' not found: {e}")))?;
    // Read at most max_xml + 1: anything larger is rejected as a decompression
    // bomb rather than allowed to exhaust memory.
    let mut out = Vec::new();
    entry
        .by_ref()
        .take(max_xml + 1)
        .read_to_end(&mut out)
        .map_err(|e| AdapterError::Parse(format!("MXL decompression failed: {e}")))?;
    if out.len() as u64 > max_xml {
        return Err(AdapterError::Parse(format!(
            "MXL decompresses past the {} MiB limit (possible decompression bomb)",
            max_xml / (1024 * 1024)
        )));
    }
    Ok(out)
}

/// Parse raw MusicXML/MXL bytes into a [`Score`] with bomb/panic firewalls.
fn read_partwise_bytes(bytes: Vec<u8>) -> Result<Score> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(AdapterError::Parse(format!(
            "MusicXML input exceeds the {} MiB limit",
            MAX_INPUT_BYTES / (1024 * 1024)
        )));
    }
    let xml = xml_bytes_from_input(bytes)?;
    // The firewall covers conversion too: panics there would otherwise escape
    // to the PyO3 boundary as aborts instead of AdapterError::Parse.
    catch_read(|| {
        let mxml_score = musicxml::read_score_data_partwise(xml).map_err(AdapterError::Parse)?;
        convert_mxml_score(&mxml_score)
    })
}

impl ToIrAdapter for MxmlToIrAdapter {
    fn convert_file(&self, path: &Path) -> Result<Score> {
        let bytes = std::fs::read(path)?;
        read_partwise_bytes(bytes)
    }

    fn convert_str(&self, text: &str) -> Result<Score> {
        read_partwise_bytes(text.as_bytes().to_vec())
    }
}

impl super::ToMusicAdapter for MxmlToIrAdapter {
    fn convert_file_to_music(&self, path: &Path) -> Result<crate::ir::music::MusicDocument> {
        let score = self.convert_file(path)?;
        Ok(crate::ir::lift::lift_to_music(&score))
    }

    fn convert_str_to_music(&self, text: &str) -> Result<crate::ir::music::MusicDocument> {
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
// Score-level conversion
// ---------------------------------------------------------------------------

fn convert_mxml_score(score: &mxml::ScorePartwise) -> Result<Score> {
    let metadata = parse_metadata(score);
    let page_layout = parse_defaults(score);
    let mut ir_score = Score {
        metadata,
        page_layout,
        children: Vec::new(),
    };

    // Parse part-list for part info and part groups.
    let mut part_info: HashMap<String, PartInfo> = HashMap::new();
    let mut part_order: Vec<String> = Vec::new();
    let mut completed_groups: Vec<(PartGroup, Vec<String>)> = Vec::new();
    let mut active_groups: Vec<(PartGroup, Vec<String>)> = Vec::new();

    for item in &score.content.part_list.content.content {
        match item {
            mxml::PartListElement::ScorePart(sp) => {
                let info = parse_score_part(sp);
                let id = info.id.clone();
                part_info.insert(id.clone(), info);
                part_order.push(id.clone());
                for (_, parts) in &mut active_groups {
                    parts.push(id.clone());
                }
            }
            mxml::PartListElement::PartGroup(pg) => {
                use musicxml::datatypes::StartStop;
                match pg.attributes.r#type {
                    StartStop::Start => {
                        let group = parse_part_group(pg);
                        active_groups.push((group, Vec::new()));
                    }
                    StartStop::Stop => {
                        if let Some(completed) = active_groups.pop() {
                            completed_groups.push(completed);
                        }
                    }
                }
            }
        }
    }

    // Parse each <part> element.
    let mut parsed_parts: HashMap<String, crate::ir::part::Part> = HashMap::new();
    for mxml_part in &score.content.part {
        // In partwise, part.content contains PartElement::Measure(...)
        let id = mxml_part.attributes.id.0.clone();
        let info = if id.is_empty() {
            if part_info.len() == 1 {
                part_info.values().next().cloned().unwrap_or_default()
            } else {
                PartInfo::default()
            }
        } else {
            part_info.get(&id).cloned().unwrap_or_default()
        };
        let resolved_id = if id.is_empty() { info.id.clone() } else { id };
        let part = convert_part(mxml_part, &info)?;
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
            ir_score.children.push(ScoreChild::PartGroup(pg));
        }
    }

    // Add ungrouped parts in original order.
    for id in &part_order {
        if !grouped_ids.contains(id) {
            if let Some(part) = parsed_parts.remove(id) {
                ir_score.children.push(ScoreChild::Part(part));
            }
        }
    }

    // Detect anacrusis: check if the first measure is implicit (pickup)
    detect_anacrusis(&mut ir_score);

    Ok(ir_score)
}

/// Detect anacrusis (pickup measure) and set `score.metadata.partial_duration`.
fn detect_anacrusis(score: &mut Score) {
    let first_measure = score.parts().first().and_then(|p| p.measures.first());

    if let Some(measure) = first_measure {
        if !measure.implicit {
            return;
        }
        if let Some(voice) = measure.voices.first() {
            let mut total = Ratio::new(0i64, 1);
            for elem in &voice.elements {
                let dur = match elem {
                    VoiceElement::Note(n) => &n.duration,
                    VoiceElement::Rest(r) => &r.duration,
                    VoiceElement::Chord(c) => &c.duration,
                };
                total += dur.actual_duration();
            }
            if total > Ratio::new(0i64, 1) {
                let partial = Duration::new(total);
                score.metadata.partial_duration = Some(partial);
            }
        }
    }
}

fn parse_metadata(score: &mxml::ScorePartwise) -> ScoreMetadata {
    let mut meta = ScoreMetadata::default();

    // Work title
    if let Some(ref work) = score.content.work {
        if let Some(ref wt) = work.content.work_title {
            meta.title = Some(wt.content.clone());
        }
    }
    // Movement title as fallback
    if meta.title.is_none() {
        if let Some(ref mt) = score.content.movement_title {
            meta.title = Some(mt.content.clone());
        }
    }

    // Identification
    if let Some(ref ident) = score.content.identification {
        for creator in &ident.content.creator {
            let creator_type = creator
                .attributes
                .r#type
                .as_ref()
                .map(|t| t.0.as_str())
                .unwrap_or("");
            let text = creator.content.clone();
            match creator_type {
                "composer" => meta.composer = Some(text),
                "arranger" => meta.arranger = Some(text),
                "lyricist" => meta.lyricist = Some(text),
                _ => {
                    meta.extra.insert(creator_type.to_string(), text);
                }
            }
        }
        for rights in &ident.content.rights {
            let rights_type = rights
                .attributes
                .r#type
                .as_ref()
                .map(|t| t.0.as_str())
                .unwrap_or("")
                .to_string();
            meta.rights.push((rights_type, rights.content.clone()));
        }
    }

    // Fallback: <credit> elements
    for credit in &score.content.credit {
        let credit_type = credit
            .content
            .credit_type
            .first()
            .map(|ct| ct.content.to_lowercase())
            .unwrap_or_default();
        let words = match &credit.content.credit {
            mxml::CreditSubcontents::Text(text) => text
                .credit_words
                .as_ref()
                .map(|cw| cw.content.clone())
                .unwrap_or_default(),
            mxml::CreditSubcontents::Image(_) => String::new(),
        };
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
            "lyricist" | "poet" if meta.lyricist.is_none() => {
                meta.lyricist = Some(words);
            }
            _ => {}
        }
    }

    meta
}

// ---------------------------------------------------------------------------
// Defaults / page layout parsing
// ---------------------------------------------------------------------------

fn parse_defaults(score: &mxml::ScorePartwise) -> Option<PageLayout> {
    let defaults = score.content.defaults.as_ref()?;

    let (mm, tenths_val) = if let Some(ref scaling) = defaults.content.scaling {
        let mm_val: f64 = scaling.content.millimeters.content.0;
        let tenths_v: f64 = scaling.content.tenths.content.0;
        (mm_val, tenths_v)
    } else {
        return None;
    };

    let scale = mm / tenths_val;
    let to_cm = |val: f64| val * scale / 10.0;
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

    if let Some(ref pl) = defaults.content.page_layout {
        layout.page_height = pl.content.page_height.as_ref().map(|h| to_cm(h.content.0));
        layout.page_width = pl.content.page_width.as_ref().map(|w| to_cm(w.content.0));

        if let Some(margins) = pl.content.page_margins.first() {
            layout.left_margin = Some(to_cm(margins.content.left_margin.content.0));
            layout.right_margin = Some(to_cm(margins.content.right_margin.content.0));
            layout.top_margin = Some(to_cm(margins.content.top_margin.content.0));
            layout.bottom_margin = Some(to_cm(margins.content.bottom_margin.content.0));
        }
    }

    if let Some(ref sl) = defaults.content.system_layout {
        layout.system_distance = sl
            .content
            .system_distance
            .as_ref()
            .map(|d| to_cm(d.content.0));
        layout.top_system_distance = sl
            .content
            .top_system_distance
            .as_ref()
            .map(|d| to_cm(d.content.0));
    }

    Some(layout)
}

// ---------------------------------------------------------------------------
// Part-list parsing
// ---------------------------------------------------------------------------

fn parse_score_part(sp: &mxml::ScorePart) -> PartInfo {
    let mut info = PartInfo {
        id: sp.attributes.id.0.clone(),
        ..Default::default()
    };

    info.name = sp.content.part_name.content.clone();

    if let Some(ref abbrev) = sp.content.part_abbreviation {
        info.abbreviation = abbrev.content.clone();
    }

    // MIDI instrument info (first <midi-instrument> child).
    if let Some(midi) = sp.content.midi_instrument.first() {
        if let Some(ref ch) = midi.content.midi_channel {
            info.midi_channel = ch.content.0;
        }
        // MusicXML `<midi-program>` is 1-indexed (1–128); the IR stores the
        // 0-indexed MIDI program (matching the MIDI Program Change value).
        let program_0 = midi
            .content
            .midi_program
            .as_ref()
            .map(|p| p.content.0.saturating_sub(1));
        if let Some(p) = program_0 {
            info.midi_program = p;
        }
        if let Some(ref name) = midi.content.midi_name {
            info.midi_instrument = name.content.clone();
        } else if let Some(gm) = program_0.and_then(crate::adapters::gm::gm_name_from_program) {
            // No `<midi-name>`: recover the GM name from the program so the
            // instrument still survives to LilyPond (which carries only a name).
            info.midi_instrument = gm.to_string();
        }
    }

    info
}

fn parse_part_group(pg: &mxml::PartGroup) -> PartGroup {
    let mut group = PartGroup::new("StaffGroup");

    if let Some(ref name) = pg.content.group_name {
        group.name = name.content.clone();
    }
    if let Some(ref sym) = pg.content.group_symbol {
        use musicxml::datatypes::GroupSymbolValue as G;
        let (bracket, group_type) = match sym.content {
            G::Brace => ("brace", "PianoStaff"),
            G::Bracket => ("bracket", "StaffGroup"),
            G::Line => ("line", "ChoirStaff"),
            G::Square => ("square", "StaffGroup"),
            G::None => ("none", "StaffGroup"),
        };
        group.bracket = bracket.to_string();
        group.group_type = group_type.to_string();
    }
    if let Some(ref num) = pg.attributes.number {
        group.number = num.0.parse().unwrap_or(1);
    }

    group
}
