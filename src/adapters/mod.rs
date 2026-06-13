//! Format adapters — convert between external formats and the lytk IR.
//!
//! Every adapter implements [`ToIrAdapter`] (parse a format into IR) or
//! [`FromIrAdapter`] (emit IR into a format), or both.

use std::path::Path;

use thiserror::Error;

use crate::ir::music::MusicDocument;
use crate::ir::Score;

pub mod abc_to_ir;
pub mod dynamics_velocity;
pub mod ir_to_abc;
pub mod ir_to_ly;
pub mod ir_to_mxml;
pub mod ly_flatten;
pub mod ly_to_ir;
pub mod mxml_to_ir;

pub mod ir_to_midi;
pub mod midi_to_ir;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by format adapters.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing required element: {0}")]
    MissingElement(String),

    #[error("invalid value: {field}: {value}")]
    InvalidValue { field: String, value: String },

    #[error("unsupported feature: {0}")]
    Unsupported(String),

    #[error("parse error: {0}")]
    Parse(String),
}

/// Adapter result type.
pub type Result<T> = std::result::Result<T, AdapterError>;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Adapter that reads a format and produces an IR [`Score`] (Layer 2).
pub trait ToIrAdapter {
    /// Parse a file (path) into an IR score.
    fn convert_file(&self, path: &Path) -> Result<Score>;

    /// Parse an in-memory string into an IR score.
    fn convert_str(&self, text: &str) -> Result<Score>;
}

/// Adapter that takes an IR [`Score`] (Layer 2) and emits a format.
pub trait FromIrAdapter {
    /// Render a score to a format string.
    fn convert(&self, score: &Score) -> Result<String>;

    /// Render a score and write it to a file.
    fn write(&self, score: &Score, path: &Path) -> Result<()> {
        let output = self.convert(score)?;
        std::fs::write(path, output)?;
        Ok(())
    }
}

/// Adapter that reads a format and produces a [`MusicDocument`] (Layer 1).
///
/// This is the preferred trait for format parsers in the new two-layer architecture.
/// Parsers that implement `ToIrAdapter` can get a default implementation via the
/// lift pass (`Score → Music`).
pub trait ToMusicAdapter {
    /// Parse a file (path) into a Music tree.
    fn convert_file_to_music(&self, path: &Path) -> Result<MusicDocument>;

    /// Parse an in-memory string into a Music tree.
    fn convert_str_to_music(&self, text: &str) -> Result<MusicDocument>;
}

/// Adapter that takes a [`MusicDocument`] (Layer 1) and emits a format.
///
/// This is the preferred trait for format emitters in the new two-layer architecture.
/// Emitters that implement `FromIrAdapter` can get a default implementation via the
/// lower pass (`Music → Score`).
pub trait FromMusicAdapter {
    /// Render a Music tree to a format string.
    fn convert_music(&self, doc: &MusicDocument) -> Result<String>;

    /// Render a Music tree and write it to a file.
    fn write_music(&self, doc: &MusicDocument, path: &Path) -> Result<()> {
        let output = self.convert_music(doc)?;
        std::fs::write(path, output)?;
        Ok(())
    }
}
