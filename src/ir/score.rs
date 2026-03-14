//! Score-level IR nodes — the root of the IR tree.
//!
//! # Influences
//! - `Score`, `PartGroup`, `ScoreMetadata` from lytk-py (`lytk-py/ir/score.py`).
//! - PartGroup nesting model from lytk-py (ChoirStaff, PianoStaff, etc.).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::language::{PitchLanguage, PitchMode};
use super::part::Part;

/// Score identification and metadata.
///
/// From lytk-py's `ScoreMetadata` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScoreMetadata {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub composer: Option<String>,
    pub arranger: Option<String>,
    pub lyricist: Option<String>,
    /// Rights / copyright entries: (type, text).
    pub rights: Vec<(String, String)>,
    /// Additional metadata fields.
    pub extra: HashMap<String, String>,
    /// LilyPond note-name language. `None` means unset (defaults to Nederlands).
    pub pitch_language: Option<PitchLanguage>,
    /// Pitch-entry mode for LilyPond emission.
    pub pitch_mode: PitchMode,
}

/// An entry in a score's child list: either a bare Part or a PartGroup.
///
/// Replaces the dynamic-dispatch `IRNode` children list from the Python
/// prototype with a typed Rust enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScoreChild {
    Part(Part),
    PartGroup(PartGroup),
}

/// Root of the IR tree.
///
/// From lytk-py's `Score(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub metadata: ScoreMetadata,
    /// Direct children: parts and/or part groups.
    pub children: Vec<ScoreChild>,
}

impl Score {
    pub fn new() -> Self {
        Self {
            metadata: ScoreMetadata::default(),
            children: Vec::new(),
        }
    }

    /// Return an iterator over all parts (including those nested in groups).
    pub fn parts(&self) -> Vec<&Part> {
        let mut result = Vec::new();
        for child in &self.children {
            match child {
                ScoreChild::Part(p) => result.push(p),
                ScoreChild::PartGroup(g) => g.collect_parts(&mut result),
            }
        }
        result
    }

    /// Return an iterator over all parts (mutable).
    pub fn parts_mut(&mut self) -> Vec<&mut Part> {
        let mut result = Vec::new();
        for child in &mut self.children {
            match child {
                ScoreChild::Part(p) => result.push(p),
                ScoreChild::PartGroup(g) => g.collect_parts_mut(&mut result),
            }
        }
        result
    }
}

impl Default for Score {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let title = self.metadata.title.as_deref().unwrap_or("untitled");
        write!(f, "<Score {:?} parts={}>", title, self.parts().len())
    }
}

/// A group of parts (StaffGroup, ChoirStaff, PianoStaff, etc.).
///
/// From lytk-py's `PartGroup(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartGroup {
    /// Display name for the group.
    pub name: String,
    /// LilyPond context type: StaffGroup, ChoirStaff, PianoStaff, etc.
    pub group_type: String,
    /// Bracket style: bracket, brace, line, square, none.
    pub bracket: String,
    /// Group number (from MusicXML).
    pub number: u8,
    /// Children: parts or nested part groups.
    pub children: Vec<ScoreChild>,
}

impl PartGroup {
    pub fn new(group_type: &str) -> Self {
        Self {
            name: String::new(),
            group_type: group_type.to_string(),
            bracket: "bracket".to_string(),
            number: 1,
            children: Vec::new(),
        }
    }

    fn collect_parts<'a>(&'a self, out: &mut Vec<&'a Part>) {
        for child in &self.children {
            match child {
                ScoreChild::Part(p) => out.push(p),
                ScoreChild::PartGroup(g) => g.collect_parts(out),
            }
        }
    }

    fn collect_parts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Part>) {
        for child in &mut self.children {
            match child {
                ScoreChild::Part(p) => out.push(p),
                ScoreChild::PartGroup(g) => g.collect_parts_mut(out),
            }
        }
    }
}

impl std::fmt::Display for PartGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<PartGroup {:?} children={}>",
            self.group_type,
            self.children.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_default() {
        let s = Score::new();
        assert!(s.parts().is_empty());
        assert!(s.metadata.title.is_none());
    }

    #[test]
    fn score_with_parts() {
        let mut s = Score::new();
        s.metadata.title = Some("Test".to_string());
        s.children.push(ScoreChild::Part(Part::new("P1")));
        s.children.push(ScoreChild::Part(Part::new("P2")));
        assert_eq!(s.parts().len(), 2);
    }

    #[test]
    fn score_nested_part_group() {
        let mut s = Score::new();
        let mut pg = PartGroup::new("PianoStaff");
        pg.children.push(ScoreChild::Part(Part::new("P1")));
        pg.children.push(ScoreChild::Part(Part::new("P2")));
        s.children.push(ScoreChild::PartGroup(pg));
        // parts() should flatten into the group
        assert_eq!(s.parts().len(), 2);
    }

    #[test]
    fn score_display() {
        let mut s = Score::new();
        s.metadata.title = Some("Sonata".to_string());
        let display = format!("{}", s);
        assert!(display.contains("Sonata"));
    }
}
