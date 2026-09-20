//! # lytk — fast symbolic-music conversion & augmentation
//!
//! lytk converts between **LilyPond**, **MusicXML/MXL**, **MIDI** and **ABC**
//! through a shared Internal Representation (IR), applies composable transforms
//! (transpose, invert, retrograde, language change), and emits ML
//! representations (note-array, event-sequence, piano-roll). It ships as a Rust
//! crate plus a CLI, and builds the `lytk._core` extension module that backs
//! the `lytk` package on PyPI.
//!
//! ## Architecture
//! - [`ir`] — the IR: a measure-based [`Score`] (Layer 2) and a recursive
//!   [`MusicDocument`] music tree (Layer 1), bridged by lift/lower.
//! - [`adapters`] — per-format readers/writers ([`ToIrAdapter`]/[`FromIrAdapter`]).
//! - [`transforms`] — composable IR passes.
//! - [`representations`] — note-array / event-sequence / piano-roll encoders.
//!
//! ## Rust quickstart
//! ```no_run
//! use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
//! use _core::adapters::ir_to_ly::IrToLyAdapter;
//! use _core::adapters::{ToIrAdapter, FromIrAdapter};
//!
//! let score = MxmlToIrAdapter::new().convert_file("in.musicxml".as_ref())?;
//! let lilypond = IrToLyAdapter::new().convert(&score)?;
//! # Ok::<(), _core::adapters::AdapterError>(())
//! ```
//!
//! The most-used types are re-exported at the crate root: [`Score`] and
//! [`MusicDocument`].

pub mod adapters;
pub mod ir;
pub mod parser;
pub mod representations;
pub mod transforms;

// Re-export the core IR types at the crate root for Rust consumers.
pub use ir::music::MusicDocument;
pub use ir::Score;

// The PyO3 bindings that make up the `lytk._core` extension module.
mod navigation;
// pyo3 proc macros wrap return values with `Into::into()` on the error path,
// which clippy flags as a no-op when the error is already `PyErr`. Scoped to
// this module: as a crate-level allow it also silenced the rest of the crate,
// where it was hiding four real hits.
#[allow(clippy::useless_conversion)]
mod python;
