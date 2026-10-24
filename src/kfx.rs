//! Reading a DRM-free KFX book, as Send-to-Kindle stores it on the Kindle
//! (`documents/Downloads/Items01/<stem>_<content id>.kfx`).
//!
//! A KFX container (`CONT`, version 2) is a list of entities, each one or
//! more Ion values ([`crate::ion`]) named by a type symbol from Amazon's
//! shared `YJ_symbols` table. The ones read here:
//!
//! | Type | What |
//! |---|---|
//! | `$145` content | strings, each the text of one block |
//! | `$259` storyline | blocks in reading order: `eid` (`$155`), text (`$145`: content name + index), style runs (`$142`), nested blocks (`$146`) |
//! | `$260` section | one per spine item; its page template names the storyline |
//! | `$538` document data | the reading order of the sections |
//! | `$264` position map | the `eid`s in each section |
//! | `$265` position ID map | each section's first `pid` and length |
//! | `$609` section position IDs | each section's runs: `pid` delta and `eid` |
//! | `$490` book metadata | `title`, `author`, `content_id`, … |
//!
//! A position is a block `eid` and an offset in code points into the
//! block's text; its `pid` is the section's first `pid`, plus the start of
//! the block's run, plus the offset.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use crate::ion::{self, Value};

// Symbol IDs in the shared `YJ_symbols` table.
const NAME: u32 = 4;
const SECTION_TEMPLATES: u32 = 141;
const STYLE_RUNS: u32 = 142;
const RUN_OFFSET: u32 = 143;
const LENGTH: u32 = 144;
const CONTENT: u32 = 145;
const CONTENT_LIST: u32 = 146;
const ID: u32 = 155;
const READING_ORDERS: u32 = 169;
const SECTIONS: u32 = 170;
const SECTION_NAME: u32 = 174;
const STORY_NAME: u32 = 176;
const CONTAINS: u32 = 181;
const PID_START: u32 = 184;
const METADATA_LIST: u32 = 258;
const STORYLINE: u32 = 259;
const SECTION: u32 = 260;
const POSITION_MAP: u32 = 264;
const POSITION_ID_MAP: u32 = 265;
const KEY: u32 = 307;
const INDEX: u32 = 403;
const INDEX_OFFSET: u32 = 413;
const INDEX_LENGTH: u32 = 414;
const RAW_MEDIA: u32 = 417;
const BOOK_METADATA: u32 = 490;
const CATEGORISED_METADATA: u32 = 491;
const METADATA_KEY: u32 = 492;
const DOCUMENT_DATA: u32 = 538;
const SECTION_POSITION_IDS: u32 = 609;
// In `$490` entries, `$307` is the value and `$492` the key.
const METADATA_VALUE: u32 = KEY;

#[derive(Debug)]
pub enum Error {
  Io(std::io::Error),
  /// Encrypted (`\xeaDRMION\xee`); only DRM-free books can be read.
  Drm,
  Ion(ion::Error),
  Format(String),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Io(e) => write!(f, "{e}"),
      Error::Drm => write!(f, "KFX: the book has DRM"),
      Error::Ion(e) => write!(f, "KFX: {e}"),
      Error::Format(e) => write!(f, "KFX: {e}"),
    }
  }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
  fn from(e: std::io::Error) -> Self {
    Error::Io(e)
  }
}

impl From<ion::Error> for Error {
  fn from(e: ion::Error) -> Self {
    Error::Ion(e)
  }
}

fn broken(what: impl Into<String>) -> Error {
  Error::Format(what.into())
}

/// A block with text, as it is laid out on the Kindle.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
  pub eid: u32,
  /// The converted text: whitespace collapsed, comments dropped, inline
  /// elements flattened into it.
  pub text: String,
  /// Style runs (`$142`) as (offset, length) in code points: where inline
  /// elements such as `em` were.
  pub styled: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
  /// Symbol ID of the section name.
  pub name: u32,
  /// Text blocks in reading order, nested ones flattened.
  pub blocks: Vec<Block>,
  /// The `eid`s of the section (`$264`), page template and blocks.
  pub eids: Vec<u32>,
  /// First `pid` and number of positions (`$265`).
  pub pid_start: u32,
  pub pid_len: u32,
  /// Runs (`$609`) as (`pid` from the section start, `eid`); the last one,
  /// `eid` 0, marks the end.
  pub runs: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kfx {
  /// Sections in reading order: section *n* is spine item *n* of the EPUB.
  pub sections: Vec<Section>,
  /// String values of the book metadata (`$490`), by key.
  pub metadata: HashMap<String, String>,
}

impl Kfx {
  pub fn open(path: &Path) -> Result<Self, Error> {
    Kfx::parse(&std::fs::read(path)?)
  }

  pub fn parse(data: &[u8]) -> Result<Self, Error> {
    let entities = entities(data)?;
    let of_type = |t: u32| entities.iter().filter(move |e| e.kind == t);

    // Content: name -> strings.
    let mut content: HashMap<u32, Vec<String>> = HashMap::new();
    for e in of_type(CONTENT) {
      for v in &e.values {
        let name = v.get(NAME).and_then(Value::as_symbol).unwrap_or(e.id);
        let strings = v
          .get(CONTENT_LIST)
          .and_then(Value::as_list)
          .ok_or_else(|| broken(format!("content ${name} without strings")))?;
        content.insert(
          name,
          strings
            .iter()
            .map(|s| s.as_str().unwrap_or_default().to_string())
            .collect(),
        );
      }
    }

    let mut stories: HashMap<u32, Vec<Block>> = HashMap::new();
    for e in of_type(STORYLINE) {
      for v in &e.values {
        let name = v.get(STORY_NAME).and_then(Value::as_symbol).unwrap_or(e.id);
        let mut blocks = Vec::new();
        if let Some(items) = v.get(CONTENT_LIST).and_then(Value::as_list) {
          collect_blocks(items, &content, &mut blocks)?;
        }
        stories.insert(name, blocks);
      }
    }

    let mut section_stories: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in of_type(SECTION) {
      for v in &e.values {
        let name = v
          .get(SECTION_NAME)
          .and_then(Value::as_symbol)
          .unwrap_or(e.id);
        let mut names = Vec::new();
        for t in v
          .get(SECTION_TEMPLATES)
          .and_then(Value::as_list)
          .unwrap_or_default()
        {
          if let Some(story) = t.get(STORY_NAME).and_then(Value::as_symbol) {
            if !names.contains(&story) {
              names.push(story);
            }
          }
        }
        section_stories.insert(name, names);
      }
    }

    // Reading order: document data, else the reading orders in `$258`.
    let order = of_type(DOCUMENT_DATA)
      .chain(of_type(METADATA_LIST))
      .flat_map(|e| &e.values)
      .find_map(|v| {
        let first = v.get(READING_ORDERS)?.as_list()?.first()?;
        first
          .get(SECTIONS)?
          .as_list()?
          .iter()
          .map(Value::as_symbol)
          .collect::<Option<Vec<_>>>()
      })
      .ok_or_else(|| broken("no reading order"))?;

    let mut eids: HashMap<u32, Vec<u32>> = HashMap::new();
    for v in of_type(POSITION_MAP).flat_map(|e| &e.values) {
      for entry in v.as_list().unwrap_or_default() {
        let Some(name) = entry.get(SECTION_NAME).and_then(Value::as_symbol) else {
          continue;
        };
        let mut list = Vec::new();
        for item in entry
          .get(CONTAINS)
          .and_then(Value::as_list)
          .unwrap_or_default()
        {
          match item.as_list() {
            // [first eid, count]
            Some([first, count]) => {
              let (first, count) = (int(first)?, int(count)?);
              list.extend(first..first + count);
            }
            _ => list.push(int(item)?),
          }
        }
        eids.insert(name, list);
      }
    }

    let mut pid_ranges: HashMap<u32, (u32, u32)> = HashMap::new();
    for v in of_type(POSITION_ID_MAP).flat_map(|e| &e.values) {
      for entry in v.get(CONTAINS).and_then(Value::as_list).unwrap_or_default() {
        if let Some(name) = entry.get(SECTION_NAME).and_then(Value::as_symbol) {
          let start = int(
            entry
              .get(PID_START)
              .ok_or_else(|| broken("pid map without start"))?,
          )?;
          let len = int(
            entry
              .get(LENGTH)
              .ok_or_else(|| broken("pid map without length"))?,
          )?;
          pid_ranges.insert(name, (start, len));
        }
      }
    }

    let mut runs: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for v in of_type(SECTION_POSITION_IDS).flat_map(|e| &e.values) {
      let Some(name) = v.get(SECTION_NAME).and_then(Value::as_symbol) else {
        continue;
      };
      // [pid delta, eid], or a bare pid delta for the next eid.
      let (mut pid, mut eid) = (0u32, 0u32);
      let mut list = Vec::new();
      for item in v.get(CONTAINS).and_then(Value::as_list).unwrap_or_default() {
        let delta = match item.as_list() {
          Some([delta, id]) => {
            eid = int(id)?;
            int(delta)?
          }
          _ => {
            eid += 1;
            int(item)?
          }
        };
        pid = pid
          .checked_add(delta)
          .ok_or_else(|| broken("pid overflow"))?;
        list.push((pid, eid));
      }
      runs.insert(name, list);
    }

    let mut sections = Vec::new();
    for name in order {
      let mut blocks = Vec::new();
      for story in section_stories.get(&name).into_iter().flatten() {
        blocks.extend(stories.get(story).cloned().unwrap_or_default());
      }
      let (pid_start, pid_len) = pid_ranges.get(&name).copied().unwrap_or_default();
      sections.push(Section {
        name,
        blocks,
        eids: eids.remove(&name).unwrap_or_default(),
        pid_start,
        pid_len,
        runs: runs.remove(&name).unwrap_or_default(),
      });
    }

    let mut metadata = HashMap::new();
    for v in of_type(BOOK_METADATA).flat_map(|e| &e.values) {
      for category in v
        .get(CATEGORISED_METADATA)
        .and_then(Value::as_list)
        .unwrap_or_default()
      {
        for kv in category
          .get(METADATA_LIST)
          .and_then(Value::as_list)
          .unwrap_or_default()
        {
          if let (Some(k), Some(value)) = (
            kv.get(METADATA_KEY).and_then(Value::as_str),
            kv.get(METADATA_VALUE).and_then(Value::as_str),
          ) {
            metadata.insert(k.to_string(), value.to_string());
          }
        }
      }
    }

    Ok(Kfx { sections, metadata })
  }

  /// A metadata value, such as `title`, `author` or `content_id`.
  pub fn meta(&self, key: &str) -> Option<&str> {
    self.metadata.get(key).map(String::as_str)
  }

  /// The section index and the block with this `eid`.
  pub fn block(&self, eid: u32) -> Option<(usize, usize, &Block)> {
    self.sections.iter().enumerate().find_map(|(s, section)| {
      let (b, block) = section
        .blocks
        .iter()
        .enumerate()
        .find(|(_, b)| b.eid == eid)?;
      Some((s, b, block))
    })
  }

  /// The `pid` of a character, from the position maps; `None` if the
  /// `eid` has no run.
  pub fn pid(&self, eid: u32, offset: u32) -> Option<u32> {
    let section = self
      .sections
      .iter()
      .find(|s| s.eids.contains(&eid) || s.runs.iter().any(|r| r.1 == eid))?;
    // A block may be split into several runs (around nested blocks); an
    // offset past its last run counts on from that run's end.
    let mut left = offset;
    let mut past_end = None;
    for (i, &(start, id)) in section.runs.iter().enumerate() {
      if id != eid {
        continue;
      }
      let end = section.runs.get(i + 1).map_or(section.pid_len, |r| r.0);
      if left < end - start {
        return Some(section.pid_start + start + left);
      }
      left -= end - start;
      past_end = Some(end + left);
    }
    past_end.map(|pid| section.pid_start + pid)
  }
}

fn int(v: &Value) -> Result<u32, Error> {
  v.as_int()
    .and_then(|n| u32::try_from(n).ok())
    .ok_or_else(|| broken(format!("expected a number, found {v:?}")))
}

/// Text blocks of a storyline content list, depth first.
fn collect_blocks(
  items: &[Value],
  content: &HashMap<u32, Vec<String>>,
  out: &mut Vec<Block>,
) -> Result<(), Error> {
  for item in items {
    let text = match item.get(CONTENT) {
      Some(Value::String(s)) => Some(s.clone()),
      Some(r) => {
        let name = r.get(NAME).and_then(Value::as_symbol);
        let index = r.get(INDEX).and_then(Value::as_int);
        match (name, index) {
          (Some(name), Some(index)) => Some(
            content
              .get(&name)
              .and_then(|c| c.get(usize::try_from(index).ok()?))
              .ok_or_else(|| broken(format!("no text {index} in content ${name}")))?
              .clone(),
          ),
          _ => None,
        }
      }
      None => None,
    };
    if let (Some(text), Some(eid)) = (text, item.get(ID)) {
      let styled = item
        .get(STYLE_RUNS)
        .and_then(Value::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(|r| Some((int(r.get(RUN_OFFSET)?).ok()?, int(r.get(LENGTH)?).ok()?)))
        .collect();
      out.push(Block {
        eid: int(eid)?,
        text,
        styled,
      });
    }
    if let Some(children) = item.get(CONTENT_LIST).and_then(Value::as_list) {
      collect_blocks(children, content, out)?;
    }
  }
  Ok(())
}

struct Entity {
  id: u32,
  kind: u32,
  values: Vec<Value>,
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
  Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<usize> {
  usize::try_from(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?)).ok()
}

/// The entities of a container, raw media (`$417`) left out.
fn entities(data: &[u8]) -> Result<Vec<Entity>, Error> {
  if data.starts_with(b"\xeaDRMION\xee") {
    return Err(Error::Drm);
  }
  if !data.starts_with(b"CONT") {
    return Err(broken("not a KFX container"));
  }
  let short = || broken("container header cut off");
  let header_len = u32_at(data, 6).ok_or_else(short)? as usize;
  let info_offset = u32_at(data, 10).ok_or_else(short)? as usize;
  let info_len = u32_at(data, 14).ok_or_else(short)? as usize;
  let info = data
    .get(info_offset..info_offset + info_len)
    .ok_or_else(short)?;
  let info = ion::read_stream(info)?
    .into_iter()
    .next()
    .ok_or_else(|| broken("no container info"))?;
  let field = |k| {
    info
      .get(k)
      .and_then(Value::as_int)
      .and_then(|n| usize::try_from(n).ok())
  };
  let index_offset = field(INDEX_OFFSET).ok_or_else(|| broken("no entity index"))?;
  let index_len = field(INDEX_LENGTH).ok_or_else(|| broken("no entity index"))?;
  let index = data
    .get(index_offset..index_offset + index_len)
    .ok_or_else(|| broken("entity index cut off"))?;

  let mut out = Vec::new();
  for entry in index.as_chunks::<24>().0 {
    let bad = || broken("bad entity index entry");
    let id = u32_at(entry, 0).ok_or_else(bad)?;
    let kind = u32_at(entry, 4).ok_or_else(bad)?;
    if kind == RAW_MEDIA {
      continue;
    }
    let offset = header_len + u64_at(entry, 8).ok_or_else(bad)?;
    let len = u64_at(entry, 16).ok_or_else(bad)?;
    let raw = data
      .get(offset..offset + len)
      .filter(|r| r.starts_with(b"ENTY"))
      .ok_or_else(|| broken(format!("entity ${id} missing or broken")))?;
    let entity_header = u32_at(raw, 6).ok_or_else(bad)? as usize;
    let payload = raw
      .get(entity_header..)
      .ok_or_else(|| broken(format!("entity ${id} cut off")))?;
    out.push(Entity {
      id,
      kind,
      values: ion::read_stream(payload)?,
    });
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rejects_drm_and_other_files() {
    assert!(matches!(Kfx::parse(b"\xeaDRMION\xee..."), Err(Error::Drm)));
    assert!(matches!(Kfx::parse(b"PK\x03\x04"), Err(Error::Format(_))));
    assert!(matches!(Kfx::parse(b"CONT\x02\x00"), Err(Error::Format(_))));
  }
}
