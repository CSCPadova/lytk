//! Part-level IR node.
//!
//! A part represents a single instrument (or instrument group like piano) and
//! contains a sequence of measures.
//!
//! # Influences
//! - Field set from lytk-py's `Part(IRNode)` (`lytk-py/ir/part.py`).
//! - MIDI metadata fields from lytk-py.

use serde::{Deserialize, Serialize};

use super::measure::Measure;

/// A single instrument part. Holds a sequence of [`Measure`]s.
///
/// From lytk-py's `Part(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// Full instrument name.
    pub name: String,
    /// Abbreviated instrument name.
    pub abbreviation: String,
    /// Unique part identifier (from MusicXML `<part>` id attribute).
    pub part_id: String,
    /// MIDI instrument name.
    pub midi_instrument: String,
    /// MIDI channel number (0-based).
    pub midi_channel: u8,
    /// MIDI program number.
    pub midi_program: u8,
    /// Number of staves for this part (e.g. 2 for piano).
    pub staves: u8,
    /// Ordered sequence of measures.
    pub measures: Vec<Measure>,
}

impl Part {
    pub fn new(part_id: &str) -> Self {
        Self {
            name: String::new(),
            abbreviation: String::new(),
            part_id: part_id.to_string(),
            midi_instrument: String::new(),
            midi_channel: 0,
            midi_program: 0,
            staves: 1,
            measures: Vec::new(),
        }
    }
}

impl std::fmt::Display for Part {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<Part {:?} id={:?} measures={}>",
            self.name,
            self.part_id,
            self.measures.len()
        )
    }
}
