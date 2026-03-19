//! Music tree — the Layer 1 internal representation.
//!
//! A tree of music expressions inspired by LilyPond's internal model.
//! This is what parsers produce and what transforms operate on.
//! Unlike the Layer 2 IR (Score/Part/Measure/Voice), the Music tree
//! has no measures — it represents music as nested sequential and
//! simultaneous containers with leaf events.
//!
//! # Hierarchy
//! ```text
//! Music::Context(Score)
//!  └── Music::Simultaneous
//!       ├── Music::Context(PianoStaff)
//!       │    └── Music::Simultaneous
//!       │         ├── Music::Context(Staff)
//!       │         │    └── Music::Sequential [notes, rests, time sigs, ...]
//!       │         ├── Music::Context(Dynamics)
//!       │         │    └── Music::Sequential [skips with pedal/dynamic annotations]
//!       │         └── Music::Context(Staff)
//!       │              └── Music::Sequential [notes, rests, ...]
//!       └── Music::Context(Staff)
//!            └── Music::Sequential [notes, rests, ...]
//! ```
//!
//! # Lowering
//! Use [`super::lower::lower_to_score`] to convert a Music tree to the
//! Layer 2 `Score` structure for MusicXML/MIDI export.

use super::annotation::Annotation;
use super::direction::{Barline, TempoDirection};
use super::duration::Duration;
use super::harmony::{FiguredBass, Harmony};
use super::measure::{Clef, KeySignature, TimeSignature};
use super::pitch::Pitch;
use super::score::ScoreMetadata;
use serde::{Deserialize, Serialize};

/// The type of a LilyPond context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextType {
    Score,
    StaffGroup,
    ChoirStaff,
    PianoStaff,
    GrandStaff,
    Staff,
    Voice,
    Dynamics,
    Lyrics,
    TabStaff,
    TabVoice,
    FiguredBass,
    ChordNames,
}

impl ContextType {
    /// Parse a context type name from a LilyPond string.
    pub fn from_ly_name(name: &str) -> Option<Self> {
        match name {
            "Score" => Some(Self::Score),
            "StaffGroup" => Some(Self::StaffGroup),
            "ChoirStaff" => Some(Self::ChoirStaff),
            "PianoStaff" => Some(Self::PianoStaff),
            "GrandStaff" => Some(Self::GrandStaff),
            "Staff" => Some(Self::Staff),
            "Voice" => Some(Self::Voice),
            "Dynamics" => Some(Self::Dynamics),
            "Lyrics" => Some(Self::Lyrics),
            "TabStaff" => Some(Self::TabStaff),
            "TabVoice" => Some(Self::TabVoice),
            "FiguredBass" => Some(Self::FiguredBass),
            "ChordNames" => Some(Self::ChordNames),
            _ => None,
        }
    }

    /// The LilyPond context name.
    pub fn ly_name(&self) -> &'static str {
        match self {
            Self::Score => "Score",
            Self::StaffGroup => "StaffGroup",
            Self::ChoirStaff => "ChoirStaff",
            Self::PianoStaff => "PianoStaff",
            Self::GrandStaff => "GrandStaff",
            Self::Staff => "Staff",
            Self::Voice => "Voice",
            Self::Dynamics => "Dynamics",
            Self::Lyrics => "Lyrics",
            Self::TabStaff => "TabStaff",
            Self::TabVoice => "TabVoice",
            Self::FiguredBass => "FiguredBass",
            Self::ChordNames => "ChordNames",
        }
    }

    /// True if this context type groups multiple staves (piano, organ, etc.)
    pub fn is_staff_group(&self) -> bool {
        matches!(
            self,
            Self::StaffGroup | Self::ChoirStaff | Self::PianoStaff | Self::GrandStaff
        )
    }
}

/// Repeat type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RepeatType {
    /// Volta repeat with repeat barlines and endings.
    Volta,
    /// Unfold repeat — written out in full.
    Unfold,
    /// Percent repeat (single-measure repeat sign).
    Percent,
    /// Tremolo repeat between two notes.
    Tremolo,
}

/// A music expression — the core node of the Layer 1 IR.
///
/// Music is a recursive tree where containers (`Sequential`, `Simultaneous`,
/// `Context`) hold children and leaf nodes represent musical events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Music {
    // ── Containers ──────────────────────────────────────────
    /// Notes played one after another: `{ c4 d e f }`
    Sequential(Vec<Music>),

    /// Notes played at the same time: `<< { } \\ { } >>`
    Simultaneous(Vec<Music>),

    /// A named context: `\new Staff = "rh" { ... }`
    Context {
        context_type: ContextType,
        name: Option<String>,
        content: Box<Music>,
    },

    // ── Pitched events ──────────────────────────────────────
    /// A single note with pitch, duration, and annotations.
    Note {
        pitch: Pitch,
        duration: Duration,
        annotations: Vec<Annotation>,
    },

    /// A chord: `<c e g>4`
    Chord {
        /// Each pitch with its per-note annotations (fingering, accidental).
        pitches: Vec<(Pitch, Vec<Annotation>)>,
        duration: Duration,
        /// Chord-level annotations (arpeggio, staccato, etc.)
        annotations: Vec<Annotation>,
    },

    // ── Unpitched events ────────────────────────────────────
    /// A visible rest.
    Rest {
        duration: Duration,
        is_measure_rest: bool,
    },

    /// An invisible spacer rest (LilyPond `s` or `\skip`).
    Skip { duration: Duration },

    // ── Attribute events ────────────────────────────────────
    /// Time signature change (not a container — just an event).
    TimeSignature(TimeSignature),

    /// Key signature change.
    KeySignature(KeySignature),

    /// Clef change.
    Clef(Clef),

    /// Tempo marking.
    Tempo(TempoDirection),

    /// Barline (explicit bar check `|` or special barline `\bar "||"`).
    Barline(Barline),

    // ── Directions ──────────────────────────────────────────
    /// A standalone direction not attached to a note (dynamics, pedal, text, etc.)
    /// In most cases, annotations on `Note`/`Chord` are preferred.
    /// This variant is for directions that appear between notes.
    Direction(Box<super::direction::Direction>),

    // ── Wrappers ────────────────────────────────────────────
    /// Grace notes: `\grace { c16 d }` or `\acciaccatura c16`
    Grace {
        content: Box<Music>,
        /// True for acciaccatura (slashed), false for appoggiatura.
        slash: bool,
    },

    /// Tuplet: `\tuplet 3/2 { c4 d e }`
    Tuplet {
        normal: u8,
        actual: u8,
        content: Box<Music>,
    },

    /// Repeat: `\repeat volta 2 { ... } \alternative { ... }`
    Repeat {
        repeat_type: RepeatType,
        count: u16,
        body: Box<Music>,
        alternatives: Vec<Music>,
    },

    /// A resolved variable reference (keeps name for round-trip fidelity).
    Variable { name: String, content: Box<Music> },

    // ── Lyrics / Harmony / Figured Bass ─────────────────────
    /// A figured bass entry.
    FiguredBass(FiguredBass),

    /// A chord symbol / harmony.
    Harmony(Harmony),

    /// A lyric syllable.
    Lyric(super::articulation::LyricSyllable),
}

impl Music {
    /// Create an empty sequential container.
    pub fn empty() -> Self {
        Music::Sequential(Vec::new())
    }

    /// True if this is an empty Sequential with no children.
    pub fn is_empty(&self) -> bool {
        matches!(self, Music::Sequential(v) if v.is_empty())
    }

    /// Wrap this music in a context.
    pub fn in_context(self, context_type: ContextType, name: Option<String>) -> Self {
        Music::Context {
            context_type,
            name,
            content: Box::new(self),
        }
    }

    /// The duration of this music element (for leaf events).
    /// Returns None for containers — use `total_duration` for those.
    pub fn leaf_duration(&self) -> Option<&Duration> {
        match self {
            Music::Note { duration, .. }
            | Music::Chord { duration, .. }
            | Music::Rest { duration, .. }
            | Music::Skip { duration } => Some(duration),
            _ => None,
        }
    }

    /// Iterate over direct children (for containers).
    pub fn children(&self) -> &[Music] {
        match self {
            Music::Sequential(v) | Music::Simultaneous(v) => v,
            Music::Repeat { alternatives, .. } => alternatives,
            _ => &[],
        }
    }

    /// Mutable access to direct children (for containers).
    pub fn children_mut(&mut self) -> &mut Vec<Music> {
        match self {
            Music::Sequential(v) | Music::Simultaneous(v) => v,
            Music::Repeat { alternatives, .. } => alternatives,
            _ => panic!("children_mut called on non-container Music variant"),
        }
    }

    /// Access the inner content of wrapper variants (Context, Grace, Tuplet, Variable, Repeat body).
    pub fn inner(&self) -> Option<&Music> {
        match self {
            Music::Context { content, .. }
            | Music::Grace { content, .. }
            | Music::Tuplet { content, .. }
            | Music::Variable { content, .. }
            | Music::Repeat { body: content, .. } => Some(content),
            _ => None,
        }
    }
}

/// A complete music document with metadata and a music tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MusicDocument {
    /// Score metadata (title, composer, etc.)
    pub metadata: ScoreMetadata,
    /// The root music expression.
    pub music: Music,
}

impl MusicDocument {
    /// Create a new document with the given music tree.
    pub fn new(music: Music) -> Self {
        Self {
            metadata: ScoreMetadata::default(),
            music,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::duration::Frac;

    #[test]
    fn test_empty_music() {
        let m = Music::empty();
        assert!(m.is_empty());
    }

    #[test]
    fn test_sequential_note() {
        let note = Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        };
        let seq = Music::Sequential(vec![note]);
        assert!(!seq.is_empty());
        assert_eq!(seq.children().len(), 1);
    }

    #[test]
    fn test_context_wrapping() {
        let note = Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        };
        let staff = note.in_context(ContextType::Staff, Some("rh".to_string()));
        match &staff {
            Music::Context {
                context_type,
                name,
                content,
            } => {
                assert_eq!(*context_type, ContextType::Staff);
                assert_eq!(name.as_deref(), Some("rh"));
                assert!(matches!(content.as_ref(), Music::Note { .. }));
            }
            _ => panic!("Expected Context"),
        }
    }

    #[test]
    fn test_simultaneous_voices() {
        let v1 = Music::Sequential(vec![Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        }]);
        let v2 = Music::Sequential(vec![Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::E, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        }]);
        let sim = Music::Simultaneous(vec![v1, v2]);
        assert_eq!(sim.children().len(), 2);
    }

    #[test]
    fn test_leaf_duration() {
        let note = Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        };
        assert_eq!(note.leaf_duration().unwrap().base, Frac::new(1, 4));

        let seq = Music::Sequential(vec![]);
        assert!(seq.leaf_duration().is_none());
    }

    #[test]
    fn test_context_type_from_ly_name() {
        assert_eq!(
            ContextType::from_ly_name("PianoStaff"),
            Some(ContextType::PianoStaff)
        );
        assert_eq!(ContextType::from_ly_name("Voice"), Some(ContextType::Voice));
        assert_eq!(ContextType::from_ly_name("Unknown"), None);
    }

    #[test]
    fn test_music_document() {
        let doc = MusicDocument::new(Music::empty());
        assert!(doc.music.is_empty());
        assert!(doc.metadata.title.is_none());
    }

    #[test]
    fn test_tuplet() {
        let note = Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::C, 4),
            duration: Duration::eighth(),
            annotations: vec![],
        };
        let tuplet = Music::Tuplet {
            normal: 2,
            actual: 3,
            content: Box::new(Music::Sequential(vec![note.clone(), note.clone(), note])),
        };
        assert!(tuplet.inner().is_some());
    }

    #[test]
    fn test_grace() {
        let note = Music::Note {
            pitch: Pitch::new(crate::ir::pitch::PitchStep::C, 4),
            duration: Duration::sixteenth(),
            annotations: vec![],
        };
        let grace = Music::Grace {
            content: Box::new(note),
            slash: true,
        };
        match &grace {
            Music::Grace { slash, .. } => assert!(*slash),
            _ => panic!("Expected Grace"),
        }
    }
}
