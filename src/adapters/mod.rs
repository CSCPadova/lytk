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

/// Longest music the MusicXML, ABC and Humdrum readers accept, in whole notes
/// (the LilyPond and MIDI readers have the same bound): past it the bars cut
/// downstream would be millions.
pub(crate) const MAX_WHOLE_NOTES: u32 = 100_000;

fn too_long() -> AdapterError {
    AdapterError::Unsupported(format!(
        "the music is longer than {MAX_WHOLE_NOTES} whole notes; lytk refuses input this large"
    ))
}

/// Refuse music longer than [`MAX_WHOLE_NOTES`].
pub(crate) fn check_music_length(music: &crate::ir::music::Music) -> Result<()> {
    if music.approx_length() > f64::from(MAX_WHOLE_NOTES) {
        return Err(too_long());
    }
    Ok(())
}

/// Refuse a score longer than [`MAX_WHOLE_NOTES`]: its longest part, each
/// measure as long as its longest voice.
pub(crate) fn check_score_length(score: &Score) -> Result<()> {
    let seconds = |e: &crate::ir::note::VoiceElement| {
        let d = e.metric_duration();
        *d.numer() as f64 / *d.denom() as f64
    };
    let longest = score
        .parts()
        .iter()
        .map(|p| {
            p.measures
                .iter()
                .map(|m| {
                    m.voices
                        .iter()
                        .map(|v| v.elements.iter().map(seconds).sum::<f64>())
                        .fold(0.0, f64::max)
                })
                .sum::<f64>()
        })
        .fold(0.0, f64::max);
    if longest > f64::from(MAX_WHOLE_NOTES) {
        return Err(too_long());
    }
    Ok(())
}

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

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// A text file's bytes as UTF-8: as they are when they are UTF-8 (a leading
/// byte-order mark dropped), decoded by their byte-order mark when UTF-16,
/// else Latin-1 (each byte its own character), the usual encoding of older
/// ABC, Humdrum and MIDI files.
pub(crate) fn decode_text_bytes(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(s) => match s.strip_prefix('\u{feff}') {
            Some(rest) => rest.to_string(),
            None => s,
        },
        Err(e) => {
            let b = e.into_bytes();
            let utf16 = |rest: &[u8], big_endian: bool| -> String {
                let units = rest.chunks_exact(2).map(|p| {
                    if big_endian {
                        u16::from_be_bytes([p[0], p[1]])
                    } else {
                        u16::from_le_bytes([p[0], p[1]])
                    }
                });
                char::decode_utf16(units)
                    .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect()
            };
            match b.as_slice() {
                [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
                [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
                _ => b.iter().map(|&c| char::from(c)).collect(),
            }
        }
    }
}

/// Unicode noncharacters standing in, inside MusicXML text, for what the
/// `musicxml` crate's parser cannot keep: `<` (it would start a tag), `"`
/// (it would end an attribute value) and the line breaks and tabs it deletes
/// from text. Noncharacters never occur in interchanged text, unlike the
/// private-use area that SMuFL's music glyphs live in.
pub(crate) const XML_STAND_INS: [(char, char); 4] = [
    ('<', '\u{FDD0}'),
    ('"', '\u{FDD1}'),
    ('\n', '\u{FDD2}'),
    ('\t', '\u{FDD3}'),
];

fn stand_in(c: char) -> char {
    XML_STAND_INS
        .iter()
        .find(|(raw, _)| *raw == c)
        .map_or(c, |(_, s)| *s)
}

/// One character reference or predefined entity at the start of `s` (just
/// past its `&`): the character and the bytes it took, `;` included.
fn xml_entity(s: &str) -> Option<(char, usize)> {
    let end = s.get(..12).unwrap_or(s).find(';')?;
    let name = &s[..end];
    let c = match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let code = match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => name.strip_prefix('#')?.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((c, end + 1))
}

/// `s` with its entities decoded, characters that can't stay as themselves
/// as [`XML_STAND_INS`]; `keep` lists the raw characters left as they are.
fn decode_xml_chars(s: &str, keep: &[char], out: &mut String) {
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '&' {
            if let Some((d, len)) = xml_entity(&rest[1..]) {
                out.push(if keep.contains(&d) { d } else { stand_in(d) });
                rest = &rest[1 + len..];
                continue;
            }
        }
        out.push(if keep.contains(&c) { c } else { stand_in(c) });
        rest = &rest[c.len_utf8()..];
    }
}

/// MusicXML ready for the `musicxml` crate, which reads text and attribute
/// values raw: their entities decoded (`&amp;` was kept as `&amp;` in a
/// title, `&#233;` as `&#233;`), CDATA sections made text, and line breaks
/// inside text kept, all with [`XML_STAND_INS`] where needed.
/// [`restore_xml_text`] turns the stand-ins back. `None` when nothing
/// changes.
pub(crate) fn decode_xml_text(xml: &str) -> Option<String> {
    if !xml.contains(['&', '\n', '\t']) && !xml.contains("<![CDATA[") {
        return None;
    }
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while !rest.is_empty() {
        // Text up to the next markup: inner whitespace is content, the
        // whitespace around it is layout the parser trims.
        let text_end = rest.find('<').unwrap_or(rest.len());
        let text = &rest[..text_end];
        let core = text.trim();
        if core.is_empty() {
            out.push_str(text);
        } else {
            let lead = &text[..text.len() - text.trim_start().len()];
            let trail = &text[text.trim_end().len()..];
            out.push_str(lead);
            decode_xml_chars(&core.replace('\r', ""), &['>', '"', '&', '\''], &mut out);
            out.push_str(trail);
        }
        rest = &rest[text_end..];
        if rest.is_empty() {
            break;
        }
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").unwrap_or(body.len());
            for c in body[..end].chars().filter(|&c| c != '\r') {
                out.push(if c == '"' { c } else { stand_in(c) });
            }
            rest = body.get(end + 3..).unwrap_or("");
        } else if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(rest.len(), |e| e + 3);
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        } else {
            // A tag, its quoted attribute values decoded (`<` and `"` as
            // stand-ins: either would break the tag).
            let keep = ['>', '&', '\'', '\n', '\t'];
            let mut quote: Option<char> = None;
            let mut value = String::new();
            let mut end = rest.len();
            for (i, c) in rest.char_indices() {
                match quote {
                    None => {
                        out.push(c);
                        if c == '>' {
                            end = i + 1;
                            break;
                        }
                        if c == '"' || c == '\'' {
                            quote = Some(c);
                            value.clear();
                        }
                    }
                    Some(q) if c == q => {
                        decode_xml_chars(&value, &keep, &mut out);
                        out.push(c);
                        quote = None;
                    }
                    Some(_) => value.push(c),
                }
            }
            if quote.is_some() {
                out.push_str(&value); // an unterminated value, as it was
            }
            rest = &rest[end..];
        }
    }
    Some(out)
}

/// A string of a score read through [`decode_xml_text`], its stand-ins
/// turned back into the characters they stand for.
pub(crate) fn restore_xml_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            XML_STAND_INS
                .iter()
                .find(|(_, stand)| *stand == c)
                .map_or(c, |(raw, _)| *raw)
        })
        .collect()
}

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

#[cfg(test)]
mod text_tests {
    use super::*;
    use crate::ir::articulation::{LyricSyllable, SyllabicType};

    #[test]
    fn text_bytes_in_any_encoding_become_utf8() {
        assert_eq!(decode_text_bytes("Grüß".as_bytes().to_vec()), "Grüß");
        assert_eq!(decode_text_bytes(b"\xEF\xBB\xBFT:Tune".to_vec()), "T:Tune");
        // Latin-1 (`ü` = 0xFC, `ß` = 0xDF) isn't UTF-8.
        assert_eq!(decode_text_bytes(b"Gr\xFC\xDF".to_vec()), "Grüß");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Ré".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text_bytes(utf16), "Ré");
    }

    #[test]
    fn xml_text_is_decoded_for_the_crate() {
        let xml =
            "<a t=\"x &quot;y&quot;\">\n  <b>Tom &amp; Jerry &lt;live&gt; &#233;&#x41;</b>\n  \
                   <c>one\ntwo</c><!-- a &amp; b --><d><![CDATA[x < y]]></d>\n</a>";
        let out = decode_xml_text(xml).expect("changed");
        assert_eq!(
            out,
            "<a t=\"x \u{FDD1}y\u{FDD1}\">\n  <b>Tom & Jerry \u{FDD0}live> éA</b>\n  \
             <c>one\u{FDD2}two</c><!-- a &amp; b --><d>x \u{FDD0} y</d>\n</a>"
        );
        assert_eq!(
            restore_xml_text("x \u{FDD0} y\u{FDD2}\u{FDD1}"),
            "x < y\n\""
        );
        assert_eq!(decode_xml_text("<a>plain</a>"), None);
    }

    #[test]
    fn musicxml_text_round_trips_whatever_its_characters() {
        use crate::ir::duration::Duration;
        use crate::ir::note::{Note, VoiceElement};
        use crate::ir::pitch::{Pitch, PitchStep};
        let mut score = crate::adapters::ly_to_ir::LyToIrAdapter::new()
            .convert_str("{ c'4 }")
            .expect("reads");
        let title = "Tom & Jerry <live> \"quoted\" 'apos'";
        score.metadata.title = Some(title.to_string());
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.lyrics.push(LyricSyllable {
            text: "<a> & b".to_string(),
            syllabic: SyllabicType::Single,
            number: 1,
            extend: false,
            elision: false,
            name: None,
        });
        score.parts_mut()[0].measures[0].voices[0].elements[0] = VoiceElement::Note(Box::new(note));
        let xml = crate::adapters::ir_to_mxml::IrToMxmlAdapter::new()
            .convert(&score)
            .expect("writes");
        assert!(xml.contains("Tom &amp; Jerry &lt;live&gt;"), "{xml}");
        let back = crate::adapters::mxml_to_ir::MxmlToIrAdapter::new()
            .convert_str(&xml)
            .expect("reads back");
        assert_eq!(back.metadata.title.as_deref(), Some(title));
        let VoiceElement::Note(n) = &back.parts()[0].measures[0].voices[0].elements[0] else {
            panic!("a note");
        };
        assert_eq!(n.lyrics[0].text, "<a> & b");
    }
}
