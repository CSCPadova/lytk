//! Format adapters — convert between external formats and the lytk IR.
//!
//! Every adapter implements [`ToIrAdapter`] (parse a format into IR) or
//! [`FromIrAdapter`] (emit IR into a format), or both.
//!
//! MusicXML and LilyPond adapters are always compiled. Optional adapters
//! (MIDI, ABC) are gated behind Cargo feature flags.

use std::path::Path;

use thiserror::Error;

use crate::ir::Score;

pub mod ir_to_ly;
pub mod mxl_zip;
pub mod mxml_to_ir;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by format adapters.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("XML attribute error: {0}")]
    XmlAttr(#[from] quick_xml::events::attributes::AttrError),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

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

/// Adapter that reads a format and produces an IR [`Score`].
pub trait ToIrAdapter {
    /// Parse a file (path) into an IR score.
    fn convert_file(&self, path: &Path) -> Result<Score>;

    /// Parse an in-memory string into an IR score.
    fn convert_str(&self, text: &str) -> Result<Score>;
}

/// Adapter that takes an IR [`Score`] and emits a format.
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
