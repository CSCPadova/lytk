//! XML helper: a minimal DOM-like tree built from quick-xml events.

use quick_xml::events::Event;
use quick_xml::Reader;

use super::super::{AdapterError, Result};

/// A simple in-memory XML element for easy traversal.
/// We build this from quick-xml events to avoid repeated streaming.
#[derive(Debug, Clone)]
pub(super) struct XmlNode {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XmlNode>,
    pub text: String,
}

impl XmlNode {
    pub fn new(tag: String) -> Self {
        Self {
            tag,
            attrs: Vec::new(),
            children: Vec::new(),
            text: String::new(),
        }
    }

    /// Get attribute value by name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Find first child element by tag name.
    pub fn find(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// Find all child elements by tag name.
    pub fn find_all(&self, tag: &str) -> Vec<&XmlNode> {
        self.children.iter().filter(|c| c.tag == tag).collect()
    }

    /// Get text content, trimmed.
    pub fn text_content(&self) -> &str {
        self.text.trim()
    }

    /// Parse text content as i64, with a default.
    pub fn text_i64(&self, default: i64) -> i64 {
        self.text_content().parse().unwrap_or(default)
    }

    /// Find a child and get its text as i64.
    pub fn child_i64(&self, tag: &str, default: i64) -> i64 {
        self.find(tag)
            .map(|n| n.text_i64(default))
            .unwrap_or(default)
    }

    /// Find a child and get its text content.
    pub fn child_text(&self, tag: &str) -> Option<&str> {
        self.find(tag).map(|n| {
            let t = n.text_content();
            if t.is_empty() {
                return "";
            }
            t
        })
    }
}

/// Parse XML string into a tree of XmlNodes.
pub(super) fn parse_xml(xml: &str) -> Result<XmlNode> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<XmlNode> = vec![XmlNode::new("__root__".into())];
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut node = XmlNode::new(tag);
                for attr in e.attributes() {
                    let attr = attr?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let val = String::from_utf8_lossy(&attr.value).into_owned();
                    node.attrs.push((key, val));
                }
                stack.push(node);
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().unwrap();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    return Ok(node);
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut node = XmlNode::new(tag);
                for attr in e.attributes() {
                    let attr = attr?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let val = String::from_utf8_lossy(&attr.value).into_owned();
                    node.attrs.push((key, val));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e
                    .unescape()
                    .map_err(|err| AdapterError::Parse(err.to_string()))?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {} // comments, PI, CDATA, etc.
            Err(e) => return Err(AdapterError::Xml(e)),
        }
        buf.clear();
    }

    // Return the root's first real child (skip __root__ wrapper).
    let root = stack.pop().unwrap();
    root.children
        .into_iter()
        .next()
        .ok_or_else(|| AdapterError::MissingElement("root element".into()))
}
