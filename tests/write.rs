//! `pull --write` on fake devices against the local mock server: what is
//! written, the backup, and everything that must leave the database alone.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use common::{fake_device, fixtures, MockServer, KOBO_VERSION, TOLINO_VERSION};
use nickelinksync::hash::partial_md5;
use rusqlite::Connection;

/// `DateLastRead` of book-a.epub on the fake Tolino, 2026-01-03T12:00:00Z.
const LAST_READ: i64 = 1_767_441_600;
/// Further on than the device (spine item 89 against 81), as KOReader would
/// store it.
const AHEAD: &str = "/body[1]/DocFragment[90]/body[1]/p[1]/text()[1].0";
/// How the Tolino stores that place.
const AHEAD_ON_DEVICE: &str = "OEBPS/xhtml/xhtml-0-89.xhtml#point(/1/2/1/1:0)";
const BOOK: &str = "file:///mnt/onboard/Books/book-a.epub";

struct Setup {
  root: PathBuf,
  backups: PathBuf,
  server: MockServer,
  config: PathBuf,
}

impl Setup {
  fn new(name: &str, sql: &str, version: &str) -> Self {
    let root = fake_device(name, sql, version);
    let backups = PathBuf::from(format!("{}-backups", root.display()));
    let _ = fs::remove_dir_all(&backups);
    let server = MockServer::start();
    let config = server.config(&root, common::PASSWORD);
    Setup {
      root,
      backups,
      server,
      config,
    }
  }

  fn tolino(name: &str) -> Self {
    Setup::new(
      name,
      "tolino-shinecolor-5.18-placeholder-books.sql",
      TOLINO_VERSION,
    )
  }

  fn run(&self, args: &[&str]) -> (Output, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_nickelinksync"))
      .args(args)
      .arg("--device")
      .arg(&self.root)
      .arg("--config")
      .arg(&self.config)
      .env("NICKELINKSYNC_BACKUP_DIR", &self.backups)
      .output()
      .unwrap();
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    (output, out)
  }

  fn db(&self) -> PathBuf {
    self.root.join(".kobo/KoboReader.sqlite")
  }

  /// `ChapterIDBookmarked`, `___PercentRead`, `DateLastRead` of a book.
  fn row(&self, content_id: &str) -> (String, i64, String) {
    Connection::open(self.db())
      .unwrap()
      .query_row(
        "SELECT ChapterIDBookmarked, ___PercentRead, DateLastRead FROM content WHERE ContentID = ?1",
        [content_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
      )
      .unwrap()
  }

  fn backups(&self) -> Vec<PathBuf> {
    match fs::read_dir(&self.backups) {
      Ok(dir) => dir.map(|e| e.unwrap().path()).collect(),
      Err(_) => Vec::new(),
    }
  }

  fn journals(&self) -> Vec<String> {
    fs::read_dir(self.root.join(".kobo"))
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
      .filter(|n| n.starts_with("KoboReader.sqlite-"))
      .collect()
  }
}

impl Drop for Setup {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
    let _ = fs::remove_dir_all(&self.backups);
  }
}

fn hash(file: &str) -> String {
  partial_md5(&fixtures().join("epub").join(file)).unwrap()
}

#[test]
fn pull_write_sets_the_position_after_a_backup() {
  let s = Setup::tolino("write");
  let before = fs::read(s.db()).unwrap();
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, LAST_READ + 60);
  let (output, out) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(output.status.success(), "{out}");
  for expected in [
    &format!("  move to   {AHEAD} (90 %)"),
    &format!("  on device {AHEAD_ON_DEVICE}\n"),
    "Wrote 1 position(s) to the device.",
    "Eject the device before you open the book.",
  ] {
    assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
  }
  // DateLastRead stays: the server is still the newer side.
  assert_eq!(
    s.row(BOOK),
    (AHEAD_ON_DEVICE.into(), 90, "2026-01-03T12:00:00Z".into())
  );
  let backups = s.backups();
  assert_eq!(backups.len(), 1);
  let name = backups[0]
    .file_name()
    .unwrap()
    .to_string_lossy()
    .into_owned();
  assert!(
    name.starts_with("KoboReader-") && name.ends_with("Z.sqlite"),
    "{name}"
  );
  assert!(out.contains(&format!("Backup of the database: {}", backups[0].display())));
  assert_eq!(fs::read(&backups[0]).unwrap(), before);
  assert!(s.journals().is_empty(), "{:?}", s.journals());

  // Now both sides agree: push sends nothing, pull writes nothing.
  let (_, out) = s.run(&["push", "--book", "book-a.epub"]);
  assert!(out.contains("  in sync   "), "{out}");
  assert!(s.server.puts().is_empty());
  let (_, out) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(out.contains("Nothing to write to the device."), "{out}");
  assert_eq!(s.backups().len(), 1);
}

/// Nickel keeps the database in WAL mode: the change must end up in the
/// database file itself, with no `-wal` or `-shm` left, still in WAL mode.
#[test]
fn pull_write_in_wal_mode_leaves_one_file() {
  let s = Setup::tolino("write-wal");
  let mode: String = Connection::open(s.db())
    .unwrap()
    .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
    .unwrap();
  assert_eq!(mode, "wal");
  assert!(s.journals().is_empty(), "{:?}", s.journals());
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, LAST_READ + 60);
  let (output, out) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(output.status.success(), "{out}");
  assert!(s.journals().is_empty(), "{:?}", s.journals());
  let header = fs::read(s.db()).unwrap();
  // File format bytes 18 and 19: 2 = WAL.
  assert_eq!(&header[18..20], &[2, 2]);
  // Read the file alone, as a copy would be: the change is in it.
  let copy = s.root.join("copy.sqlite");
  fs::write(&copy, &header).unwrap();
  let position: String = Connection::open_with_flags(
    format!("file:{}?immutable=1", copy.display()),
    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
  )
  .unwrap()
  .query_row(
    "SELECT ChapterIDBookmarked FROM content WHERE ContentID = ?1",
    [BOOK],
    |r| r.get(0),
  )
  .unwrap();
  assert_eq!(position, AHEAD_ON_DEVICE);
}

#[test]
fn forced_pull_moves_date_last_read_behind_the_server() {
  let s = Setup::tolino("write-force");
  // The device is newer, so only --force takes the server position.
  let server_time = LAST_READ - 3600;
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, server_time);
  let (_, out) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(out.contains("  keep      device is newer"), "{out}");
  assert!(s.backups().is_empty());
  let (output, out) = s.run(&["pull", "--book", "book-a.epub", "--write", "--force"]);
  assert!(output.status.success(), "{out}");
  // One second before the server's time: the server stays newer, so a push
  // never sends the old device position back.
  assert_eq!(
    s.row(BOOK),
    (AHEAD_ON_DEVICE.into(), 90, "2026-01-03T10:59:59Z".into())
  );
  let (_, out) = s.run(&["status", "--book", "book-a.epub"]);
  assert!(out.contains("  sync      in sync"), "{out}");
}

#[test]
fn unanswered_pull_writes_nothing() {
  let s = Setup::tolino("write-ask");
  let before = fs::read(s.db()).unwrap();
  // Newer but behind the device: needs an answer, and there is no terminal.
  s.server.store(
    &hash("book-a.epub"),
    "/body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0",
    0.03,
    LAST_READ + 60,
  );
  let (output, out) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(output.status.success(), "{out}");
  assert!(out.contains("no terminal"), "{out}");
  assert!(out.contains("Nothing to write to the device."), "{out}");
  assert_eq!(fs::read(s.db()).unwrap(), before);
  assert!(s.backups().is_empty());
}

#[test]
fn pull_write_needs_a_mounted_device() {
  let s = Setup::tolino("write-noversion");
  fs::remove_file(s.root.join(".kobo/version")).unwrap();
  let before = fs::read(s.db()).unwrap();
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, LAST_READ + 60);
  let (output, _) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("no readable .kobo/version"), "{stderr}");
  assert_eq!(fs::read(s.db()).unwrap(), before);
  assert!(s.backups().is_empty());
}

#[test]
fn pull_write_refuses_with_a_journal() {
  let s = Setup::tolino("write-journal");
  let journal = s.root.join(".kobo/KoboReader.sqlite-journal");
  fs::write(&journal, b"").unwrap();
  let before = fs::read(s.db()).unwrap();
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, LAST_READ + 60);
  let (output, _) = s.run(&["pull", "--book", "book-a.epub", "--write"]);
  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(
    stderr.contains("KoboReader.sqlite-journal exists"),
    "{stderr}"
  );
  assert_eq!(fs::read(s.db()).unwrap(), before);
  assert!(s.backups().is_empty());
}

#[test]
fn pull_write_leaves_the_kobo_alone() {
  // Back to the Kobo's positions is not translated yet.
  let s = Setup::new(
    "write-kobo",
    "kobo-clarabw-4.46-placeholder-books.sql",
    KOBO_VERSION,
  );
  let before = fs::read(s.db()).unwrap();
  for file in ["book-a.epub", "book-a.kepub.epub"] {
    s.server.store(&hash(file), AHEAD, 0.9, 2_000_000_000);
  }
  let (output, out) = s.run(&["pull", "--write"]);
  assert!(output.status.success(), "{out}");
  assert!(out.contains("  skipped   not translated: "), "{out}");
  assert!(out.contains("Nothing to write to the device."), "{out}");
  assert_eq!(fs::read(s.db()).unwrap(), before);
  assert!(s.backups().is_empty());
}

/// The database as Nickel holds it while it runs: WAL mode, the latest
/// change still in `-wal`, the connection open.
fn nickel_running(s: &Setup, position: &str) -> Connection {
  let conn = Connection::open(s.db()).unwrap();
  let _: String = conn
    .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
    .unwrap();
  conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
  conn
    .execute(
      "UPDATE content SET ChapterIDBookmarked = ?1 WHERE ContentID = ?2",
      [position, BOOK],
    )
    .unwrap();
  conn
}

#[test]
fn live_reads_what_nickel_has_not_checkpointed() {
  let s = Setup::tolino("live-read");
  let _nickel = nickel_running(&s, AHEAD_ON_DEVICE);
  assert!(s.journals().iter().any(|j| j.ends_with("-wal")));
  let args = ["status", "--content-id", BOOK];
  let (_, live) = s.run(&[&args[..], &["--live"]].concat());
  assert!(
    live.contains(&format!("  position  {AHEAD_ON_DEVICE}\n")),
    "{live}"
  );
  // A snapshot reads the database file alone and misses it.
  let (_, snapshot) = s.run(&args);
  assert!(!snapshot.contains(AHEAD_ON_DEVICE), "{snapshot}");
  // Only that one book.
  assert_eq!(live.matches("  file      ").count(), 1, "{live}");
}

#[test]
fn live_pull_emits_the_position_and_never_writes() {
  let s = Setup::tolino("live-pull");
  let nickel = nickel_running(&s, "OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)");
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, LAST_READ + 60);
  let base = ["pull", "--live", "--content-id", BOOK, "--timeout", "5"];
  let (output, out) = s.run(&[&base[..], &["--emit"]].concat());
  assert!(output.status.success(), "{out}");
  assert!(
    out.contains(&format!(
      "\nNICKELINKSYNC_POSITION\t90\t{AHEAD_ON_DEVICE}\n"
    )),
    "{out}"
  );
  // What the database holds, for the log.
  assert!(
    out.contains("  stored    OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)\n"),
    "{out}"
  );
  assert!(
    out.contains("Nothing was written to the device database (Nickel is running)."),
    "{out}"
  );
  // Only with --emit.
  let (_, out) = s.run(&base);
  assert!(!out.contains("NICKELINKSYNC_POSITION"), "{out}");
  let stored: String = nickel
    .query_row(
      "SELECT ChapterIDBookmarked FROM content WHERE ContentID = ?1",
      [BOOK],
      |r| r.get(0),
    )
    .unwrap();
  assert_eq!(stored, "OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)");
}

/// The plugin, just before Nickel opens the book: written into the
/// database Nickel holds open, one backup a day.
#[test]
fn live_write_for_one_book_while_nickel_runs() {
  let s = Setup::tolino("live-write");
  let nickel = nickel_running(&s, "OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)");
  s.server
    .store(&hash("book-a.epub"), AHEAD, 0.9, LAST_READ + 60);
  // Never for all books at once.
  let (output, _) = s.run(&["pull", "--live", "--write"]);
  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("needs --content-id"), "{stderr}");
  assert!(s.backups().is_empty());

  let args = ["pull", "--live", "--content-id", BOOK, "--write", "--emit"];
  let (output, out) = s.run(&args);
  assert!(output.status.success(), "{out}");
  assert!(
    out.contains("Wrote 1 position(s) to the device database while Nickel runs."),
    "{out}"
  );
  assert!(out.contains("NICKELINKSYNC_POSITION\t90\t"), "{out}");
  // Nickel's own connection sees it.
  let (position, percent): (String, i64) = nickel
    .query_row(
      "SELECT ChapterIDBookmarked, ___PercentRead FROM content WHERE ContentID = ?1",
      [BOOK],
      |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap();
  assert_eq!((position.as_str(), percent), (AHEAD_ON_DEVICE, 90));
  // Its WAL files stay, and so does the WAL mode.
  assert!(s.journals().iter().any(|j| j.ends_with("-wal")));
  let backups = s.backups();
  assert_eq!(backups.len(), 1, "{backups:?}");
  let name = backups[0]
    .file_name()
    .unwrap()
    .to_string_lossy()
    .into_owned();
  assert!(
    name.starts_with("KoboReader-20") && name.len() == "KoboReader-20261024.sqlite".len(),
    "{name}"
  );
  // The backup is a consistent database, from before the write.
  let old: String = Connection::open(&backups[0])
    .unwrap()
    .query_row(
      "SELECT ChapterIDBookmarked FROM content WHERE ContentID = ?1",
      [BOOK],
      |r| r.get(0),
    )
    .unwrap();
  assert_eq!(old, "OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)");

  // Again the same day: in sync now, and no second backup.
  s.server.store(
    &hash("book-a.epub"),
    AHEAD.replace("text()[1].0", "text()[1].3").as_str(),
    0.91,
    LAST_READ + 120,
  );
  let (output, out) = s.run(&args);
  assert!(output.status.success(), "{out}");
  assert_eq!(s.backups().len(), 1);
}

#[test]
fn live_pull_tells_the_plugin_about_an_unanswered_question() {
  let s = Setup::tolino("live-unanswered");
  let _nickel = nickel_running(&s, "OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)");
  // Newer, but behind the device: `backwards` asks, and nobody can answer.
  s.server.store(
    &hash("book-a.epub"),
    "/body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0",
    0.03,
    LAST_READ + 60,
  );
  let args = ["pull", "--live", "--content-id", BOOK, "--write", "--emit"];
  let (output, out) = s.run(&args);
  assert!(output.status.success(), "{out}");
  assert!(out.contains("\nNICKELINKSYNC_UNANSWERED\t1\n"), "{out}");
  assert!(!out.contains("NICKELINKSYNC_POSITION"), "{out}");
  assert!(s.backups().is_empty());
}
