//! A small XML tree for EPUB chapters that keeps every node the position
//! formats count: whitespace-only text, comments and CDATA sections stay
//! separate nodes, and text split by entity references is one text node.

use std::fmt;

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
  Element(Element),
  Text(String),
  CData(String),
  Comment(String),
  /// Processing instructions; counted as nodes, never hold a position.
  Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
  /// Local name, without a namespace prefix.
  pub name: String,
  pub id: Option<String>,
  pub children: Vec<Node>,
}

impl Element {
  /// The first descendant (or self) with this `id`, and the child indices
  /// that lead to it.
  pub fn find_id(&self, id: &str) -> Option<(Vec<usize>, &Element)> {
    if self.id.as_deref() == Some(id) {
      return Some((Vec::new(), self));
    }
    for (i, child) in self.children.iter().enumerate() {
      if let Node::Element(e) = child {
        if let Some((mut path, found)) = e.find_id(id) {
          path.insert(0, i);
          return Some((path, found));
        }
      }
    }
    None
  }

  /// The node at a path of child indices.
  pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
    let (&last, parents) = path.split_last()?;
    let mut element = self;
    for &i in parents {
      match element.children.get(i)? {
        Node::Element(e) => element = e,
        _ => return None,
      }
    }
    element.children.get(last)
  }

  /// The element at a path of child indices (`[]` is `self`).
  pub fn element_at(&self, path: &[usize]) -> Option<&Element> {
    if path.is_empty() {
      return Some(self);
    }
    match self.node_at(path)? {
      Node::Element(e) => Some(e),
      _ => None,
    }
  }
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "XML: {}", self.0)
  }
}

impl std::error::Error for Error {}

/// Parses a document and returns its root element.
pub fn parse(xml: &str) -> Result<Element, Error> {
  let mut reader = Reader::from_str(xml);
  // Whitespace is significant; empty elements become start + end.
  reader.config_mut().expand_empty_elements = true;
  let mut stack: Vec<Element> = Vec::new();
  let mut root = None;
  loop {
    let event = reader
      .read_event()
      .map_err(|e| Error(format!("at byte {}: {e}", reader.buffer_position())))?;
    match event {
      Event::Start(start) => stack.push(element(&start)?),
      Event::End(_) => {
        let done = stack
          .pop()
          .ok_or_else(|| Error("unmatched end tag".into()))?;
        match stack.last_mut() {
          Some(parent) => parent.children.push(Node::Element(done)),
          None => root = Some(done),
        }
      }
      Event::Empty(_) => unreachable!("expand_empty_elements is set"),
      Event::Text(text) => {
        if let Some(parent) = stack.last_mut() {
          push_text(parent, &text.xml10_content());
        }
      }
      Event::GeneralRef(r) => {
        if let Some(parent) = stack.last_mut() {
          push_text(parent, &resolve(&r)?);
        }
      }
      Event::CData(c) => {
        if let Some(parent) = stack.last_mut() {
          parent
            .children
            .push(Node::CData(c.xml10_content().into_owned()));
        }
      }
      Event::Comment(c) => {
        if let Some(parent) = stack.last_mut() {
          parent
            .children
            .push(Node::Comment(c.into_inner().into_owned()));
        }
      }
      Event::PI(_) => {
        if let Some(parent) = stack.last_mut() {
          parent.children.push(Node::Other);
        }
      }
      Event::Decl(_) | Event::DocType(_) => {}
      Event::Eof => break,
    }
  }
  root.ok_or_else(|| Error("no root element".into()))
}

fn element(start: &BytesStart) -> Result<Element, Error> {
  let name = start.local_name().as_ref().to_string();
  let mut id = None;
  for attr in start.attributes().with_checks(false) {
    let attr = attr.map_err(|e| Error(e.to_string()))?;
    if attr.key.local_name().as_ref() == "id" {
      let value = attr
        .normalized_value_with(XmlVersion::Implicit1_0, 8, entity)
        .map_err(|e| Error(e.to_string()))?;
      id = Some(value.into_owned());
    }
  }
  Ok(Element {
    name,
    id,
    children: Vec::new(),
  })
}

/// Appends text, joining it with a directly preceding text node: an entity
/// reference does not split a text node.
fn push_text(parent: &mut Element, text: &str) {
  if let Some(Node::Text(last)) = parent.children.last_mut() {
    last.push_str(text);
  } else {
    parent.children.push(Node::Text(text.to_string()));
  }
}

fn resolve(r: &BytesRef) -> Result<String, Error> {
  if r.is_char_ref() {
    let c = r
      .resolve_char_ref()
      .map_err(|e| Error(e.to_string()))?
      .ok_or_else(|| Error("bad character reference".into()))?;
    return Ok(c.to_string());
  }
  let name = r.clone().into_inner();
  Ok(match entity(&name) {
    Some(s) => s.to_string(),
    // Unknown named entity (needs a DTD we do not load): keep it as written.
    None => format!("&{name};"),
  })
}

/// The XML entities and the HTML ones that show up in EPUBs.
fn entity(name: &str) -> Option<&'static str> {
  Some(match name {
    "amp" => "&",
    "lt" => "<",
    "gt" => ">",
    "quot" => "\"",
    "apos" => "'",
    "nbsp" => "\u{a0}",
    "shy" => "\u{ad}",
    "ndash" => "\u{2013}",
    "mdash" => "\u{2014}",
    "hellip" => "\u{2026}",
    "lsquo" => "\u{2018}",
    "rsquo" => "\u{2019}",
    "sbquo" => "\u{201a}",
    "ldquo" => "\u{201c}",
    "rdquo" => "\u{201d}",
    "bdquo" => "\u{201e}",
    "laquo" => "\u{ab}",
    "raquo" => "\u{bb}",
    "thinsp" => "\u{2009}",
    "ensp" => "\u{2002}",
    "emsp" => "\u{2003}",
    "zwnj" => "\u{200c}",
    "zwj" => "\u{200d}",
    "copy" => "\u{a9}",
    _ => return None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keeps_whitespace_comments_and_cdata_apart() {
    let root =
      parse("<?xml version='1.0'?>\n<html><body>\n<p>a &amp; b&#160;c<!--x--><![CDATA[<d>]]>e</p>\n</body></html>").unwrap();
    let body = root.element_at(&[0]).unwrap();
    assert_eq!(body.name, "body");
    assert_eq!(body.children[0], Node::Text("\n".into()));
    let Node::Element(p) = &body.children[1] else {
      panic!()
    };
    assert_eq!(
      p.children,
      [
        Node::Text("a & b\u{a0}c".into()),
        Node::Comment("x".into()),
        Node::CData("<d>".into()),
        Node::Text("e".into()),
      ]
    );
  }

  #[test]
  fn finds_ids() {
    let root =
      parse(r#"<html><body><div id="a"><span id="kobo.1.1">x</span></div></body></html>"#).unwrap();
    let (path, span) = root.find_id("kobo.1.1").unwrap();
    assert_eq!(path, [0, 0, 0]);
    assert_eq!(span.name, "span");
  }
}
