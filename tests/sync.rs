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
    "  sync      server is newer, but behind the device: pull asks first",
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
  // The server is newer but at 3 %: going back from 74 % asks first.
  for expected in [
    "  server    /body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0 (3 %)",
    "  ask       the server position lies before the device's (confirm = backwards)",
    "            would ask first",
  ] {
    assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
  }
  assert!(!out.contains("would move to"), "{out}");
  assert!(out.contains("  server    (nothing stored)"), "{out}");
  assert!(
    out.contains("Dry run: nothing was written to the device"),
    "{out}"
  );
  assert_eq!(fs::read(&db).unwrap(), before);
  assert!(s.server.puts().is_empty());
}

/// A server position further on than the device's (chapter 90 against 82).
const AHEAD: &str = "/body[1]/DocFragment[90]/body[1]/p[1]/text()[1].0";
/// One before it (chapter 5).
const BEHIND: &str = "/body[1]/DocFragment[5]/body[1]/p[1]/text()[1].0";

impl Setup {
  fn confirm(&self, setting: &str) {
    let mut text = fs::read_to_string(&self.config).unwrap();
    text.push_str(&format!("confirm = {setting}\n"));
    fs::write(&self.config, text).unwrap();
  }
}

#[test]
fn push_asks_before_going_backwards() {
  // KOReader got to 90 %, then the book was only opened on the device.
  let s = Setup::new("push-backwards");
  s.server.store(&book_hash(), AHEAD, 0.9, LAST_READ - 60);
  let output = s.run(&["push", "--book", "book-a.epub"]);
  let out = stdout(&output);
  assert!(output.status.success(), "{out}");
  for expected in [
    &format!("  device    {XPOINTER} (74 %), last read 2026-01-03 12:00:00 UTC"),
    "  ask       the device position lies before the server's (confirm = backwards)",
    "  skipped   needs an answer, but there is no terminal; --force overwrites anyway",
    "1 book(s) need an answer",
  ] {
    assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
  }
  assert!(s.server.puts().is_empty());

  let out = stdout(&s.run(&["push", "--book", "book-a.epub", "--dry-run"]));
  assert!(out.contains("            would ask first"), "{out}");
  assert!(s.server.puts().is_empty());

  let out = stdout(&s.run(&["push", "--book", "book-a.epub", "--force"]));
  assert!(out.contains("  sent      "), "{out}");
  assert_eq!(
    s.server.document(&book_hash()).unwrap()["progress"],
    XPOINTER
  );
}

#[test]
fn push_with_confirm_never_is_newest_wins() {
  let s = Setup::new("push-never");
  s.confirm("never");
  s.server.store(&book_hash(), AHEAD, 0.9, LAST_READ - 60);
  let out = stdout(&s.run(&["push", "--book", "book-a.epub"]));
  assert!(out.contains("  sent      "), "{out}");
  assert!(!out.contains("  ask "), "{out}");
}

#[test]
fn push_with_confirm_always_asks_also_forwards() {
  let s = Setup::new("push-always");
  s.confirm("always");
  s.server.store(&book_hash(), BEHIND, 0.03, LAST_READ - 60);
  let out = stdout(&s.run(&["push", "--book", "book-a.epub"]));
  assert!(
    out.contains("  ask       the positions differ (confirm = always)"),
    "{out}"
  );
  assert!(out.contains("no terminal"), "{out}");
  assert!(s.server.puts().is_empty());
  // A newer server position is asked about too, not just skipped.
  s.server.store(&book_hash(), AHEAD, 0.9, LAST_READ + 60);
  let out = stdout(&s.run(&["push", "--book", "book-a.epub"]));
  assert!(
    out.contains("  ask       the device position lies before the server's"),
    "{out}"
  );
  assert!(!out.contains("server is newer"), "{out}");
  assert!(s.server.puts().is_empty());
}

#[test]
fn pull_follows_the_confirm_setting() {
  let s = Setup::new("pull-confirm");
  // Server newer and further on: pull moves without asking.
  s.server.store(&book_hash(), AHEAD, 0.9, LAST_READ + 60);
  let out = stdout(&s.run(&["pull", "--book", "book-a.epub"]));
  assert!(
    out.contains(&format!("  would move to {AHEAD} (90 %)")),
    "{out}"
  );

  // Server newer but behind: asks; with `never` it moves, `--force` too.
  s.server.store(&book_hash(), BEHIND, 0.03, LAST_READ + 60);
  let out = stdout(&s.run(&["pull", "--book", "book-a.epub", "--force"]));
  assert!(
    out.contains(&format!("  would move to {BEHIND} (3 %)")),
    "{out}"
  );
  s.confirm("never");
  let out = stdout(&s.run(&["pull", "--book", "book-a.epub"]));
  assert!(
    out.contains(&format!("  would move to {BEHIND} (3 %)")),
    "{out}"
  );

  // Device newer: kept, unless forced.
  s.server.store(&book_hash(), AHEAD, 0.9, LAST_READ - 60);
  let out = stdout(&s.run(&["pull", "--book", "book-a.epub"]));
  assert!(out.contains("  keep      device is newer than "), "{out}");
  let out = stdout(&s.run(&["pull", "--book", "book-a.epub", "--force"]));
  assert!(out.contains("  would move to "), "{out}");
}

#[test]
fn status_says_who_asks() {
  let s = Setup::new("status-confirm");
  s.server.store(&book_hash(), AHEAD, 0.9, LAST_READ - 60);
  let out = stdout(&s.run(&["status", "--book", "book-a.epub"]));
  assert!(
    out.contains("  sync      device is newer, but behind the server: push asks first"),
    "{out}"
  );
  s.confirm("always");
  s.server.store(&book_hash(), BEHIND, 0.03, LAST_READ - 60);
  let out = stdout(&s.run(&["status", "--book", "book-a.epub"]));
  assert!(
    out.contains("  sync      device is newer: push asks first (confirm = always)"),
    "{out}"
  );
}

#[test]
fn unknown_confirm_value_is_an_error() {
  let s = Setup::new("confirm-bad");
  s.confirm("sometimes");
  let output = s.run(&["push"]);
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("`confirm` must be"));
}
