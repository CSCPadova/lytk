//! Score-level IR nodes — the root of the IR tree.
//!
//! # Influences
//! - `Score`, `PartGroup`, `ScoreMetadata` from lytk-py (`lytk-py/ir/score.py`).
//! - PartGroup nesting model from lytk-py (ChoirStaff, PianoStaff, etc.).

use std::collections::HashMap;

use super::serde_defaults::{is_default, is_one, one};
use serde::{Deserialize, Serialize};

use super::duration::Duration;
use super::language::{PitchLanguage, PitchMode};
use super::music::ContextType;
use super::part::Part;

/// Score identification and metadata.
///
/// From lytk-py's `ScoreMetadata` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScoreMetadata {
    #[serde(default, skip_serializing_if = "is_default")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub arranger: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub lyricist: Option<String>,
    /// Rights / copyright entries: (type, text).
    #[serde(default, skip_serializing_if = "is_default")]
    pub rights: Vec<(String, String)>,
    /// Additional metadata fields.
    #[serde(default, skip_serializing_if = "is_default")]
    pub extra: HashMap<String, String>,
    /// LilyPond note-name language. `None` means unset (defaults to Nederlands).
    #[serde(default, skip_serializing_if = "is_default")]
    pub pitch_language: Option<PitchLanguage>,
    /// Pitch-entry mode for LilyPond emission.
    #[serde(default, skip_serializing_if = "is_default")]
    pub pitch_mode: PitchMode,
    /// Anacrusis / pickup duration (emitted as `\partial <dur>`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub partial_duration: Option<Duration>,
    /// The LilyPond version the source's `\version` states (`2.24.0`), when
    /// the score was read from LilyPond that states a valid one.
    #[serde(default, skip_serializing_if = "is_default")]
    pub lilypond_version: Option<String>,
}

/// Page layout dimensions (all measurements in cm, staff-size in points).
///
/// Derived from MusicXML `<defaults>` / `<page-layout>` / `<system-layout>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageLayout {
    /// Page height in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub page_height: Option<f64>,
    /// Page width in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub page_width: Option<f64>,
    /// Left margin in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub left_margin: Option<f64>,
    /// Right margin in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub right_margin: Option<f64>,
    /// Top margin in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub top_margin: Option<f64>,
    /// Bottom margin in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub bottom_margin: Option<f64>,
    /// Distance between systems in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub system_distance: Option<f64>,
    /// Distance from top margin to first system in cm.
    #[serde(default, skip_serializing_if = "is_default")]
    pub top_system_distance: Option<f64>,
    /// Staff size in points.
    #[serde(default, skip_serializing_if = "is_default")]
    pub staff_size: Option<f64>,
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub metadata: ScoreMetadata,
    /// Page layout dimensions and staff sizing.
    #[serde(default, skip_serializing_if = "is_default")]
    pub page_layout: Option<PageLayout>,
    /// Direct children: parts and/or part groups.
    pub children: Vec<ScoreChild>,
}

impl Score {
    pub fn new() -> Self {
        Self {
            metadata: ScoreMetadata::default(),
            page_layout: None,
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

named_enum! {
    /// How a part group is bracketed (MusicXML's `<group-symbol>`).
    #[derive(Default)]
    pub enum GroupSymbol {
        #[default]
        Bracket => "bracket",
        Brace => "brace",
        Line => "line",
        Square => "square",
        NoSymbol => "none",
    }
}

/// A group of parts (StaffGroup, ChoirStaff, PianoStaff, etc.).
///
/// From lytk-py's `PartGroup(IRNode)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartGroup {
    /// Display name for the group.
    #[serde(default, skip_serializing_if = "is_default")]
    pub name: String,
    /// The LilyPond context: StaffGroup, ChoirStaff, PianoStaff, GrandStaff.
    pub group_type: ContextType,
    /// How the group is bracketed.
    #[serde(default, skip_serializing_if = "is_default")]
    pub bracket: GroupSymbol,
    /// Group number (from MusicXML).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub number: u8,
    /// Children: parts or nested part groups.
    pub children: Vec<ScoreChild>,
}

impl PartGroup {
    pub fn new(group_type: ContextType) -> Self {
        Self {
            name: String::new(),
            group_type,
            bracket: GroupSymbol::Bracket,
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
            self.group_type.ly_name(),
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
        let mut pg = PartGroup::new(ContextType::PianoStaff);
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
