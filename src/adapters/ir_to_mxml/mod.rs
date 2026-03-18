//! IR → MusicXML emitter.
//!
//! Converts an IR [`Score`] into MusicXML 4.0 `<score-partwise>` XML.
//!
//! # Reference
//! Ported from the Python prototype `lytk-py/converters/ir_to_mxml.py`.

mod direction;
mod helpers;
mod note;
mod part;
mod score;
#[cfg(test)]
mod tests;

use std::io::Cursor;

use quick_xml::events::{BytesDecl, Event};
use quick_xml::Writer;

use crate::ir::score::Score;

use super::{FromIrAdapter, Result};

use helpers::compute_score_divisions;

/// Default MusicXML divisions per quarter note.
const DEFAULT_DIVISIONS: u16 = 4;

// ---------------------------------------------------------------------------
// Writer type alias
// ---------------------------------------------------------------------------

type W = Writer<Cursor<Vec<u8>>>;

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
}

impl Default for IrToMxmlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIrAdapter for IrToMxmlAdapter {
    fn convert(&self, score: &Score) -> Result<String> {
        // Auto-compute divisions that accommodate all tuplet ratios in the score
        let effective_divisions = compute_score_divisions(score, self.divisions);
        let adapter = Self {
            version: self.version.clone(),
            divisions: effective_divisions,
        };

        let buf = Cursor::new(Vec::new());
        let mut w = Writer::new_with_indent(buf, b' ', 2);

        // XML declaration
        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        adapter.write_score(&mut w, score)?;

        let bytes = w.into_inner().into_inner();
        Ok(String::from_utf8(bytes).expect("XML output is valid UTF-8"))
    }
}

impl super::FromMusicAdapter for IrToMxmlAdapter {
    fn convert_music(
        &self,
        doc: &crate::ir::music::MusicDocument,
    ) -> Result<String> {
        let score = crate::ir::lower::lower_to_score(doc);
        self.convert(&score)
    }
}
