//! Lower a Music tree (Layer 1) to a Score layout (Layer 2).
//!
//! This pass walks the Music tree, tracking cumulative time, and produces
//! the measure-based `Score → Part → Measure → Voice → VoiceElement` structure
//! needed by the MusicXML and MIDI exporters.
//!
//! # Algorithm
//! 1. Collect staff contexts from the Music tree into `StaffBuilder`s.
//! 2. Each `StaffBuilder` accumulates timed events (notes, rests, attribute changes).
//! 3. After collection, split events into measures based on time signatures.
//! 4. Assign voice numbers for simultaneous content.
//! 5. Build the Part/PartGroup hierarchy from the context tree.

mod build;
mod state;
mod walk;

#[cfg(test)]
mod tests;

use super::music::{Music, MusicDocument};
use super::score::*;
use state::LowerState;
use walk::walk_music;
use build::build_score;

/// Convert a `MusicDocument` to a `Score`.
pub fn lower_to_score(doc: &MusicDocument) -> Score {
    let mut state = LowerState::new(doc.metadata.clone());
    walk_music(&doc.music, &mut state);
    build_score(&mut state)
}

/// Convert a bare `Music` tree to a `Score` with default metadata.
pub fn lower_music_to_score(music: &Music) -> Score {
    let mut state = LowerState::new(ScoreMetadata::default());
    walk_music(music, &mut state);
    build_score(&mut state)
}
