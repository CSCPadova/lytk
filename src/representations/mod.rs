//! ML representations of symbolic music (Epic D), modeled on `muspy`.
//!
//! These convert between the Layer-1 [`MusicDocument`](crate::ir::music::MusicDocument)
//! Music tree and dense/array encodings suitable for machine learning:
//!
//! - [`note_array`] — the note-based representation: a flat list of
//!   `(onset, duration, pitch, velocity)` rows in fixed time-step units.
//! - [`event_sequence`] — the event-based representation: a flat sequence of
//!   note-on/note-off/time-shift/velocity event codes (Performance-RNN style).
//! - [`piano_roll`] — the piano-roll representation: a dense `T × 128` matrix.
//!
//! Representations go through the **Music tree**, not the measure-based Score:
//! it is format-agnostic and reuses the exact [`Frac`](crate::ir::duration::Frac)
//! durations, so timing is exact up to the chosen resolution. The event and
//! piano-roll representations build on the note-array as their timed form.

pub mod event_sequence;
pub mod metrics;
pub mod note_array;
pub mod piano_roll;

pub use event_sequence::{
    from_event_sequence, to_event_sequence, EventOptions, EventSequence, DEFAULT_MAX_TIME_SHIFT,
    DEFAULT_VELOCITY_BINS,
};
pub use note_array::{from_note_array, to_note_array, NoteArray, NoteRow, DEFAULT_RESOLUTION};
pub use piano_roll::{from_piano_roll, to_piano_roll, PianoRoll, PITCH_COUNT};
