//! Reading an EPUB: the OPF spine and the chapter documents.
//!
//! Positions on the device name the chapter by its path from the EPUB root
//! (`OEBPS/xhtml/xhtml-0-81.xhtml`); KOReader names it by its spine index
//! (`DocFragment[82]`, 1-based). The spine maps one to the other.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use crate::dom;

#[derive(Debug)]
pub enum Error {
  Io(io::Error),
  Zip(zip::result::ZipError),
  Xml(String),
  /// A file the EPUB needs is missing or the OPF has no spine.
  Broken(String),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Io(e) => write!(f, "{e}"),
      Error::Zip(e) => write!(f, "ZIP: {e}"),
      Error::Xml(e) => write!(f, "{e}"),
      Error::Broken(e) => write!(f, "EPUB: {e}"),
    }
  }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
  fn from(e: io::Error) -> Self {
    Error::Io(e)
  }
}

impl From<zip::result::ZipError> for Error {
  fn from(e: zip::result::ZipError) -> Self {
    Error::Zip(e)
  }
}

impl From<dom::Error> for Error {
  fn from(e: dom::Error) -> Self {
    Error::Xml(e.to_string())
  }
}

pub struct Epub {
  archive: ZipArchive<File>,
  /// Spine items as paths from the EPUB root, in reading order.
  pub spine: Vec<String>,
}

impl Epub {
  pub fn open(path: &Path) -> Result<Self, Error> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    let container = read(&mut archive, "META-INF/container.xml")?;
    let opf_path = start_tags(&container)?
      .into_iter()
      .find(|(name, _)| name == "rootfile")
      .and_then(|(_, attrs)| attrs.get("full-path").cloned())
      .ok_or_else(|| Error::Broken("no rootfile in META-INF/container.xml".into()))?;
    let opf_dir = opf_path
      .rsplit_once('/')
      .map_or("", |(dir, _)| dir)
      .to_string();
    let opf = read(&mut archive, &opf_path)?;
    let tags = start_tags(&opf)?;
    let manifest: HashMap<_, _> = tags
      .iter()
      .filter(|(name, _)| name == "item")
      .filter_map(|(_, a)| Some((a.get("id")?.clone(), a.get("href")?.clone())))
      .collect();
    let spine: Vec<_> = tags
      .iter()
      .filter(|(name, _)| name == "itemref")
      .filter_map(|(_, a)| manifest.get(a.get("idref")?))
      .map(|href| join(&opf_dir, href))
      .collect();
    if spine.is_empty() {
      return Err(Error::Broken(format!("{opf_path} has no spine")));
    }
    Ok(Epub { archive, spine })
  }

  /// Spine index of a chapter given by its path from the EPUB root.
  pub fn spine_index(&self, chapter: &str) -> Option<usize> {
    let chapter = percent_decode(chapter);
    self.spine.iter().position(|item| *item == chapter)
  }

  /// The parsed chapter document at a spine index.
  pub fn chapter(&mut self, index: usize) -> Result<dom::Element, Error> {
    let path = self
      .spine
      .get(index)
      .ok_or_else(|| Error::Broken(format!("no spine item {index}")))?
      .clone();
    Ok(dom::parse(&read(&mut self.archive, &path)?)?)
  }
}

fn read(archive: &mut ZipArchive<File>, name: &str) -> Result<String, Error> {
  let mut file = archive
    .by_name(name)
    .map_err(|_| Error::Broken(format!("{name} is missing")))?;
  let mut text = String::new();
  file.read_to_string(&mut text)?;
  Ok(text)
}

type Attrs = HashMap<String, String>;

/// Local name and attributes of every start or empty tag.
fn start_tags(xml: &str) -> Result<Vec<(String, Attrs)>, Error> {
  let mut reader = Reader::from_str(xml);
  let mut tags = Vec::new();
  loop {
    match reader.read_event().map_err(|e| Error::Xml(e.to_string()))? {
      Event::Start(t) | Event::Empty(t) => tags.push(tag(&t)),
      Event::Eof => return Ok(tags),
      _ => {}
    }
  }
}

fn tag(t: &BytesStart) -> (String, Attrs) {
  let attrs = t
    .attributes()
    .with_checks(false)
    .flatten()
    .map(|a| {
      (
        a.key.local_name().as_ref().to_string(),
        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
          .map(|v| v.into_owned())
          .unwrap_or_default(),
      )
    })
    .collect();
  (t.local_name().as_ref().to_string(), attrs)
}

/// Resolves an href from the OPF against the OPF directory: drops any
/// fragment, decodes `%XX`, and folds `.` and `..` segments.
fn join(base: &str, href: &str) -> String {
  let href = percent_decode(href.split('#').next().unwrap_or(href));
  let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
  for segment in href.split('/') {
    match segment {
      "" | "." => {}
      ".." => {
        parts.pop();
      }
      s => parts.push(s),
    }
  }
  parts.join("/")
}

fn percent_decode(s: &str) -> String {
  let bytes = s.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    let hex = bytes
      .get(i + 1..i + 3)
      .and_then(|h| std::str::from_utf8(h).ok());
    match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
      (b'%', Some(b)) => {
        out.push(b);
        i += 3;
      }
      (b, _) => {
        out.push(b);
        i += 1;
      }
    }
  }
  String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn joins_hrefs() {
    assert_eq!(join("OEBPS", "xhtml/a.xhtml"), "OEBPS/xhtml/a.xhtml");
    assert_eq!(join("", "text/part0004.html"), "text/part0004.html");
    assert_eq!(join("OPS/x", "../a%20b.html#top"), "OPS/a b.html");
  }
}
