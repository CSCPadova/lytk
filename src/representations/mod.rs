//! ML representations of symbolic music (Epic D), modeled on `muspy`.
//!
//! These convert between the Layer-1 [`MusicDocument`](crate::ir::music::MusicDocument)
//! Music tree and dense/array encodings suitable for machine learning:
//!
//! - [`note_array`] — the note-based representation: a flat list of
//!   `(onset, duration, pitch, velocity)` rows in fixed time-step units.
//!
//! (Event-sequence and piano-roll representations follow in EDT2/EDT3.)
//!
//! Representations go through the **Music tree**, not the measure-based Score:
//! it is format-agnostic and reuses the exact [`Frac`](crate::ir::duration::Frac)
//! durations, so timing is exact up to the chosen resolution.

pub mod note_array;

pub use note_array::{from_note_array, to_note_array, NoteArray, NoteRow, DEFAULT_RESOLUTION};
