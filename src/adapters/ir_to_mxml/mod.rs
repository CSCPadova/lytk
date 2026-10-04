//! IR → MusicXML emitter.
//!
//! Converts an IR [`Score`] into MusicXML 4.0 `<score-partwise>` XML,
//! using the `musicxml` crate for typed struct construction and serialization.

mod direction;
mod helpers;
mod note;
mod part;
mod score;
#[cfg(test)]
mod tests;

use std::path::Path;

use crate::ir::score::Score;

use super::{AdapterError, FromIrAdapter, Result};

use helpers::compute_score_divisions;
use musicxml_internal::{ElementSerializer, XmlElement};

/// Default MusicXML divisions per quarter note.
const DEFAULT_DIVISIONS: u16 = 4;

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// Converts an IR [`Score`] to MusicXML.
pub struct IrToMxmlAdapter {
    /// MusicXML version string.
    version: String,
    /// Divisions per quarter note.
    divisions: u16,
}

impl IrToMxmlAdapter {
    pub fn new() -> Self {
        Self {
            version: "4.0".to_string(),
            divisions: DEFAULT_DIVISIONS,
        }
    }

    pub fn with_divisions(mut self, divisions: u16) -> Self {
        self.divisions = divisions;
        self
    }

    /// Build the MusicXML score tree, auto-computing divisions that accommodate
    /// every tuplet ratio in the score. Shared by `convert` and `write`.
    fn build(&self, score: &Score) -> musicxml::elements::ScorePartwise {
        let effective_divisions = compute_score_divisions(score, self.divisions);
        let adapter = Self {
            version: self.version.clone(),
            divisions: effective_divisions,
        };
        // MusicXML spells stems and beams out: what the source left to the
        // engraver is engraved here.
        let mut score = score.clone();
        crate::ir::beams::engrave(&mut score);
        // A note's marks are written just before it, in its voice.
        crate::ir::marks::sink(&mut score);
        adapter.build_score_partwise(&score)
    }
}

impl Default for IrToMxmlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl IrToMxmlAdapter {
    /// Serialize to compressed MXL bytes: a ZIP with `META-INF/container.xml`
    /// pointing at the score. Zipped from `convert()`'s string so fractional
    /// alters (microtones) are decoded, unlike the crate's internal writer.
    pub fn convert_mxl_bytes(&self, score: &Score) -> Result<Vec<u8>> {
        let xml = FromIrAdapter::convert(self, score)?;
        let container = r#"<?xml version="1.0" encoding="UTF-8"?>
<container>
  <rootfiles>
    <rootfile full-path="score.xml" media-type="application/vnd.recordare.musicxml+xml"/>
  </rootfiles>
</container>
"#;
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default();
            let write = |zw: &mut zip::ZipWriter<_>, name: &str, data: &[u8]| -> Result<()> {
                zw.start_file(name, opts)
                    .and_then(|()| std::io::Write::write_all(zw, data).map_err(Into::into))
                    .map_err(|e| AdapterError::Parse(format!("MXL write failed: {e}")))
            };
            write(&mut zw, "META-INF/container.xml", container.as_bytes())?;
            write(&mut zw, "score.xml", xml.as_bytes())?;
            zw.finish()
                .map_err(|e| AdapterError::Parse(format!("MXL write failed: {e}")))?;
        }
        Ok(buf)
    }
}

impl FromIrAdapter for IrToMxmlAdapter {
    fn convert(&self, score: &Score) -> Result<String> {
        let mxml_score = self.build(score);
        let tree = <musicxml::elements::ScorePartwise as ElementSerializer>::serialize(&mxml_score);
        Ok(crate::adapters::decode_fractional_alters(render_xml(&tree)))
    }

    fn write(&self, score: &Score, path: &Path) -> Result<()> {
        // Detect MXL (compressed) vs plain XML by extension
        let compressed = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mxl"))
            .unwrap_or(false);

        if compressed {
            std::fs::write(path, self.convert_mxl_bytes(score)?)?;
        } else {
            // Plain XML goes through convert() so fractional alters decode.
            std::fs::write(path, self.convert(score)?)?;
        }

        Ok(())
    }
}

/// A score tree as MusicXML text, laid out as the `musicxml` crate lays it
/// out, every text and attribute value escaped: the crate writes them raw,
/// so a `&` in a title made XML that no parser reads.
fn render_xml(xml: &XmlElement) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(
        "<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \
         \"http://www.musicxml.org/dtds/partwise.dtd\">\n",
    );
    render_element(&mut out, xml, 0);
    out
}

fn render_element(out: &mut String, xml: &XmlElement, depth: usize) {
    let indent = |out: &mut String| (0..depth).for_each(|_| out.push_str("  "));
    if depth > 0 {
        out.push('\n');
    }
    indent(out);
    out.push('<');
    out.push_str(&xml.name);
    for (key, value) in &xml.attributes {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        escape_xml(out, value, true);
        out.push('"');
    }
    if xml.elements.is_empty() && xml.text.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for element in &xml.elements {
        render_element(out, element, depth + 1);
    }
    if xml.text.is_empty() {
        out.push('\n');
        indent(out);
    } else {
        escape_xml(out, &xml.text, false);
    }
    out.push_str("</");
    out.push_str(&xml.name);
    out.push('>');
}

fn escape_xml(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

impl super::FromMusicAdapter for IrToMxmlAdapter {
    fn convert_music(&self, doc: &crate::ir::music::MusicDocument) -> Result<String> {
        let score = crate::ir::lower::lower_to_score(doc);
        self.convert(&score)
    }
}
