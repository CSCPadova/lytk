//! MXL (compressed MusicXML) ZIP handling.
//!
//! `.mxl` files are ZIP archives. The rootfile path is declared inside
//! `META-INF/container.xml`. This module extracts the XML content string.

use std::io::Read;
use std::path::Path;

use super::{AdapterError, Result};

/// Read a MusicXML file, handling both `.xml` and `.mxl` (ZIP) inputs.
pub fn read_musicxml(path: &Path) -> Result<String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mxl") => extract_mxl(path),
        _ => Ok(std::fs::read_to_string(path)?),
    }
}

/// Extract the root XML document from a `.mxl` ZIP archive.
fn extract_mxl(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let rootfile = find_rootfile(&mut archive)?;
    let mut entry = archive.by_name(&rootfile)?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    Ok(buf)
}

/// Parse `META-INF/container.xml` to find the root-file path.
fn find_rootfile(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<String> {
    let mut container = archive
        .by_name("META-INF/container.xml")
        .map_err(|_| AdapterError::MissingElement("META-INF/container.xml".into()))?;

    let mut xml = String::new();
    container.read_to_string(&mut xml)?;

    // Parse the container XML to find <rootfile full-path="..."/>
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Empty(ref e) | quick_xml::events::Event::Start(ref e))
                if e.name().as_ref() == b"rootfile" =>
            {
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == b"full-path" {
                        return Ok(String::from_utf8_lossy(&attr.value).into_owned());
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(AdapterError::Xml(e)),
            _ => {}
        }
        buf.clear();
    }

    Err(AdapterError::MissingElement(
        "rootfile full-path in META-INF/container.xml".into(),
    ))
}
