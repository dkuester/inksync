//! Access to the Nickel device database (`.kobo/KoboReader.sqlite`).
//!
//! Lists sideloaded books and reads their stored reading position. The
//! database is opened read-only unless a write is explicitly requested.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

/// `ContentID` prefix of sideloaded books; store books have UUIDs instead.
pub const ONBOARD: &str = "file:///mnt/onboard/";

#[derive(Debug)]
pub enum Error {
  /// The directory has no `.kobo/KoboReader.sqlite`.
  NotADevice(PathBuf),
  Io(PathBuf, io::Error),
  Sqlite(rusqlite::Error),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::NotADevice(root) => write!(
        f,
        "{}: no .kobo/KoboReader.sqlite, not a Kobo or Tolino",
        root.display()
      ),
      Error::Io(path, e) => write!(f, "{}: {e}", path.display()),
      Error::Sqlite(e) => write!(f, "device database: {e}"),
    }
  }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
  fn from(e: rusqlite::Error) -> Self {
    Error::Sqlite(e)
  }
}

/// A Kobo or Tolino whose storage is mounted (USB) or a directory with the
/// same layout.
pub struct Device {
  pub root: PathBuf,
  /// `None` if `.kobo/version` is missing or unreadable.
  pub firmware: Option<Firmware>,
  pub db: DeviceDb,
}

impl Device {
  /// Opens the device at `root`. The database is opened as a snapshot (see
  /// [`DeviceDb::open_snapshot`]): nothing is written to the device.
  pub fn open(root: &Path) -> Result<Self, Error> {
    let db_path = root.join(".kobo/KoboReader.sqlite");
    if !db_path.is_file() {
      return Err(Error::NotADevice(root.to_path_buf()));
    }
    let firmware = std::fs::read_to_string(root.join(".kobo/version"))
      .ok()
      .and_then(|line| Firmware::parse(&line));
    Ok(Device {
      root: root.to_path_buf(),
      firmware,
      db: DeviceDb::open_snapshot(&db_path)?,
    })
  }

  /// Where the book file is on the mounted storage.
  pub fn book_file(&self, book: &Book) -> PathBuf {
    self.root.join(book.onboard_path())
  }
}

/// From `.kobo/version`: serial, kernel, firmware, …, model ID.
pub struct Firmware {
  pub version: String,
  pub model_id: String,
}

impl Firmware {
  /// The third field is the firmware version, the last one the model ID. The
  /// first field (the serial number) is not kept.
  pub fn parse(line: &str) -> Option<Self> {
    let fields: Vec<_> = line.trim().split(',').collect();
    if fields.len() < 4 {
      return None;
    }
    Some(Firmware {
      version: fields[2].to_string(),
      model_id: fields[fields.len() - 1].to_string(),
    })
  }

  /// Model name for the IDs seen on the reference devices.
  pub fn model_name(&self) -> Option<&'static str> {
    match self.model_id.rsplit('-').next()?.trim_start_matches('0') {
      "391" => Some("Kobo Clara BW"),
      "693" => Some("Tolino Shine Color"),
      _ => None,
    }
  }
}

/// A sideloaded book and its stored position.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
  pub content_id: String,
  pub title: Option<String>,
  pub author: Option<String>,
  pub mime_type: String,
  /// `ChapterIDBookmarked`: chapter path from the EPUB root, optionally with
  /// `#kobo.N.M` (kepub on a Kobo) or `#point(...)` (Adobe point). `None`
  /// if the book was never opened.
  pub position: Option<String>,
  /// `___PercentRead`, whole percent.
  pub percent_read: i64,
  /// `DateLastRead`, as stored (ISO 8601, UTC).
  pub last_read: Option<String>,
  /// `ReadStatus`: 0 unread, 1 reading, 2 finished.
  pub read_status: i64,
}

impl Book {
  /// Path relative to the root of the device storage.
  pub fn onboard_path(&self) -> &str {
    self
      .content_id
      .strip_prefix(ONBOARD)
      .unwrap_or(&self.content_id)
  }

  /// Nickel reads the file as a kepub. A Tolino reads a `.kepub.epub` as a
  /// plain EPUB and stores `application/epub+zip`.
  pub fn is_kepub(&self) -> bool {
    self.mime_type == "application/x-kobo-epub+zip"
  }
}

/// The opened `KoboReader.sqlite`.
pub struct DeviceDb {
  conn: Connection,
}

impl DeviceDb {
  /// Opens a database that nothing else writes while it is open: a copy, or
  /// the database of a device mounted over USB (Nickel is not running then).
  /// `immutable=1` makes SQLite read it without locking and without creating
  /// journal files next to it.
  pub fn open_snapshot(path: &Path) -> Result<Self, rusqlite::Error> {
    let conn = Connection::open_with_flags(
      format!("{}?immutable=1", file_uri(path)),
      OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    Ok(DeviceDb { conn })
  }

  /// Wraps an open connection, for tests with an in-memory database.
  pub fn from_connection(conn: Connection) -> Self {
    DeviceDb { conn }
  }

  /// `DbVersion`: 176 on Kobo 4.46, 220 on Tolino 5.18.
  pub fn version(&self) -> Result<i64, rusqlite::Error> {
    self
      .conn
      .query_row("SELECT version FROM DbVersion", [], |row| row.get(0))
  }

  /// Tolinos have extra `tolino_*` tables (cloud, DRM, Skoobe).
  pub fn is_tolino(&self) -> Result<bool, rusqlite::Error> {
    self.conn.query_row(
      "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name LIKE 'tolino\\_%' ESCAPE '\\')",
      [],
      |row| row.get(0),
    )
  }

  /// Books with a `ContentID` under `file:///mnt/onboard/`, by path.
  pub fn sideloaded_books(&self) -> Result<Vec<Book>, rusqlite::Error> {
    let mut stmt = self.conn.prepare(
      "SELECT ContentID, Title, Attribution, MimeType, ChapterIDBookmarked,
              ___PercentRead, DateLastRead, ReadStatus
       FROM content
       WHERE ContentType = '6' AND ContentID LIKE ?1 || '%'
       ORDER BY ContentID",
    )?;
    let books = stmt.query_map([ONBOARD], |row| {
      Ok(Book {
        content_id: row.get(0)?,
        title: row.get(1)?,
        author: row.get(2)?,
        mime_type: row.get(3)?,
        position: row.get::<_, Option<String>>(4)?.filter(|p| !p.is_empty()),
        percent_read: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
        last_read: row.get(6)?,
        read_status: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
      })
    })?;
    books.collect()
  }
}

/// `file:` URI for SQLite, with the characters that have a meaning in a URI
/// percent-encoded.
fn file_uri(path: &Path) -> String {
  let mut uri = String::from("file:");
  for b in path.to_string_lossy().bytes() {
    match b {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
        uri.push(b as char)
      }
      _ => uri.push_str(&format!("%{b:02X}")),
    }
  }
  uri
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn firmware_from_version_line() {
    let fw = Firmware::parse(
      "N000000000000,4.9.77,5.18.264769,4.9.77,4.9.77,00000000-0000-0000-0000-000000000693\n",
    )
    .unwrap();
    assert_eq!(fw.version, "5.18.264769");
    assert_eq!(fw.model_name(), Some("Tolino Shine Color"));
    assert!(Firmware::parse("garbage").is_none());
  }

  #[test]
  fn uri_escapes_spaces_and_question_marks() {
    assert_eq!(
      file_uri(Path::new("/Volumes/My Kobo/a?b#c.sqlite")),
      "file:/Volumes/My%20Kobo/a%3Fb%23c.sqlite"
    );
  }
}
