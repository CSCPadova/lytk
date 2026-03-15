//! Internal Representation (IR) for music scores.
//!
//! The IR is the central hub of lytk. All format adapters translate to/from IR.
//!
//! # Design
//!
//! Two categories of types:
//! - **Tree nodes** (`Score`, `Part`, `Measure`, `Voice`, `Note`, …): mutable structs with
//!   parent/child links forming a tree. Nodes own their children.
//! - **Value objects** (`Pitch`, `Duration`, `KeySignature`, …): small `Clone + PartialEq`
//!   structs, often `Copy`. Stored as plain fields on nodes, never as children.
//!
//! # Tree hierarchy
//! ```text
//! Score
//!  ├── PartGroup?
//!  │    └── Part
//!  └── Part
//!       └── Measure
//!            └── Voice
//!                 ├── Note
//!                 ├── Rest
//!                 ├── Chord → [Note…]
//!                 ├── Forward
//!                 └── Backup
//! ```
//!
//! # Influences
//! - Tree structure and visitor pattern from lytk-py (Python prototype)
//! - Pitch representation (note 0–6, alter, octave) from quickly (python-ly successor)
//! - Conversion traits (Note ↔ MIDI ↔ LilyPond string) from lilypond-rs
//! - Integer time steps and annotation model from PDMX

pub mod articulation;
pub mod direction;
pub mod duration;
pub mod harmony;
pub mod language;
pub mod measure;
pub mod note;
pub mod part;
pub mod pitch;
pub mod score;
pub mod voice;

// Re-export all public types for convenience.
pub use articulation::*;
pub use direction::*;
pub use duration::Duration;
pub use harmony::*;
pub use language::{PitchLanguage, PitchMode};
pub use measure::*;
pub use note::*;
pub use part::Part;
pub use pitch::*;
pub use score::*;
pub use voice::Voice;
