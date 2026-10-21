//! Checks the anonymized device database fixtures in `tests/fixtures/db` (see
//! `tests/fixtures/README.md`): each one loads into SQLite and has the
//! structure the database reader relies on.

use rusqlite::{Connection, OptionalExtension};

fn load(name: &str) -> Connection {
  let path = format!("{}/tests/fixtures/db/{name}", env!("CARGO_MANIFEST_DIR"));
  let sql = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
  let db = Connection::open_in_memory().unwrap();
  db.execute_batch(&sql).unwrap();
  db
}

struct Book {
  content_id: String,
  mime_type: String,
  bookmark: String,
  percent_read: i64,
  shortcovers: i64,
}

impl Book {
  /// Chapter path (relative to the EPUB root) and fragment of the position.
  fn position(&self) -> (&str, &str) {
    self
      .bookmark
      .split_once('#')
      .unwrap_or_else(|| panic!("no fragment in {}", self.bookmark))
  }
}

fn sideloaded_books(db: &Connection) -> Vec<Book> {
  let mut stmt = db
    .prepare(
      "SELECT ContentID, MimeType, ChapterIDBookmarked, ___PercentRead, NumShortcovers
       FROM content
       WHERE ContentType = '6' AND ContentID LIKE 'file:///mnt/onboard/%'
       ORDER BY ContentID",
    )
    .unwrap();
  stmt
    .query_map([], |row| {
      Ok(Book {
        content_id: row.get(0)?,
        mime_type: row.get(1)?,
        bookmark: row.get(2)?,
        percent_read: row.get(3)?,
        shortcovers: row.get(4)?,
      })
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn db_version(db: &Connection) -> i64 {
  db.query_row("SELECT version FROM DbVersion", [], |row| row.get(0))
    .unwrap()
}

fn tolino_tables(db: &Connection) -> Vec<String> {
  let mut stmt = db
    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'tolino\\_%' ESCAPE '\\' ORDER BY name")
    .unwrap();
  stmt
    .query_map([], |row| row.get(0))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// Chapter rows of a book as (`ContentType`, `VolumeIndex`, `ContentID`,
/// `adobe_location`), in `VolumeIndex` order.
fn chapters(db: &Connection, book: &Book) -> Vec<(String, i64, String, Option<String>)> {
  let mut stmt = db
    .prepare(
      "SELECT ContentType, VolumeIndex, ContentID, adobe_location FROM content
       WHERE BookID = ?1 ORDER BY ContentType, VolumeIndex",
    )
    .unwrap();
  stmt
    .query_map([&book.content_id], |row| {
      Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// `kobo.N.M`, the ID of a span that kepubify or calibre inserted.
fn is_kobo_span(fragment: &str) -> bool {
  let Some(rest) = fragment.strip_prefix("kobo.") else {
    return false;
  };
  let parts: Vec<_> = rest.split('.').collect();
  parts.len() == 2
    && parts
      .iter()
      .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `point(/a/b/…:offset)`, an Adobe point: 1-based node indices and a
/// character offset.
fn is_adobe_point(fragment: &str) -> bool {
  let Some(inner) = fragment
    .strip_prefix("point(")
    .and_then(|f| f.strip_suffix(')'))
  else {
    return false;
  };
  let Some((path, offset)) = inner.split_once(':') else {
    return false;
  };
  let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
  path.starts_with("/1/") && path[1..].split('/').all(numeric) && numeric(offset)
}

fn is_uuid(s: &str) -> bool {
  let groups: Vec<_> = s.split('-').collect();
  groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
    && groups
      .iter()
      .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The table must exist and hold no rows.
fn assert_empty(db: &Connection, table: &str) {
  let count: i64 = db
    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
      row.get(0)
    })
    .unwrap();
  assert_eq!(count, 0, "{table} must stay empty in a fixture");
}

#[test]
fn kobo_clarabw_4_46() {
  let db = load("kobo-clarabw-4.46.sql");
  assert_eq!(db_version(&db), 176);
  assert!(
    tolino_tables(&db).is_empty(),
    "a Kobo has no tolino_* tables"
  );

  let books = sideloaded_books(&db);
  let ids: Vec<_> = books.iter().map(|b| b.content_id.as_str()).collect();
  assert_eq!(
    ids,
    [
      "file:///mnt/onboard/Books/book-a.kepub.epub",
      "file:///mnt/onboard/Books/book-b.kepub.epub",
    ]
  );
  // (book, spine index of the chapter with the stored position, percent read)
  let expected = [(&books[0], 81, 75), (&books[1], 5, 0)];
  for (book, spine_index, percent) in expected {
    assert_eq!(book.mime_type, "application/x-kobo-epub+zip");
    assert_eq!(book.percent_read, percent);
    let (chapter, fragment) = book.position();
    assert!(is_kobo_span(fragment), "{}", book.bookmark);

    // ContentType 9 rows are the spine: one per item, VolumeIndex 0..n-1,
    // NumShortcovers of the book row is their count. ContentType 899 rows
    // are table-of-contents entries with an index sequence of their own.
    let rows = chapters(&db, book);
    let spine: Vec<_> = rows.iter().filter(|r| r.0 == "9").collect();
    assert!(rows.iter().all(|r| r.0 == "9" || r.0 == "899"));
    assert_eq!(spine.len() as i64, book.shortcovers);
    assert!(spine.iter().enumerate().all(|(i, r)| r.1 == i as i64));

    // Chapter row ContentID: <path>!<OPF dir>!<href relative to the OPF
    // dir>. The stored position is relative to the EPUB root instead.
    let path = book.content_id.strip_prefix("file://").unwrap();
    let row_id = &spine[spine_index as usize].2;
    let (opf_dir, href) = row_id
      .strip_prefix(path)
      .and_then(|rest| rest.strip_prefix('!'))
      .and_then(|rest| rest.split_once('!'))
      .unwrap_or_else(|| panic!("unexpected chapter ContentID {row_id}"));
    let from_root = if opf_dir.is_empty() {
      href.to_string()
    } else {
      format!("{opf_dir}/{href}")
    };
    assert_eq!(from_root, chapter);

    // The same row through BookID + VolumeIndex, as the reader will query it.
    let found: Option<String> = db
      .query_row(
        "SELECT ContentID FROM content WHERE BookID = ?1 AND ContentType = '9' AND VolumeIndex = ?2",
        rusqlite::params![book.content_id, spine_index],
        |row| row.get(0),
      )
      .optional()
      .unwrap();
    assert_eq!(found.as_ref(), Some(row_id));
  }
}

#[test]
fn tolino_shine5_5_18() {
  let db = load("tolino-shine5-5.18.sql");
  assert_eq!(db_version(&db), 220);
  let tables = tolino_tables(&db);
  assert_eq!(
    tables,
    ["tolino_bookmark", "tolino_contentItem", "tolino_dogear"]
  );
  for table in &tables {
    assert_empty(&db, table);
  }

  let books = sideloaded_books(&db);
  let ids: Vec<_> = books.iter().map(|b| b.content_id.as_str()).collect();
  assert_eq!(
    ids,
    [
      "file:///mnt/onboard/Books/book-a.epub",
      "file:///mnt/onboard/book-c.epub",
    ]
  );
  // (book, VolumeIndex of the ToC entry for the chapter with the position, percent read)
  let expected = [(&books[0], Some(80), 74), (&books[1], None, 100)];
  for (book, toc_index, percent) in expected {
    assert_eq!(book.mime_type, "application/epub+zip");
    assert_eq!(book.percent_read, percent);
    let (chapter, fragment) = book.position();
    assert!(is_adobe_point(fragment), "{}", book.bookmark);

    // All chapter rows are ContentType 9 with a UUID as ContentID. They are
    // table-of-contents entries, not spine items: VolumeIndex counts ToC
    // entries, NumShortcovers of the book row is their count, and
    // adobe_location holds the target relative to the EPUB root.
    let rows = chapters(&db, book);
    assert!(rows.iter().all(|r| r.0 == "9" && is_uuid(&r.2)));
    assert_eq!(rows.len() as i64, book.shortcovers);
    assert!(rows.iter().enumerate().all(|(i, r)| r.1 == i as i64));
    let entry = rows.iter().find(|r| {
      r.3
        .as_deref()
        .is_some_and(|loc| loc.split('#').next() == Some(chapter))
    });
    assert_eq!(entry.map(|r| r.1), toc_index, "{}", book.bookmark);
  }
  // Book A is the same book as Book A on the Kobo, where xhtml-0-81 has spine
  // index 81; here its ToC entry has VolumeIndex 80, because one spine item
  // has no ToC entry. Book C's position lies in a spine item without any ToC
  // entry. So on a Tolino the chapter must come from the OPF spine.
}

#[test]
fn position_formats() {
  assert!(is_kobo_span("kobo.1.172"));
  assert!(!is_kobo_span("kobo.1"));
  assert!(!is_kobo_span("kobo.1.x"));
  assert!(is_adobe_point("point(/1/4/16/1:356)"));
  assert!(!is_adobe_point("point(/2/4:0)"));
  assert!(!is_adobe_point("point(/1/4/16/1)"));
  assert!(is_uuid("df2ca96a-5585-4ddd-9176-dbfa891a9b6f"));
  assert!(!is_uuid("/mnt/onboard/book.epub"));
}
