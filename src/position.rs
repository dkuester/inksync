//! Position translation between Nickel and KOReader.
//!
//! Converts a Nickel position (kepub span or Adobe point inside a chapter) to
//! a KOReader XPointer and back, using the EPUB itself. Back is done for the
//! Tolino's Adobe point so far ([`from_xpointer`]); the Kobo's span and its
//! 4.x point follow with the Kobo.
//!
//! The rules come from highlights of the same words on a Kobo 4.46, a Tolino
//! 5.18 and in KOReader (probe books Book P and Book Q, see SCOPE.md):
//!
//! | | Offset unit | Whitespace | Comments, CDATA |
//! |---|---|---|---|
//! | Kobo span | UTF-16 units | as is | (spans end there) |
//! | Adobe point, Kobo 4.x | UTF-8 bytes | as is | skipped, text joined |
//! | Adobe point, Tolino 5.x | UTF-8 bytes | as is | nodes of their own |
//! | KOReader XPointer | code points | runs collapsed to one space | comment: skipped, CDATA: a `text()` of its own |
//!
//! Node paths agree everywhere: Adobe points count all child nodes from `/1`
//! = `html`, KOReader counts elements by tag name and text nodes by
//! position, below `/body[1]/DocFragment[spine index + 1]`.

use std::fmt;

use crate::dom::{Element, Node};
use crate::epub::{self, Epub};

/// A position as Nickel stores it in `ChapterIDBookmarked` (or a highlight).
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
  /// Chapter path from the EPUB root.
  pub chapter: String,
  pub anchor: Anchor,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
  /// Chapter only (Nickel stores no fragment at a chapter start).
  None,
  /// `#kobo.N.M` in a kepub; `offset` in UTF-16 units into the span's text
  /// (0 for a reading position, set for highlights).
  KoboSpan { id: String, offset: usize },
  /// `#point(/1/a/b…:offset)`; `steps` are 1-based, the first one is `html`;
  /// `offset` in UTF-8 bytes.
  Point { steps: Vec<usize>, offset: usize },
}

impl Position {
  /// Parses `ChapterIDBookmarked`. An unknown fragment is ignored (chapter
  /// only).
  pub fn parse(stored: &str) -> Self {
    let (chapter, fragment) = stored.split_once('#').unwrap_or((stored, ""));
    let anchor = if let Some(id) = fragment.strip_prefix("kobo.") {
      Anchor::KoboSpan {
        id: format!("kobo.{id}"),
        offset: 0,
      }
    } else if let Some((steps, offset)) = parse_point(fragment) {
      Anchor::Point { steps, offset }
    } else {
      Anchor::None
    };
    Position {
      chapter: chapter.to_string(),
      anchor,
    }
  }
}

impl fmt::Display for Position {
  /// As Nickel stores it in `ChapterIDBookmarked`.
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.chapter)?;
    match &self.anchor {
      Anchor::None => Ok(()),
      Anchor::KoboSpan { id, .. } => write!(f, "#{id}"),
      Anchor::Point { steps, offset } => {
        write!(f, "#point(")?;
        for step in steps {
          write!(f, "/{step}")?;
        }
        write!(f, ":{offset})")
      }
    }
  }
}

fn parse_point(fragment: &str) -> Option<(Vec<usize>, usize)> {
  let inner = fragment.strip_prefix("point(")?.strip_suffix(')')?;
  let (path, offset) = inner.split_once(':')?;
  let steps = path
    .strip_prefix('/')?
    .split('/')
    .map(|s| s.parse().ok())
    .collect::<Option<Vec<usize>>>()?;
  if steps.first() != Some(&1) || steps.contains(&0) {
    return None;
  }
  Some((steps, offset.parse().ok()?))
}

/// How Nickel's Adobe point counts nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointFlavor {
  /// Kobo firmware 4.x: comments and CDATA are skipped and the text around
  /// them counts as one node.
  JoinText,
  /// Tolino firmware 5.x: every node counts, comments and CDATA included.
  AllNodes,
}

impl PointFlavor {
  /// From the firmware version (`4.46.23836`, `5.18.264769`).
  pub fn for_firmware(version: &str) -> Self {
    match version
      .split('.')
      .next()
      .and_then(|m| m.parse::<u32>().ok())
    {
      Some(major) if major < 5 => PointFlavor::JoinText,
      _ => PointFlavor::AllNodes,
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct XPointer {
  /// 0-based spine index of the chapter.
  pub spine_index: usize,
  /// `/body[1]/DocFragment[n]…`
  pub xpointer: String,
  /// `false` if only the chapter is known (no fragment, or the fragment did
  /// not resolve).
  pub exact: bool,
}

#[derive(Debug)]
pub enum Error {
  Epub(epub::Error),
  /// The chapter is not in the spine of this file.
  UnknownChapter(String),
  /// Not `/body[1]/DocFragment[n]…`, or `n` is not in the spine.
  BadXPointer(String),
  /// No translation back for this device yet.
  Unsupported(&'static str),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Epub(e) => write!(f, "{e}"),
      Error::UnknownChapter(c) => write!(f, "chapter {c} is not in the spine"),
      Error::BadXPointer(x) => write!(f, "not a KOReader position in this book: {x}"),
      Error::Unsupported(e) => write!(f, "{e}"),
    }
  }
}

impl std::error::Error for Error {}

impl From<epub::Error> for Error {
  fn from(e: epub::Error) -> Self {
    Error::Epub(e)
  }
}

/// Converts a Nickel position in `epub` to a KOReader XPointer. A fragment
/// that does not resolve in the chapter falls back to the chapter start.
pub fn to_xpointer(
  epub: &mut Epub,
  position: &Position,
  flavor: PointFlavor,
) -> Result<XPointer, Error> {
  let spine_index = epub
    .spine_index(&position.chapter)
    .ok_or_else(|| Error::UnknownChapter(position.chapter.clone()))?;
  let chapter = format!("/body[1]/DocFragment[{}]", spine_index + 1);
  let target = match &position.anchor {
    Anchor::None => None,
    anchor => resolve(&epub.chapter(spine_index)?, anchor, flavor),
  };
  Ok(match target {
    Some(xpointer) => XPointer {
      spine_index,
      xpointer: format!("{chapter}{xpointer}"),
      exact: true,
    },
    None => XPointer {
      spine_index,
      xpointer: chapter,
      exact: false,
    },
  })
}

/// A KOReader position translated for the device.
#[derive(Debug, Clone, PartialEq)]
pub struct Translated {
  pub position: Position,
  /// `false` if the XPointer below the chapter did not resolve, so the
  /// position is the chapter's first text.
  pub exact: bool,
}

/// Converts a KOReader XPointer to an Adobe point in `epub`, the inverse of
/// [`to_xpointer`] for the Tolino ([`PointFlavor::AllNodes`]). A path that
/// does not resolve in the chapter falls back to the chapter's first text.
///
/// In a run of spaces KOReader counts one character, so an offset in or
/// after a run maps to the end of the run, the next visible character.
pub fn from_xpointer(
  epub: &mut Epub,
  xpointer: &str,
  flavor: PointFlavor,
) -> Result<Translated, Error> {
  if flavor != PointFlavor::AllNodes {
    return Err(Error::Unsupported(
      "positions back to Kobo firmware 4.x are not translated yet",
    ));
  }
  let bad = || Error::BadXPointer(xpointer.to_string());
  let rest = xpointer
    .strip_prefix("/body[1]/DocFragment[")
    .or_else(|| xpointer.strip_prefix("/body/DocFragment["))
    .ok_or_else(bad)?;
  let (n, below) = rest.split_once(']').ok_or_else(bad)?;
  let spine_index = n
    .parse::<usize>()
    .ok()
    .and_then(|n| n.checked_sub(1))
    .filter(|&i| i < epub.spine.len())
    .ok_or_else(bad)?;
  let chapter = epub.spine[spine_index].clone();
  let html = epub.chapter(spine_index)?;
  let resolved = Some(below)
    .filter(|b| !b.is_empty())
    .and_then(|b| point_for(&html, b));
  let (path, bytes, exact) = match resolved {
    Some((path, bytes)) => (path, bytes, true),
    None => {
      // The chapter's first text, below `body` as KOReader starts there.
      let body = html
        .children
        .iter()
        .position(|c| matches!(c, Node::Element(e) if e.name == "body"));
      let inner = body
        .and_then(|i| match &html.children[i] {
          Node::Element(e) => first_visible_text(e).map(|p| [vec![i], p].concat()),
          _ => None,
        })
        .ok_or_else(|| Error::BadXPointer(format!("{xpointer} (chapter without text)")))?;
      (inner, 0, false)
    }
  };
  let mut steps = vec![1];
  steps.extend(path.iter().map(|i| i + 1));
  Ok(Translated {
    position: Position {
      chapter,
      anchor: Anchor::Point {
        steps,
        offset: bytes,
      },
    },
    exact,
  })
}

/// Child indices from `html` and a UTF-8 byte offset for the part of a
/// KOReader XPointer below `DocFragment[n]`: `/body[1]/p[3]/text()[1].12`.
/// A path ending on an element gives its first text.
fn point_for(html: &Element, below: &str) -> Option<(Vec<usize>, usize)> {
  let (steps, offset) = match below.rsplit_once('.') {
    Some((steps, offset)) if offset.bytes().all(|b| b.is_ascii_digit()) => {
      (steps, offset.parse::<usize>().ok()?)
    }
    _ => (below, 0),
  };
  let mut element = html;
  let mut path = Vec::new();
  for step in steps.strip_prefix('/')?.split('/') {
    let (name, n) = match step.split_once('[') {
      Some((name, n)) => (name, n.strip_suffix(']')?.parse::<usize>().ok()?),
      None => (step, 1),
    };
    if n == 0 {
      return None;
    }
    if name == "text()" {
      // Text nodes by position among text and CDATA siblings.
      let i = element
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, Node::Text(_) | Node::CData(_)))
        .nth(n - 1)?
        .0;
      let (Node::Text(text) | Node::CData(text)) = &element.children[i] else {
        unreachable!()
      };
      path.push(i);
      return Some((path, collapsed_to_bytes(text, offset)));
    }
    let i = element
      .children
      .iter()
      .enumerate()
      .filter(|(_, c)| matches!(c, Node::Element(e) if e.name == name))
      .nth(n - 1)?
      .0;
    let Node::Element(child) = &element.children[i] else {
      unreachable!()
    };
    path.push(i);
    element = child;
  }
  // Ends on an element: its first text.
  path.extend(first_visible_text(element)?);
  Some((path, 0))
}

/// Like [`first_text`], but skips text that is only whitespace (the line
/// break before the first paragraph), as a reader would show it.
fn first_visible_text(element: &Element) -> Option<Vec<usize>> {
  for (i, child) in element.children.iter().enumerate() {
    match child {
      Node::Text(t) | Node::CData(t) if !t.trim().is_empty() => return Some(vec![i]),
      Node::Element(e) => {
        if let Some(mut path) = first_visible_text(e) {
          path.insert(0, i);
          return Some(path);
        }
      }
      _ => {}
    }
  }
  None
}

/// The UTF-8 byte offset in `s` after `collapsed` characters counted as
/// [`collapsed_len`] does: the longest prefix of that length, so an offset
/// right after a collapsed run lands behind the whole run.
fn collapsed_to_bytes(s: &str, collapsed: usize) -> usize {
  let (mut len, mut in_space, mut end) = (0, false, 0);
  for (i, c) in s.char_indices() {
    let space = matches!(c, ' ' | '\t' | '\n' | '\r');
    let next = if space && in_space { len } else { len + 1 };
    if next > collapsed {
      break;
    }
    (len, in_space, end) = (next, space, i + c.len_utf8());
  }
  end
}

/// The XPointer below `DocFragment[n]` for an anchor in the chapter `html`.
fn resolve(html: &Element, anchor: &Anchor, flavor: PointFlavor) -> Option<String> {
  let (path, chars) = match anchor {
    Anchor::None => return None,
    Anchor::KoboSpan { id, offset } => {
      let (span_path, span) = html.find_id(id)?;
      let (text_path, text) = first_text(span)?;
      let mut path = span_path;
      path.extend(text_path);
      (path, utf16_to_chars(text, *offset))
    }
    Anchor::Point { steps, offset } => match flavor {
      PointFlavor::AllNodes => all_nodes_point(html, &steps[1..], *offset)?,
      PointFlavor::JoinText => joined_point(html, &steps[1..], *offset)?,
    },
  };
  koreader_path(html, &path, chars)
}

/// Adobe point counting every child node.
fn all_nodes_point(html: &Element, steps: &[usize], offset: usize) -> Option<(Vec<usize>, usize)> {
  let path: Vec<usize> = steps.iter().map(|s| s - 1).collect();
  match html.node_at(&path)? {
    Node::Text(t) | Node::CData(t) => Some((path.clone(), bytes_to_chars(t, offset))),
    Node::Element(e) => {
      // A point on an element: its first text.
      let (inner, _) = first_text(e)?;
      Some(([path, inner].concat(), 0))
    }
    _ => None,
  }
}

/// Adobe point on Kobo 4.x: in each element, a run of text, CDATA and
/// comments counts as one node whose text is the text and CDATA joined.
fn joined_point(html: &Element, steps: &[usize], offset: usize) -> Option<(Vec<usize>, usize)> {
  let mut element = html;
  let mut path = Vec::new();
  for (depth, &step) in steps.iter().enumerate() {
    let groups = joined_children(element);
    let members = groups.get(step - 1)?;
    let last = depth + 1 == steps.len();
    match members.as_slice() {
      [i] if matches!(element.children[*i], Node::Element(_)) => {
        path.push(*i);
        let Node::Element(e) = &element.children[*i] else {
          unreachable!()
        };
        if last {
          let (inner, _) = first_text(e)?;
          path.extend(inner);
          return Some((path, 0));
        }
        element = e;
      }
      _ if last => {
        // Walk the joined text to the member that holds the offset.
        let mut left = offset;
        let texts: Vec<(usize, &str)> = members
          .iter()
          .filter_map(|&i| match &element.children[i] {
            Node::Text(t) | Node::CData(t) => Some((i, t.as_str())),
            _ => None,
          })
          .collect();
        for (n, &(i, text)) in texts.iter().enumerate() {
          if left < text.len() || n + 1 == texts.len() {
            path.push(i);
            return Some((path, bytes_to_chars(text, left)));
          }
          left -= text.len();
        }
        return None;
      }
      _ => return None,
    }
  }
  None
}

/// Children of an element as Kobo 4.x counts them: each element alone, each
/// run of text, CDATA and comments together (indices into `children`).
fn joined_children(element: &Element) -> Vec<Vec<usize>> {
  let mut groups: Vec<Vec<usize>> = Vec::new();
  let mut in_text = false;
  for (i, child) in element.children.iter().enumerate() {
    match child {
      Node::Text(_) | Node::CData(_) | Node::Comment(_) => {
        if in_text {
          groups.last_mut().unwrap().push(i);
        } else {
          groups.push(vec![i]);
          in_text = true;
        }
      }
      _ => {
        groups.push(vec![i]);
        in_text = false;
      }
    }
  }
  groups
}

/// The first text or CDATA node below `element`, as a path of child indices.
fn first_text(element: &Element) -> Option<(Vec<usize>, &str)> {
  for (i, child) in element.children.iter().enumerate() {
    match child {
      Node::Text(t) | Node::CData(t) if !t.is_empty() => return Some((vec![i], t)),
      Node::Element(e) => {
        if let Some((mut path, text)) = first_text(e) {
          path.insert(0, i);
          return Some((path, text));
        }
      }
      _ => {}
    }
  }
  None
}

/// KOReader's path from the chapter's `body` to the text node at `path`
/// (child indices from `html`), with the offset converted to collapsed code
/// points.
fn koreader_path(html: &Element, path: &[usize], chars: usize) -> Option<String> {
  let (&text_index, element_path) = path.split_last()?;
  let mut steps = String::new();
  let mut element = html;
  for &i in element_path {
    let Node::Element(child) = &element.children[i] else {
      return None;
    };
    // Elements by tag name: the n-th sibling with the same name.
    let n = element.children[..=i]
      .iter()
      .filter(|c| matches!(c, Node::Element(e) if e.name == child.name))
      .count();
    steps.push_str(&format!("/{}[{n}]", child.name));
    element = child;
  }
  if !steps.starts_with("/body[") {
    return None;
  }
  let text = match &element.children[text_index] {
    Node::Text(t) | Node::CData(t) => t,
    _ => return None,
  };
  // Text nodes by position among text and CDATA siblings; comments do not
  // count but still split the text.
  let n = element.children[..=text_index]
    .iter()
    .filter(|c| matches!(c, Node::Text(_) | Node::CData(_)))
    .count();
  let prefix: String = text.chars().take(chars).collect();
  Some(format!("{steps}/text()[{n}].{}", collapsed_len(&prefix)))
}

/// Code points after collapsing runs of spaces, tabs and line breaks to one
/// space (no-break spaces stay).
fn collapsed_len(s: &str) -> usize {
  let mut len = 0;
  let mut in_space = false;
  for c in s.chars() {
    let space = matches!(c, ' ' | '\t' | '\n' | '\r');
    if !(space && in_space) {
      len += 1;
    }
    in_space = space;
  }
  len
}

/// Number of characters in the first `bytes` UTF-8 bytes of `s`.
fn bytes_to_chars(s: &str, bytes: usize) -> usize {
  s.char_indices().take_while(|(i, _)| *i < bytes).count()
}

/// Number of characters in the first `units` UTF-16 units of `s`.
fn utf16_to_chars(s: &str, units: usize) -> usize {
  let mut seen = 0;
  s.chars()
    .take_while(|c| {
      let fits = seen < units;
      seen += c.len_utf16();
      fits
    })
    .count()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dom::parse;

  #[test]
  fn parses_stored_positions() {
    assert_eq!(
      Position::parse("OEBPS/a.xhtml#kobo.2.1").anchor,
      Anchor::KoboSpan {
        id: "kobo.2.1".into(),
        offset: 0
      }
    );
    assert_eq!(
      Position::parse("OPS/a.html#point(/1/4/16/1:356)").anchor,
      Anchor::Point {
        steps: vec![1, 4, 16, 1],
        offset: 356
      }
    );
    assert_eq!(Position::parse("OEBPS/a.xhtml").anchor, Anchor::None);
    assert_eq!(Position::parse("OEBPS/a.xhtml#aid_1").anchor, Anchor::None);
    assert_eq!(
      Position::parse("a.xhtml#point(/2/4:0)").anchor,
      Anchor::None
    );
  }

  #[test]
  fn flavors() {
    assert_eq!(
      PointFlavor::for_firmware("4.46.23836"),
      PointFlavor::JoinText
    );
    assert_eq!(
      PointFlavor::for_firmware("5.18.264769"),
      PointFlavor::AllNodes
    );
  }

  #[test]
  fn units() {
    assert_eq!(bytes_to_chars("äöü x", 6), 3);
    assert_eq!(utf16_to_chars("a\u{1d11e} b", 3), 2);
    assert_eq!(collapsed_len("   a  b\n\t c"), 6);
  }

  #[test]
  fn collapsed_offsets_back_to_bytes() {
    let text = "\n    Größe  über";
    // Inside the leading run: behind it, at "G".
    assert_eq!(collapsed_to_bytes(text, 0), 0);
    assert_eq!(collapsed_to_bytes(text, 1), 5);
    // "Größe" is 7 bytes; then two spaces count as one.
    assert_eq!(collapsed_to_bytes(text, 6), 12);
    assert_eq!(collapsed_to_bytes(text, 7), 14);
    assert_eq!(collapsed_to_bytes(text, 99), text.len());
    for raw in [0, 5, 7, 14] {
      let chars = bytes_to_chars(text, raw);
      let prefix: String = text.chars().take(chars).collect();
      assert_eq!(collapsed_to_bytes(text, collapsed_len(&prefix)), raw);
    }
  }

  #[test]
  fn xpointer_back_to_point() {
    let html =
      parse("<html><head/><body>\n<p>ab <!--c--> de<em>f</em></p>\n</body></html>").unwrap();
    let point = |below| point_for(&html, below);
    // After the comment: the third child of p, "d" is byte 1.
    assert_eq!(point("/body[1]/p[1]/text()[2].1"), Some((vec![1, 1, 2], 1)));
    assert_eq!(point("/body/p/text().0"), Some((vec![1, 1, 0], 0)));
    // On an element: its first text.
    assert_eq!(point("/body[1]/p[1]/em[1]"), Some((vec![1, 1, 3, 0], 0)));
    assert_eq!(point("/body[1]/p[2]/text()[1].0"), None);
    assert_eq!(point("/body[1]/p[0]"), None);
  }

  #[test]
  fn displays_as_stored() {
    for stored in [
      "OEBPS/a.xhtml",
      "OEBPS/a.xhtml#kobo.2.1",
      "OPS/a.html#point(/1/4/16/1:356)",
    ] {
      assert_eq!(Position::parse(stored).to_string(), stored);
    }
  }

  #[test]
  fn comment_splits_text() {
    let html = parse("<html><body>\n<p>ab <!--c--> de</p>\n</body></html>").unwrap();
    // Tolino: the text after the comment is node 3, offset 1 = "d".
    let tolino = Anchor::Point {
      steps: vec![1, 1, 2, 3],
      offset: 1,
    };
    assert_eq!(
      resolve(&html, &tolino, PointFlavor::AllNodes).as_deref(),
      Some("/body[1]/p[1]/text()[2].1")
    );
    // Kobo 4.x: one joined node, offset 4 = "d".
    let kobo = Anchor::Point {
      steps: vec![1, 1, 2, 1],
      offset: 4,
    };
    assert_eq!(
      resolve(&html, &kobo, PointFlavor::JoinText).as_deref(),
      Some("/body[1]/p[1]/text()[2].1")
    );
  }
}
