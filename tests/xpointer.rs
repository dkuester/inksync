//! Converts positions stored on the devices to KOReader XPointers and
//! compares them with what KOReader itself stored for the same place:
//! highlights of the same words in Book P and Book Q on the Kobo 4.46 and
//! the Tolino 5.18, and reading positions in Book A (see
//! tests/fixtures/README.md).

use std::fs;
use std::path::{Path, PathBuf};

use nickelinksync::epub::Epub;
use nickelinksync::position::{to_xpointer, Anchor, PointFlavor, Position};
use rusqlite::Connection;

fn fixtures() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load(sql: &str) -> Connection {
  let db = Connection::open_in_memory().unwrap();
  db.execute_batch(&fs::read_to_string(fixtures().join("db").join(sql)).unwrap())
    .unwrap();
  db
}

fn epub(file: &str) -> Epub {
  Epub::open(&fixtures().join("epub").join(file)).unwrap()
}

/// `["key"] = "value",` lines of a KOReader metadata.epub.lua, in order.
fn lua_values(sdr: &str, key: &str) -> Vec<String> {
  let lua = fs::read_to_string(
    fixtures()
      .join("koreader")
      .join(sdr)
      .join("metadata.epub.lua"),
  )
  .unwrap();
  let prefix = format!("[\"{key}\"] = \"");
  lua
    .lines()
    .filter_map(|l| l.trim().strip_prefix(&prefix)?.strip_suffix("\","))
    .map(str::to_string)
    .collect()
}

/// KOReader's highlight start (`pos0`) for a word.
fn koreader_pos0(sdr: &str, word: &str) -> String {
  let words = lua_values(sdr, "text");
  let pos = lua_values(sdr, "pos0");
  let i = words
    .iter()
    .position(|w| w == word)
    .unwrap_or_else(|| panic!("{word} not in {sdr}"));
  pos[i].clone()
}

/// The device highlights of a book as (word, position of the word's start).
fn device_highlights(db: &Connection, file: &str) -> Vec<(String, Position)> {
  let mut stmt = db
    .prepare(
      "SELECT Text, ContentID, StartContainerPath, StartOffset FROM Bookmark
       WHERE VolumeID = ?1 ORDER BY trim(Text)",
    )
    .unwrap();
  stmt
    .query_map([format!("file:///mnt/onboard/Books/{file}")], |row| {
      let text: String = row.get(0)?;
      let content_id: String = row.get(1)?;
      let container: String = row.get(2)?;
      let offset = row.get::<_, i64>(3)? as usize;
      // A selection that starts at the space before the word.
      let lead = text.len() - text.trim_start().len();
      let position = if let Some(id) = container.strip_prefix("span#") {
        // Kepub: chapter from `<path>!<opf dir>!<href>`, offset in StartOffset.
        let mut parts = content_id.rsplitn(3, '!');
        let href = parts.next().unwrap();
        let dir = parts.next().unwrap();
        Position {
          chapter: if dir.is_empty() {
            href.to_string()
          } else {
            format!("{dir}/{href}")
          },
          anchor: Anchor::KoboSpan {
            id: id.replace('\\', ""),
            offset: offset + lead,
          },
        }
      } else {
        let mut p = Position::parse(&container);
        if let Anchor::Point { offset, .. } = &mut p.anchor {
          *offset += lead;
        }
        p
      };
      Ok((text.trim().to_string(), position))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn check_highlights(sql: &str, flavor: PointFlavor, file: &str, sdr: &str, count: usize) {
  let db = load(sql);
  let mut book = epub(file);
  let highlights = device_highlights(&db, file);
  assert_eq!(highlights.len(), count, "{sql} {file}");
  for (word, position) in highlights {
    let got = to_xpointer(&mut book, &position, flavor).unwrap();
    assert!(got.exact, "{sql} {file} {word}: {position:?}");
    assert_eq!(
      got.xpointer,
      koreader_pos0(sdr, &word),
      "{sql} {file} {word}: {position:?}"
    );
  }
}

#[test]
fn kobo_kepub_highlights() {
  for (file, sdr) in [
    ("book-p.kepub.epub", "book-p.kepub.sdr"),
    ("book-q.kepub.epub", "book-q.kepub.sdr"),
  ] {
    check_highlights(
      "kobo-clarabw-4.46-probe-books.sql",
      PointFlavor::JoinText,
      file,
      sdr,
      8,
    );
  }
}

#[test]
fn kobo_epub_highlights() {
  for (file, sdr) in [("book-p.epub", "book-p.sdr"), ("book-q.epub", "book-q.sdr")] {
    check_highlights(
      "kobo-clarabw-4.46-probe-books.sql",
      PointFlavor::JoinText,
      file,
      sdr,
      8,
    );
  }
}

#[test]
fn tolino_epub_highlights() {
  for (file, sdr) in [("book-p.epub", "book-p.sdr"), ("book-q.epub", "book-q.sdr")] {
    check_highlights(
      "tolino-shinecolor-5.18-probe-books.sql",
      PointFlavor::AllNodes,
      file,
      sdr,
      8,
    );
  }
}

/// `ChapterIDBookmarked` of a book in a fixture.
fn stored(sql: &str, file: &str) -> Position {
  let db = load(sql);
  let value: String = db
    .query_row(
      "SELECT ChapterIDBookmarked FROM content WHERE ContentID = ?1",
      [format!("file:///mnt/onboard/Books/{file}")],
      |row| row.get(0),
    )
    .unwrap();
  Position::parse(&value)
}

#[test]
fn reading_positions_at_a_chapter_heading() {
  // Kobo, kepub: span kobo.1.1 at the heading of xhtml-0-81.xhtml.
  let kobo = stored(
    "kobo-clarabw-4.46-placeholder-books.sql",
    "book-a.kepub.epub",
  );
  let got = to_xpointer(&mut epub("book-a.kepub.epub"), &kobo, PointFlavor::JoinText).unwrap();
  assert_eq!(got.spine_index, 81);
  assert_eq!(
    got.xpointer,
    lua_values("book-a.kepub.sdr", "last_xpointer")[0]
  );

  // Tolino, plain EPUB: point(/1/2/1/1:0) at the same heading.
  let tolino = stored(
    "tolino-shinecolor-5.18-placeholder-books.sql",
    "book-a.epub",
  );
  let got = to_xpointer(&mut epub("book-a.epub"), &tolino, PointFlavor::AllNodes).unwrap();
  assert_eq!(got.xpointer, lua_values("book-a.sdr", "last_xpointer")[0]);
}

#[test]
fn chapter_only_without_fragment() {
  // Kobo, plain EPUB at a chapter start: no fragment, so the chapter only.
  let kobo = stored("kobo-clarabw-4.46-placeholder-books.sql", "book-a.epub");
  assert_eq!(kobo.anchor, Anchor::None);
  let got = to_xpointer(&mut epub("book-a.epub"), &kobo, PointFlavor::JoinText).unwrap();
  assert_eq!((got.spine_index, got.exact), (7, false));
  assert_eq!(got.xpointer, "/body[1]/DocFragment[8]");
}

#[test]
fn unknown_chapter() {
  let position = Position::parse("OEBPS/missing.xhtml#kobo.1.1");
  assert!(to_xpointer(
    &mut epub("book-a.kepub.epub"),
    &position,
    PointFlavor::JoinText
  )
  .is_err());
}
