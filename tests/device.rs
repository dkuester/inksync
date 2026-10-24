//! Reads devices built from the fixtures: a directory with `.kobo/version`,
//! `.kobo/KoboReader.sqlite` (from a SQL fixture) and the EPUB fixtures under
//! `Books/`, the layout of a mounted Kobo or Tolino.

mod common;

use std::fs;
use std::process::Command;

use common::{fake_device, fixtures};
use nickelinksync::db::Device;
use nickelinksync::hash::partial_md5;

/// `partial_md5_checksum` from a KOReader `metadata.epub.lua` fixture.
fn koreader_md5(sdr: &str) -> String {
  let lua = fs::read_to_string(
    fixtures()
      .join("koreader")
      .join(sdr)
      .join("metadata.epub.lua"),
  )
  .unwrap();
  lua
    .lines()
    .find_map(|l| {
      l.trim()
        .strip_prefix("[\"partial_md5_checksum\"] = \"")?
        .strip_suffix("\",")
    })
    .unwrap()
    .to_string()
}

#[test]
fn partial_md5_matches_koreader() {
  for (epub, sdr) in [
    ("book-a.epub", "book-a.sdr"),
    ("book-a.kepub.epub", "book-a.kepub.sdr"),
    ("book-c.epub", "book-c.sdr"),
    ("book-p.epub", "book-p.sdr"),
    ("book-p.kepub.epub", "book-p.kepub.sdr"),
    ("book-q.epub", "book-q.sdr"),
    ("book-q.kepub.epub", "book-q.kepub.sdr"),
  ] {
    let hash = partial_md5(&fixtures().join("epub").join(epub)).unwrap();
    assert_eq!(hash, koreader_md5(sdr), "{epub}");
  }
}

#[test]
fn kobo_device() {
  let root = fake_device(
    "kobo",
    "kobo-clarabw-4.46-placeholder-books.sql",
    common::KOBO_VERSION,
  );
  let device = Device::open(&root).unwrap();
  let fw = device.firmware.as_ref().unwrap();
  assert_eq!(fw.version, "4.46.23836");
  assert_eq!(fw.model_name(), Some("Kobo Clara BW"));
  assert!(!device.db.is_tolino().unwrap());
  assert_eq!(device.db.version().unwrap(), 176);

  let books = device.db.sideloaded_books().unwrap();
  let summary: Vec<_> = books
    .iter()
    .map(|b| {
      (
        b.onboard_path(),
        b.is_kepub(),
        b.position.as_deref(),
        b.percent_read,
      )
    })
    .collect();
  assert_eq!(
    summary,
    [
      (
        "Books/book-a.epub",
        false,
        Some("OEBPS/xhtml/xhtml-0-7.xhtml"),
        5
      ),
      (
        "Books/book-a.kepub.epub",
        true,
        Some("OEBPS/xhtml/xhtml-0-81.xhtml#kobo.1.1"),
        25
      ),
      (
        "Books/book-c.epub",
        false,
        Some("OPS/xhtml/front02.html#point(/1/2/1/1:0)"),
        12
      ),
    ]
  );
  // The file on the device hashes to what KOReader stored for it.
  assert_eq!(
    partial_md5(&device.book_file(&books[1])).unwrap(),
    koreader_md5("book-a.kepub.sdr")
  );
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn tolino_status_output() {
  let root = fake_device(
    "tolino",
    "tolino-shinecolor-5.18-placeholder-books.sql",
    common::TOLINO_VERSION,
  );
  let output = Command::new(env!("CARGO_BIN_EXE_nickelinksync"))
    .args(["status", "--device"])
    .arg(&root)
    // No configuration: the server is not asked.
    .env("NICKELINKSYNC_CONFIG", root.join("no-config.ini"))
    .output()
    .unwrap();
  assert!(output.status.success());
  let stdout = String::from_utf8(output.stdout).unwrap();
  assert!(stdout.starts_with("Tolino Shine Color, firmware 5.18.264769 (database version 220)\n"));
  for expected in [
    "Book A — Author A",
    "  file      Books/book-a.kepub.epub (EPUB)",
    &format!("  hash      {}", koreader_md5("book-a.kepub.sdr")),
    "  position  OPS/bookwire/bookwire_advertisement5.xhtml#point(/1/4/16/1:688)",
    // Same place as KOReader's position in this file (the chapter heading).
    "  koreader  /body[1]/DocFragment[82]/body[1]/div[1]/text()[1].0",
    "  progress  100 %, finished, last read 2026-01-02T12:00:00Z",
  ] {
    assert!(
      stdout.contains(expected),
      "missing {expected:?} in:\n{stdout}"
    );
  }
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn not_a_device() {
  let output = Command::new(env!("CARGO_BIN_EXE_nickelinksync"))
    .args(["status", "--device", env!("CARGO_MANIFEST_DIR")])
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("not a Kobo or Tolino"));
}
