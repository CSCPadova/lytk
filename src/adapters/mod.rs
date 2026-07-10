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
pub mod gm;
pub mod humdrum_to_ir;
pub mod ir_to_abc;
pub mod ir_to_humdrum;
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
///
/// `#[non_exhaustive]`: new variants may be added in future minor versions, so
/// downstream `match`es must include a wildcard arm.
#[derive(Debug, Error)]
#[non_exhaustive]
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

// ---------------------------------------------------------------------------
// Fractional-alter bridge (microtones through the `musicxml` crate)
// ---------------------------------------------------------------------------
//
// The `musicxml` crate types `<alter>` as an i16, but MusicXML allows decimal
// alters (quarter tones: 0.5, -1.5, …) — the crate silently drops any note
// carrying one. Bridge: before the crate parses, rewrite fractional alters to
// an out-of-band integer `1000 + alter·100` (0.5 → 1050); decode back to a
// `Ratio` in `extract_pitch`. Symmetrically on emission: write the encoded
// integer through the typed structs, then rewrite the serialized string back
// to the decimal. Real alters live in [-3, 3], so encoded values (700..=1300)
// are unambiguous.

pub(crate) const ALTER_ENC_BASE: i32 = 1000;
pub(crate) const ALTER_ENC_MIN: i32 = 700;
pub(crate) const ALTER_ENC_MAX: i32 = 1300;

/// Rewrite fractional `<alter>` contents in raw MusicXML to encoded integers.
pub(crate) fn encode_fractional_alters(xml: String) -> String {
    if !xml.contains("<alter>") {
        return xml;
    }
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<alter>") {
        let after = &rest[start + 7..];
        let Some(end) = after.find("</alter>") else {
            break;
        };
        let val = after[..end].trim();
        out.push_str(&rest[..start + 7]);
        match val.parse::<f64>() {
            Ok(f) if f.fract() != 0.0 && (-3.0..=3.0).contains(&f) => {
                out.push_str(&(ALTER_ENC_BASE + (f * 100.0).round() as i32).to_string());
            }
            _ => out.push_str(val),
        }
        out.push_str("</alter>");
        rest = &after[end + 8..];
    }
    out.push_str(rest);
    out
}

/// Rewrite encoded `<alter>` integers in serialized MusicXML back to decimals.
pub(crate) fn decode_fractional_alters(xml: String) -> String {
    if !xml.contains("<alter>") {
        return xml;
    }
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<alter>") {
        let after = &rest[start + 7..];
        let Some(end) = after.find("</alter>") else {
            break;
        };
        let val = after[..end].trim();
        out.push_str(&rest[..start + 7]);
        match val.parse::<i32>() {
            Ok(e) if (ALTER_ENC_MIN..=ALTER_ENC_MAX).contains(&e) => {
                out.push_str(&format!("{}", (e - ALTER_ENC_BASE) as f64 / 100.0));
            }
            _ => out.push_str(val),
        }
        out.push_str("</alter>");
        rest = &after[end + 8..];
    }
    out.push_str(rest);
    out
}

/// Normalize single-quoted XML attributes (`a='v'`) to double quotes: the
/// `musicxml` crate's tokenizer silently drops notes carrying single-quoted
/// attributes on nested elements (Sibelius exports, acid test 99a). Only
/// rewrites inside tags, so apostrophes in text content are untouched;
/// `<!…>` comment/doctype sections are skipped.
pub(crate) fn normalize_attribute_quotes(xml: String) -> String {
    if !xml.contains('\'') {
        return xml;
    }
    let bytes = xml.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut in_tag = false;
    let mut skip_special = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if !in_tag {
            if b == b'<' {
                in_tag = true;
                skip_special = bytes.get(i + 1) == Some(&b'!');
            }
            out.push(b);
            i += 1;
        } else if b == b'>' {
            in_tag = false;
            out.push(b);
            i += 1;
        } else if !skip_special && b == b'=' && bytes.get(i + 1) == Some(&b'\'') {
            let start = i + 2;
            match bytes[start..].iter().position(|&c| c == b'\'') {
                Some(off) => {
                    out.extend_from_slice(b"=\"");
                    for &vb in &bytes[start..start + off] {
                        if vb == b'"' {
                            out.extend_from_slice(b"&quot;");
                        } else {
                            out.push(vb);
                        }
                    }
                    out.push(b'"');
                    i = start + off + 1;
                }
                None => {
                    out.push(b);
                    i += 1;
                }
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or(xml)
}
