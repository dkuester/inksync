//! Reads the Kindle fixtures (Send-to-Kindle KFX and the `.sdr` state files
//! of Book P, Q and R) and translates every stored position to a KOReader
//! XPointer, compared with the XPointers in tests/fixtures/README.md
//! (section "Kindle (KFX via Send-to-Kindle)").

use std::fs;
use std::path::{Path, PathBuf};

use nickelinksync::epub::Epub;
use nickelinksync::kfx::Kfx;
use nickelinksync::kindle::{block_to_xpointer, to_xpointer, Error};
use nickelinksync::krds::{self, ReadingState};

fn fixtures() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

struct Book {
  kfx: Kfx,
  epub: Epub,
  state: ReadingState,
}

fn book(letter: char, n: u32) -> Book {
  let name = format!("book-{letter}_{n:032}");
  let dir = fixtures().join("kindle");
  let sdr = dir.join(format!("{name}.sdr"));
  let objects: Vec<_> = ["yjf", "yjr"]
    .iter()
    .flat_map(|ext| {
      let file = sdr.join(format!("{name}{}.{ext}", "0".repeat(32)));
      krds::parse(&fs::read(file).unwrap()).unwrap()
    })
    .collect();
  Book {
    kfx: Kfx::open(&dir.join(format!("{name}.kfx"))).unwrap(),
    epub: Epub::open(&fixtures().join("epub").join(format!("book-{letter}.epub"))).unwrap(),
    state: ReadingState::from_objects(&objects),
  }
}

const PROBE: &str = "/body[1]/DocFragment[2]/body[1]/";

/// Highlight starts by block: (eid, Kindle offset, KOReader XPointer below
/// the probe chapter's body, exact).
fn check_highlights(b: &mut Book, expected: &[(u32, u32, &str, bool)]) {
  assert_eq!(b.state.highlights.len(), expected.len());
  for h in &b.state.highlights {
    let &(_, offset, xpointer, exact) = expected
      .iter()
      .find(|e| e.0 == h.start.eid)
      .unwrap_or_else(|| panic!("unexpected highlight {}", h.start));
    assert_eq!(h.start.offset, offset, "{}", h.start);
    assert_eq!(b.kfx.pid(h.end.eid, h.end.offset), Some(h.end.pid));
    let x = to_xpointer(&b.kfx, &mut b.epub, &h.start).unwrap();
    assert_eq!(x.xpointer, format!("{PROBE}{xpointer}"), "{}", h.start);
    assert_eq!(x.exact, exact, "{}", h.start);
  }
}

#[test]
fn book_p_highlights() {
  let mut b = book('p', 1);
  assert_eq!(b.kfx.meta("title"), Some("book-p"));
  assert_eq!(b.kfx.meta("author"), Some("Author P"));
  assert_eq!(b.kfx.meta("content_id"), Some(&*format!("{:032}", 1)));
  assert_eq!(b.kfx.sections.len(), b.epub.spine.len());
  check_highlights(
    &mut b,
    &[
      (6, 42, "p[1]/text()[1].42", true),
      // No-break spaces are plain spaces in the KFX.
      (7, 18, "p[2]/text()[1].18", true),
      (8, 14, "p[3]/text()[1].14", true),
      (9, 13, "p[4]/text()[1].13", true),
      // Counted from the paragraph start, through the `em`; the Kindle
      // merged the p5w03 and p5w04 highlights.
      (10, 12, "p[5]/em[1]/text()[1].6", true),
      (11, 12, "p[6]/text()[1].12", true),
      // The comment is dropped in the KFX.
      (12, 12, "p[7]/text()[2].7", true),
    ],
  );
  // p5w04, inside the merged highlight: after the `em`.
  let x = block_to_xpointer(&b.kfx, &mut b.epub, 10, 18).unwrap();
  assert_eq!(x.xpointer, format!("{PROBE}p[5]/text()[2].1"));
  assert!(x.exact);
}

#[test]
fn book_q_highlights() {
  let mut b = book('q', 2);
  check_highlights(
    &mut b,
    &[
      (6, 22, "p[1]/text()[1].22", true),
      (7, 16, "p[2]/text()[1].16", true),
      (8, 14, "p[3]/text()[1].14", true),
      // `𝄞` is one code point.
      (9, 8, "p[4]/text()[1].8", true),
      // The CDATA text is lost in the KFX (`q5w01 ]]> q5w04 q5w05.`): found
      // by matching the text after it, but the block texts differ.
      (10, 10, "p[5]/text()[3].1", false),
      // Leading spaces are dropped in the KFX.
      (11, 12, "p[6]/text()[1].13", true),
      (12, 14, "p[7]/text()[1].14", true),
      // A combining accent is one code point.
      (13, 12, "p[8]/text()[1].12", true),
    ],
  );
  // The book was opened at the probe chapter's heading.
  let lpr = b.state.last_read.unwrap().position;
  let x = to_xpointer(&b.kfx, &mut b.epub, &lpr).unwrap();
  assert_eq!(x.xpointer, format!("{PROBE}h1[1]/text()[1].0"));
  assert!(x.exact);
}

#[test]
fn book_r_positions() {
  let mut b = book('r', 3);
  let lpr = b.state.last_read.unwrap();
  assert_eq!(lpr.position.to_string(), "AQECAAC7AAAA:126677");
  assert_eq!(lpr.time_ms, Some(1_767_268_803_000));
  assert_eq!(b.state.furthest, Some(lpr.position));
  assert_eq!(b.state.history.len(), 3);
  assert!(b.state.highlights.is_empty());

  let xpointer = |b: &mut Book, p: &krds::Position| {
    let x = to_xpointer(&b.kfx, &mut b.epub, p).unwrap();
    assert!(x.exact, "{p}");
    x.xpointer
  };
  // "Stuhl Garten Seite", in R3-85.
  assert_eq!(
    xpointer(&mut b, &lpr.position),
    "/body[1]/DocFragment[4]/body[1]/p[85]/text()[1].187"
  );
  // "Häuser Schatten Straße", in R2-99.
  let r2 = b.state.history[2].position;
  assert_eq!((r2.eid, r2.offset, r2.pid), (361, 192, 78026));
  assert_eq!(
    xpointer(&mut b, &r2),
    "/body[1]/DocFragment[3]/body[1]/p[99]/text()[1].192"
  );
  // The earlier pages resolve too, with the stored pid.
  for p in b.state.history.clone() {
    xpointer(&mut b, &p.position);
  }
}

#[test]
fn every_stored_pid_matches_the_position_maps() {
  for (letter, n) in [('p', 1), ('q', 2), ('r', 3)] {
    let b = book(letter, n);
    let s = &b.state;
    let positions = s
      .last_read
      .map(|l| l.position)
      .into_iter()
      .chain(s.furthest)
      .chain(s.history.iter().map(|h| h.position))
      .chain(s.highlights.iter().flat_map(|h| [h.start, h.end]));
    for p in positions {
      assert_eq!(
        b.kfx.pid(p.eid, p.offset),
        Some(p.pid),
        "book {letter}: {p}"
      );
    }
    // The sections' pid ranges follow each other.
    let mut next = 0;
    for section in &b.kfx.sections {
      assert_eq!(section.pid_start, next);
      assert_eq!(section.runs.last().map(|r| r.0), Some(section.pid_len));
      next += section.pid_len;
    }
  }
}

#[test]
fn rejects_a_stale_position() {
  let mut b = book('p', 1);
  let mut p = b.state.highlights[0].start;
  p.pid += 1;
  assert!(matches!(
    to_xpointer(&b.kfx, &mut b.epub, &p),
    Err(Error::StalePosition { .. })
  ));
  assert!(matches!(
    block_to_xpointer(&b.kfx, &mut b.epub, 9999, 0),
    Err(Error::UnknownBlock(9999))
  ));
}
