//! Voice-level IR node.
//!
//! A voice groups note/rest/chord elements that share a single rhythmic
//! stream within a measure.
//!
//! # Influences
//! - From lytk-py's `Voice(IRNode)` (`lytk-py/ir/voice.py`).

use serde::{Deserialize, Serialize};

use super::note::VoiceElement;

/// A voice within a measure. Holds a sequence of [`VoiceElement`]s.
///
/// From lytk-py's `Voice(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    /// Voice number (1-based, matches MusicXML `<voice>` element).
    pub number: u8,
    /// Ordered sequence of notes, rests, and chords.
    pub elements: Vec<VoiceElement>,
}

impl Voice {
    pub fn new(number: u8) -> Self {
        Self {
            number,
            elements: Vec::new(),
        }
    }
}

impl std::fmt::Display for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<Voice {} elements={}>",
            self.number,
            self.elements.len()
        )
    }
}
