//! Kindle position → KOReader XPointer.
//!
//! A Kindle position names a KFX block (`eid`) and an offset in code points
//! into the block's converted text ([`crate::krds`], [`crate::kfx`]). KFX
//! section *n* is spine item *n*, and its text blocks are the chapter's
//! block elements in order, so block *k* of the section is the *k*-th block
//! element with text in the EPUB chapter. Within the block, the KFX text
//! differs from the EPUB source (checked on Kindle firmware 5.19.2, see
//! SCOPE.md):
//!
//! | Source | KFX text | Handled here |
//! |---|---|---|
//! | whitespace runs | one space | collapsed the same way |
//! | leading whitespace of a block | dropped | dropped, then added back to the offset |
//! | no-break space | plain space | one character either way |
//! | inline element (`em`) | flattened into the block | the offset is split at the element |
//! | comment | dropped, text joined | the offset is split at the comment |
//! | CDATA | text lost | blocks differ: matched from both ends, [`XPointer::exact`] is `false` |
//!
//! The EPUB block's text is built the way the Kindle converts it, each
//! character remembering its text node, then the KFX offset is looked up in
//! it and turned into KOReader's node path and collapsed offset.

use std::fmt;

use crate::dom::{Element, Node};
use crate::epub::{self, Epub};
use crate::kfx::Kfx;
use crate::krds;
use crate::position::{koreader_path, XPointer};

#[derive(Debug)]
pub enum Error {
  Epub(epub::Error),
  /// No text block with this `eid` in the KFX.
  UnknownBlock(u32),
  /// The KFX position maps give another `pid`: the position was stored for
  /// a different conversion of the book.
  StalePosition {
    stored: krds::Position,
    computed: Option<u32>,
  },
  /// The KFX has more sections than the EPUB has spine items.
  NotInSpine(usize),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Epub(e) => write!(f, "{e}"),
      Error::UnknownBlock(eid) => write!(f, "no text block with eid {eid} in the KFX"),
      Error::StalePosition { stored, computed } => write!(
        f,
        "position {stored} does not fit the KFX (its maps give pid {})",
        computed.map_or("none".to_string(), |p| p.to_string())
      ),
      Error::NotInSpine(n) => write!(f, "KFX section {n} has no spine item in the EPUB"),
    }
  }
}

impl std::error::Error for Error {}

impl From<epub::Error> for Error {
  fn from(e: epub::Error) -> Self {
    Error::Epub(e)
  }
}

/// Converts a stored Kindle position to a KOReader XPointer in `epub`, the
/// EPUB the KFX was made from. The position's `pid` must match the KFX.
pub fn to_xpointer(
  kfx: &Kfx,
  epub: &mut Epub,
  position: &krds::Position,
) -> Result<XPointer, Error> {
  let computed = kfx.pid(position.eid, position.offset);
  if computed != Some(position.pid) {
    return Err(Error::StalePosition {
      stored: *position,
      computed,
    });
  }
  block_to_xpointer(kfx, epub, position.eid, position.offset)
}

/// Converts a block `eid` and an offset in code points into its KFX text to
/// a KOReader XPointer in `epub`. If the block cannot be found in the
/// chapter, the result is the chapter start, not exact.
pub fn block_to_xpointer(
  kfx: &Kfx,
  epub: &mut Epub,
  eid: u32,
  offset: u32,
) -> Result<XPointer, Error> {
  let (spine_index, block_index, block) = kfx.block(eid).ok_or(Error::UnknownBlock(eid))?;
  if spine_index >= epub.spine.len() {
    return Err(Error::NotInSpine(spine_index));
  }
  let chapter = format!("/body[1]/DocFragment[{}]", spine_index + 1);
  let html = epub.chapter(spine_index)?;
  let blocks = epub_blocks(&html);
  let kfx_texts: Vec<&str> = kfx.sections[spine_index]
    .blocks
    .iter()
    .map(|b| b.text.as_str())
    .collect();
  let found = match_block(&kfx_texts, block_index, &blocks)
    .and_then(|b| locate(&html, &blocks[b], &block.text, offset as usize));
  Ok(match found {
    Some((below, exact)) => XPointer {
      spine_index,
      xpointer: format!("{chapter}{below}"),
      exact,
    },
    None => XPointer {
      spine_index,
      xpointer: chapter,
      exact: false,
    },
  })
}

/// Elements that the conversion flattens into the surrounding block.
const INLINE: &[&str] = &[
  "a", "abbr", "acronym", "b", "bdi", "bdo", "big", "br", "cite", "code", "data", "del", "dfn",
  "em", "font", "i", "img", "ins", "kbd", "mark", "q", "rb", "rp", "rt", "rtc", "ruby", "s",
  "samp", "small", "span", "strike", "strong", "sub", "sup", "time", "tt", "u", "var", "wbr",
];

/// A block element of an EPUB chapter with its text as the Kindle converts
/// it.
#[derive(Debug)]
struct EpubBlock {
  /// The text nodes (text and CDATA) of the block and its inline elements,
  /// as paths of child indices from `html`.
  nodes: Vec<Vec<usize>>,
  /// The converted text.
  text: Vec<char>,
  /// For each character of `text`: index into `nodes` and the character
  /// index in that node.
  origin: Vec<(usize, usize)>,
}

/// The chapter's block elements that hold text, in document order.
fn epub_blocks(html: &Element) -> Vec<EpubBlock> {
  let mut out = Vec::new();
  if let Some(i) = html
    .children
    .iter()
    .position(|c| matches!(c, Node::Element(e) if e.name == "body"))
  {
    if let Node::Element(body) = &html.children[i] {
      walk_block(body, vec![i], &mut out);
    }
  }
  out
}

fn walk_block(element: &Element, path: Vec<usize>, out: &mut Vec<EpubBlock>) {
  let mut texts = Vec::new();
  let mut nested = Vec::new();
  gather(element, &path, &mut texts, &mut nested);
  let block = convert(texts);
  if !block.text.is_empty() {
    out.push(block);
  }
  for (child_path, child) in nested {
    walk_block(child, child_path, out);
  }
}

/// Text nodes of `element` and its inline descendants, and its nested block
/// elements.
fn gather<'a>(
  element: &'a Element,
  path: &[usize],
  texts: &mut Vec<(Vec<usize>, &'a str)>,
  nested: &mut Vec<(Vec<usize>, &'a Element)>,
) {
  for (i, child) in element.children.iter().enumerate() {
    let child_path = [path, &[i]].concat();
    match child {
      Node::Text(t) | Node::CData(t) => texts.push((child_path, t)),
      Node::Element(e) if INLINE.contains(&e.name.as_str()) => {
        gather(e, &child_path, texts, nested)
      }
      Node::Element(e) => nested.push((child_path, e)),
      Node::Comment(_) | Node::Other => {}
    }
  }
}

/// Builds the converted text: whitespace runs (also across nodes) become
/// one space, leading and trailing whitespace is dropped, a no-break space
/// is a plain space.
fn convert(texts: Vec<(Vec<usize>, &str)>) -> EpubBlock {
  let mut block = EpubBlock {
    nodes: Vec::new(),
    text: Vec::new(),
    origin: Vec::new(),
  };
  let mut space = None;
  for (n, (path, text)) in texts.into_iter().enumerate() {
    block.nodes.push(path);
    for (k, c) in text.chars().enumerate() {
      if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
        if !block.text.is_empty() && space.is_none() {
          space = Some((n, k));
        }
        continue;
      }
      if let Some(at) = space.take() {
        block.text.push(' ');
        block.origin.push(at);
      }
      block.text.push(if c == '\u{a0}' { ' ' } else { c });
      block.origin.push((n, k));
    }
  }
  block
}

/// The EPUB block for KFX block `index` of a section: by position if both
/// have the same number of blocks, else the block with the same text
/// nearest to that position.
fn match_block(kfx: &[&str], index: usize, epub: &[EpubBlock]) -> Option<usize> {
  if kfx.len() == epub.len() {
    return Some(index);
  }
  let target: Vec<char> = kfx.get(index)?.chars().collect();
  epub
    .iter()
    .enumerate()
    .filter(|(_, b)| b.text == target)
    .min_by_key(|(i, _)| i.abs_diff(index))
    .map(|(i, _)| i)
}

/// KOReader's path below `DocFragment[n]` for `offset` into the KFX text
/// `kfx` of the EPUB block `block`, and whether the texts agree.
fn locate(html: &Element, block: &EpubBlock, kfx: &str, offset: usize) -> Option<(String, bool)> {
  let kfx: Vec<char> = kfx.chars().collect();
  let epub = &block.text;
  let exact = kfx == *epub;
  let index = if exact {
    offset
  } else {
    // One region differs (CDATA): match the text before it from the start
    // and the text after it from the end.
    let prefix = kfx.iter().zip(epub).take_while(|(a, b)| a == b).count();
    let max_suffix = kfx.len().min(epub.len()) - prefix;
    let suffix = kfx
      .iter()
      .rev()
      .zip(epub.iter().rev())
      .take(max_suffix)
      .take_while(|(a, b)| a == b)
      .count();
    if offset < prefix {
      offset
    } else if offset >= kfx.len() - suffix {
      (epub.len() + offset).checked_sub(kfx.len())?
    } else {
      prefix
    }
  };
  let (node, chars) = match block.origin.get(index) {
    Some(&at) => at,
    // At or after the end of the block: behind its last character.
    None => {
      let &(node, k) = block.origin.last()?;
      (node, k + 1)
    }
  };
  Some((koreader_path(html, &block.nodes[node], chars)?, exact))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dom::parse;

  fn blocks(xhtml: &str) -> (Element, Vec<EpubBlock>) {
    let html = parse(xhtml).unwrap();
    let blocks = epub_blocks(&html);
    (html, blocks)
  }

  fn text(b: &EpubBlock) -> String {
    b.text.iter().collect()
  }

  #[test]
  fn converts_blocks_like_the_kindle() {
    let (_, b) = blocks(
      "<html><head><title>t</title></head><body>\n<h1>A</h1>\n<div>\n<p>  x <em>y</em>\n z<!--c--> w&#160;v </p></div>\n<p><img/></p></body></html>",
    );
    let texts: Vec<_> = b.iter().map(text).collect();
    assert_eq!(texts, ["A", "x y z w v"]);
  }

  #[test]
  fn splits_offsets_at_inline_elements_and_comments() {
    let (html, b) =
      blocks("<html><body>\n<p>  ab <em>cd ef</em> gh <!-- c --> ij</p>\n</body></html>");
    let kfx = "ab cd ef gh ij";
    let at = |o| locate(&html, &b[0], kfx, o).unwrap();
    assert_eq!(at(0), ("/body[1]/p[1]/text()[1].1".into(), true));
    assert_eq!(at(6), ("/body[1]/p[1]/em[1]/text()[1].3".into(), true));
    assert_eq!(at(9), ("/body[1]/p[1]/text()[2].1".into(), true));
    assert_eq!(at(12), ("/body[1]/p[1]/text()[3].1".into(), true));
    assert_eq!(at(14), ("/body[1]/p[1]/text()[3].3".into(), true));
  }

  #[test]
  fn matches_differing_text_from_both_ends() {
    let (html, b) = blocks("<html><body><p>a <![CDATA[b <c>]]> d e</p></body></html>");
    let kfx = "a ]]> d e";
    let at = |o| locate(&html, &b[0], kfx, o).unwrap();
    assert_eq!(at(0), ("/body[1]/p[1]/text()[1].0".into(), false));
    assert_eq!(at(6), ("/body[1]/p[1]/text()[3].1".into(), false));
    // Inside the differing part: its start in the EPUB.
    assert_eq!(at(3), ("/body[1]/p[1]/text()[2].0".into(), false));
  }

  #[test]
  fn finds_blocks_by_text_when_counts_differ() {
    let (_, b) = blocks("<html><body><p>a</p><p>b</p><p>c</p></body></html>");
    assert_eq!(match_block(&["a", "c"], 1, &b), Some(2));
    assert_eq!(match_block(&["a", "x"], 1, &b), None);
    assert_eq!(match_block(&["a", "x", "c"], 1, &b), Some(1));
  }
}
