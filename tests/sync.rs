//! `status`, `push` and `pull` on a fake Tolino against the local mock
//! server.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::Ordering;

use common::{fake_device, fixtures, MockServer, KEY, TOLINO_VERSION};
use nickelinksync::config::device_id;
use nickelinksync::hash::partial_md5;
use serde_json::Value;

/// Where the Tolino fixture's book-a.epub is, as KOReader's XPointer.
const XPOINTER: &str = "/body[1]/DocFragment[82]/body[1]/div[1]/text()[1].0";
/// `DateLastRead` of that book, 2026-01-03T12:00:00Z.
const LAST_READ: i64 = 1_767_441_600;

fn book_hash() -> String {
  partial_md5(&fixtures().join("epub/book-a.epub")).unwrap()
}

struct Setup {
  root: PathBuf,
  server: MockServer,
  config: PathBuf,
}

impl Setup {
  fn new(name: &str) -> Self {
    let root = fake_device(
      name,
      "tolino-shinecolor-5.18-placeholder-books.sql",
      TOLINO_VERSION,
    );
    let server = MockServer::start();
    let config = server.config(&root, common::PASSWORD);
    Setup {
      root,
      server,
      config,
    }
  }

  fn run(&self, args: &[&str]) -> Output {
    run(&self.root, &self.config, args)
  }
}

impl Drop for Setup {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
  }
}

fn run(root: &Path, config: &Path, args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_nickelinksync"))
    .args(args)
    .arg("--device")
    .arg(root)
    .arg("--config")
    .arg(config)
    .output()
    .unwrap()
}

fn stdout(output: &Output) -> String {
  String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn push_sends_each_book_once() {
  let s = Setup::new("push");
  let output = s.run(&["push"]);
  let out = stdout(&output);
  assert!(output.status.success(), "{out}");
  assert!(out.contains("device \"Tolino Shine Color\""), "{out}");
  assert!(
    out.contains(&format!(
      "  sent      {XPOINTER} (74 %) at 2027-01-15 08:00:00 UTC"
    )),
    "{out}"
  );
  // All three placeholder books translate.
  assert_eq!(s.server.puts().len(), 3);
  let put = s
    .server
    .puts()
    .into_iter()
    .find(|r| r.body.contains(&book_hash()))
    .unwrap();
  assert_eq!(put.headers["x-auth-key"], KEY);
  let body: Value = serde_json::from_str(&put.body).unwrap();
  assert_eq!(body["document"], book_hash());
  assert_eq!(body["progress"], XPOINTER);
  assert_eq!(body["percentage"], 0.74);
  assert_eq!(body["device"], "Tolino Shine Color");
  assert_eq!(body["device_id"], device_id("Tolino Shine Color"));

  // Nothing changed since: nothing is sent again.
  let out = stdout(&s.run(&["push"]));
  assert_eq!(s.server.puts().len(), 3, "{out}");
  assert!(out.contains("  in sync   "), "{out}");
}

#[test]
fn push_never_overwrites_a_newer_server_position() {
  let s = Setup::new("push-newer");
  let other = "/body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0";
  s.server.store(&book_hash(), other, 0.03, LAST_READ + 1);
  let out = stdout(&s.run(&["push", "--book", "book-a.epub"]));
  assert!(out.contains("  skipped   server is newer: "), "{out}");
  assert!(out.contains("--force"), "{out}");
  assert!(s.server.puts().is_empty());
  assert_eq!(s.server.document(&book_hash()).unwrap()["progress"], other);

  let out = stdout(&s.run(&["push", "--book", "book-a.epub", "--force"]));
  assert!(out.contains("  sent      "), "{out}");
  assert_eq!(
    s.server.document(&book_hash()).unwrap()["progress"],
    XPOINTER
  );
}

#[test]
fn push_replaces_an_older_server_position() {
  let s = Setup::new("push-older");
  s.server
    .store(&book_hash(), "/body[1]/DocFragment[5]", 0.03, LAST_READ - 1);
  let out = stdout(&s.run(&["push", "--book", "book-a.epub"]));
  assert!(out.contains("  sent      "), "{out}");
  assert_eq!(s.server.puts().len(), 1);
}

#[test]
fn push_dry_run_sends_nothing() {
  let s = Setup::new("push-dry");
  let out = stdout(&s.run(&["push", "--dry-run"]));
  assert!(
    out.contains(&format!("  would send {XPOINTER} (74 %)")),
    "{out}"
  );
  assert!(out.contains("Dry run: nothing was sent."), "{out}");
  assert!(s.server.puts().is_empty());
}

#[test]
fn wrong_password_fails_before_anything_is_sent() {
  let s = Setup::new("push-auth");
  let config = s.server.config(&s.root, "wrong");
  let output = run(&s.root, &config, &["push"]);
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("unauthorized"));
  assert!(s.server.puts().is_empty());
}

#[test]
fn push_needs_a_configuration() {
  let s = Setup::new("push-noconfig");
  let output = run(&s.root, &s.root.join("missing.ini"), &["push"]);
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("missing.ini"));
}

#[test]
fn status_shows_the_server_position() {
  let s = Setup::new("status");
  s.server.now.store(LAST_READ + 3600, Ordering::SeqCst);
  s.server.store(
    &book_hash(),
    "/body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0",
    0.03,
    LAST_READ + 60,
  );
  let output = s.run(&["status", "--book", "book-a.epub"]);
  let out = stdout(&output);
  assert!(output.status.success(), "{out}");
  assert!(
    out.contains(&format!("Server {} as alice", s.server.url)),
    "{out}"
  );
  for expected in [
    &format!("  koreader  {XPOINTER}"),
    "  server    /body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0 (3 %), from KOReader at 2026-01-03 12:01:00 UTC",
    "  sync      server is newer: pull would move the device",
  ] {
    assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
  }
  // Only the one book was asked for.
  assert_eq!(s.server.requests.lock().unwrap().len(), 1);
}

#[test]
fn pull_is_a_dry_run() {
  let s = Setup::new("pull");
  let db = s.root.join(".kobo/KoboReader.sqlite");
  let before = fs::read(&db).unwrap();
  s.server.store(
    &book_hash(),
    "/body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0",
    0.03,
    LAST_READ + 60,
  );
  let output = s.run(&["pull"]);
  let out = stdout(&output);
  assert!(output.status.success(), "{out}");
  assert!(
    out.contains("  would move to /body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0 (3 %)"),
    "{out}"
  );
  assert!(
    out.contains("backwards from 74 %: would ask first"),
    "{out}"
  );
  assert!(out.contains("  server    (nothing stored)"), "{out}");
  assert!(
    out.contains("Dry run: nothing was written to the device"),
    "{out}"
  );
  assert_eq!(fs::read(&db).unwrap(), before);
  assert!(s.server.puts().is_empty());
}
