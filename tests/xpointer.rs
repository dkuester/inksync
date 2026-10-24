//! Converts positions stored on the devices to KOReader XPointers and
//! compares them with what KOReader itself stored for the same place:
//! highlights of the same words in Book P and Book Q on the Kobo 4.46 and
//! the Tolino 5.18, and reading positions in Book A (see
//! tests/fixtures/README.md). The Tolino positions are also translated back
//! and must come out as stored.

use std::fs;
use std::path::{Path, PathBuf};

use nickelinksync::epub::Epub;
use nickelinksync::position::{from_xpointer, to_xpointer, Anchor, PointFlavor, Position};
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

/// Book R (`make_long_epub.py`): a position deep inside a long chapter, where
/// a reader that only finds the chapter would show a different page.
#[test]
fn long_book_mid_chapter() {
  let xpointer = |file, stored, flavor| {
    let x = to_xpointer(&mut epub(file), &Position::parse(stored), flavor).unwrap();
    assert!(x.exact, "{stored}");
    assert_eq!(x.spine_index, 2);
    x.xpointer
  };
  // `R2-117 ` is seven characters, so offset 7 is the first filler word.
  for flavor in [PointFlavor::AllNodes, PointFlavor::JoinText] {
    assert_eq!(
      xpointer(
        "book-r.epub",
        "OEBPS/text/chapter2.xhtml#point(/1/4/236/1:7)",
        flavor
      ),
      "/body[1]/DocFragment[3]/body[1]/p[117]/text()[1].7"
    );
  }
  assert_eq!(
    xpointer(
      "book-r.kepub.epub",
      "OEBPS/text/chapter2.xhtml#kobo.118.1",
      PointFlavor::JoinText
    ),
    "/body[1]/DocFragment[3]/body[1]/div[1]/div[1]/p[117]/span[1]/text()[1].0"
  );
}

/// Tolino highlights there and back, and KOReader's highlights of the same
/// words back to exactly the Tolino's points.
#[test]
fn tolino_highlights_round_trip() {
  let db = load("tolino-shinecolor-5.18-probe-books.sql");
  let flavor = PointFlavor::AllNodes;
  for (file, sdr) in [("book-p.epub", "book-p.sdr"), ("book-q.epub", "book-q.sdr")] {
    let mut book = epub(file);
    let highlights = device_highlights(&db, file);
    assert_eq!(highlights.len(), 8, "{file}");
    for (word, position) in highlights {
      let there = to_xpointer(&mut book, &position, flavor).unwrap();
      let back = from_xpointer(&mut book, &there.xpointer, flavor).unwrap();
      assert!(back.exact, "{file} {word}");
      assert_eq!(back.position, position, "{file} {word}: {}", there.xpointer);

      let koreader = from_xpointer(&mut book, &koreader_pos0(sdr, &word), flavor).unwrap();
      assert_eq!(koreader.position, position, "{file} {word} from KOReader");
    }
  }
}

#[test]
fn tolino_reading_positions_round_trip() {
  let flavor = PointFlavor::AllNodes;
  let tolino = stored(
    "tolino-shinecolor-5.18-placeholder-books.sql",
    "book-a.epub",
  );
  let mut book = epub("book-a.epub");
  let there = to_xpointer(&mut book, &tolino, flavor).unwrap();
  let back = from_xpointer(&mut book, &there.xpointer, flavor).unwrap();
  assert_eq!(back.position, tolino);

  // Book R: the page the Tolino pushed to kosync.eu, and a mid-chapter word.
  let mut book = epub("book-r.epub");
  for stored in [
    "OEBPS/text/chapter1.xhtml#point(/1/4/314/1:264)",
    "OEBPS/text/chapter2.xhtml#point(/1/4/236/1:7)",
  ] {
    let position = Position::parse(stored);
    let there = to_xpointer(&mut book, &position, flavor).unwrap();
    let back = from_xpointer(&mut book, &there.xpointer, flavor).unwrap();
    assert_eq!(back.position.to_string(), stored, "{}", there.xpointer);
  }
  // Where KOReader left Book R on the server: the start of R2-63.
  let back = from_xpointer(
    &mut book,
    "/body[1]/DocFragment[3]/body[1]/p[63]/text()[1].0",
    flavor,
  )
  .unwrap();
  assert!(back.exact);
  assert_eq!(
    back.position.to_string(),
    "OEBPS/text/chapter2.xhtml#point(/1/4/128/1:0)"
  );
}

#[test]
fn xpointer_fallbacks() {
  let flavor = PointFlavor::AllNodes;
  let mut book = epub("book-r.epub");
  // Chapter only, or a path the chapter does not have: its first text.
  for xpointer in [
    "/body[1]/DocFragment[3]",
    "/body[1]/DocFragment[3]/body[1]/p[999]/text()[1].0",
  ] {
    let back = from_xpointer(&mut book, xpointer, flavor).unwrap();
    assert!(!back.exact, "{xpointer}");
    assert_eq!(
      back.position.to_string(),
      "OEBPS/text/chapter2.xhtml#point(/1/4/2/1:0)"
    );
  }
  for xpointer in [
    "/body[1]/DocFragment[99]/body[1]",
    "/body[1]/DocFragment[0]",
    "epubcfi(/6/4!/4/2)",
  ] {
    assert!(
      from_xpointer(&mut book, xpointer, flavor).is_err(),
      "{xpointer}"
    );
  }
  // Back to Kobo 4.x is not done yet.
  assert!(from_xpointer(&mut book, "/body[1]/DocFragment[3]", PointFlavor::JoinText).is_err());
}
