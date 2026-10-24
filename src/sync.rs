//! The conflict rule between the device and the server: the newest position
//! wins, and, by default, nothing moves backwards in the book without being
//! asked to.
//!
//! The device side is the stored position with `DateLastRead`; the server
//! side is the stored progress with the time the server stored it.
//! `ReadStatus` is not compared: jumping to the last page marks a book
//! finished although the reader is elsewhere.
//!
//! Which side is newer comes from the timestamps; whether a move goes
//! backwards comes from the position in the book (spine index, then
//! percentage), never from the timestamps. Nickel sets `DateLastRead` as
//! soon as a book is opened, so after merely opening a book on the device
//! the device looks newer than a position read further on elsewhere.

use std::cmp::Ordering;
use std::fmt;
use std::io::{self, BufRead, Write};
use std::str::FromStr;

use crate::kosync::Progress;

/// The device position of a book, translated for KOSync.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
  pub xpointer: String,
  /// 0.0 to 1.0, from `___PercentRead`.
  pub percentage: f64,
  /// `DateLastRead`, Unix seconds; `None` if missing or unreadable.
  pub last_read: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Compare {
  /// The server has nothing for this book.
  NotOnServer,
  /// Both hold the same XPointer.
  Same,
  /// The device was read after the server position was stored.
  DeviceNewer,
  /// The server position was stored after the device was last read (or the
  /// device has no date).
  ServerNewer,
}

pub fn compare(local: &Local, server: Option<&Progress>) -> Compare {
  let Some(server) = server else {
    return Compare::NotOnServer;
  };
  if server.progress == local.xpointer {
    return Compare::Same;
  }
  match local.last_read {
    Some(read) if read >= server.timestamp => Compare::DeviceNewer,
    _ => Compare::ServerNewer,
  }
}

/// When to ask before one side overwrites the other (`confirm` in the
/// configuration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Confirm {
  /// Never ask: the newest position wins.
  Never,
  /// Ask when the position that would be overwritten lies further on in the
  /// book than the new one.
  #[default]
  Backwards,
  /// Ask whenever the two positions differ.
  Always,
}

impl FromStr for Confirm {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, ()> {
    match s {
      "never" => Ok(Confirm::Never),
      "backwards" => Ok(Confirm::Backwards),
      "always" => Ok(Confirm::Always),
      _ => Err(()),
    }
  }
}

impl fmt::Display for Confirm {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Confirm::Never => "never",
      Confirm::Backwards => "backwards",
      Confirm::Always => "always",
    })
  }
}

/// Which side a command overwrites: `push` the server, `pull` the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
  Push,
  Pull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
  /// Both hold the same XPointer.
  InSync,
  /// Overwrite the other side.
  Apply,
  /// Leave the other side alone: it is newer.
  Keep,
  /// Ask first. Without an answer nothing is overwritten.
  Ask {
    /// The new position lies before the one it would overwrite.
    backwards: bool,
    /// The new position is also the newer one.
    newer: bool,
  },
}

/// What `direction` should do with a book both sides hold.
pub fn decide(
  direction: Direction,
  local: &Local,
  server: &Progress,
  confirm: Confirm,
) -> Decision {
  let newer = match (compare(local, Some(server)), direction) {
    (Compare::Same, _) => return Decision::InSync,
    (Compare::DeviceNewer, Direction::Push) | (Compare::ServerNewer, Direction::Pull) => true,
    _ => false,
  };
  let device = order_in_book(
    &local.xpointer,
    local.percentage,
    &server.progress,
    server.percentage,
  );
  let backwards = match direction {
    Direction::Push => device == Ordering::Less,
    Direction::Pull => device == Ordering::Greater,
  };
  match confirm {
    Confirm::Always => Decision::Ask { backwards, newer },
    Confirm::Backwards if newer && backwards => Decision::Ask { backwards, newer },
    _ if newer => Decision::Apply,
    _ => Decision::Keep,
  }
}

/// Where position `a` lies in the book relative to `b`: by spine index
/// (`DocFragment[n]`) where both XPointers have one, then by percentage.
pub fn order_in_book(a: &str, a_percentage: f64, b: &str, b_percentage: f64) -> Ordering {
  if let (Some(a), Some(b)) = (doc_fragment(a), doc_fragment(b)) {
    if a != b {
      return a.cmp(&b);
    }
  }
  // Nickel stores whole percents; a hair's difference is no difference.
  if (a_percentage - b_percentage).abs() < 1e-6 {
    Ordering::Equal
  } else if a_percentage < b_percentage {
    Ordering::Less
  } else {
    Ordering::Greater
  }
}

/// `n` of `/body[1]/DocFragment[n]/…`.
fn doc_fragment(xpointer: &str) -> Option<usize> {
  let rest = &xpointer[xpointer.find("DocFragment[")? + "DocFragment[".len()..];
  rest[..rest.find(']')?].parse().ok()
}

/// The answer to "device or server?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
  Device,
  Server,
}

/// Asks on `output` which position should count and reads the answer from
/// `input`: `d` or `device`, `s` or `server`. An empty line or the end of
/// the input is no answer (`None`); anything else is asked again.
pub fn ask(input: &mut impl BufRead, output: &mut impl Write) -> io::Result<Option<Side>> {
  loop {
    write!(
      output,
      "            Keep which position, [d]evice or [s]erver? (Enter skips) "
    )?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
      writeln!(output)?;
      return Ok(None);
    }
    match line.trim().to_ascii_lowercase().as_str() {
      "" => return Ok(None),
      "d" | "device" => return Ok(Some(Side::Device)),
      "s" | "server" => return Ok(Some(Side::Server)),
      _ => {}
    }
  }
}

/// Seconds since 1970 for an ISO 8601 date and time in UTC as Nickel stores
/// it: `2026-01-02T12:00:00Z`, also with fractional seconds, without `Z` or
/// with a space instead of `T`. Other offsets are not accepted.
pub fn parse_timestamp(s: &str) -> Option<i64> {
  let s = s.trim().trim_end_matches('Z');
  let (date, time) = s.split_once(['T', ' '])?;
  let mut date = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
  let (y, m, d) = (date.next()??, date.next()??, date.next()??);
  let time = time.split('.').next()?;
  let mut time = time.splitn(3, ':').map(|p| p.parse::<i64>().ok());
  let (hh, mm, ss) = (time.next()??, time.next()??, time.next()??);
  if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
    return None;
  }
  Some(days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// `2026-01-02T12:00:00Z`, as Nickel stores `DateLastRead`.
pub fn iso_timestamp(t: i64) -> String {
  let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
  let (y, m, d) = civil_from_days(days);
  format!(
    "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
    secs / 3600,
    secs / 60 % 60,
    secs % 60
  )
}

/// `2026-01-02 12:00:00 UTC`.
pub fn format_timestamp(t: i64) -> String {
  let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
  let (y, m, d) = civil_from_days(days);
  format!(
    "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
    secs / 3600,
    secs / 60 % 60,
    secs % 60
  )
}

// Days since 1970-01-01 in the proleptic Gregorian calendar, after Howard
// Hinnant's `days_from_civil` / `civil_from_days`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
  let y = if m <= 2 { y - 1 } else { y };
  let era = y.div_euclid(400);
  let yoe = y - era * 400;
  let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
  let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
  let z = z + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z - era * 146_097;
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  let y = yoe + era * 400 + i64::from(m <= 2);
  (y, m, d)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn timestamps() {
    assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_timestamp("2026-01-02T12:00:00Z"), Some(1_767_355_200));
    assert_eq!(
      parse_timestamp("2026-01-02T12:00:00.123"),
      Some(1_767_355_200)
    );
    assert_eq!(parse_timestamp("2026-01-02 12:00:00"), Some(1_767_355_200));
    assert_eq!(parse_timestamp("2024-02-29T23:59:59Z"), Some(1_709_251_199));
    assert_eq!(parse_timestamp("0000-00-00T00:00:00.000"), None);
    assert_eq!(parse_timestamp(""), None);
    assert_eq!(format_timestamp(1_767_355_200), "2026-01-02 12:00:00 UTC");
    assert_eq!(format_timestamp(1_709_251_199), "2024-02-29 23:59:59 UTC");
    assert_eq!(iso_timestamp(1_767_355_200), "2026-01-02T12:00:00Z");
    assert_eq!(
      parse_timestamp(&iso_timestamp(1_709_251_199)),
      Some(1_709_251_199)
    );
  }

  fn server(progress: &str, timestamp: i64) -> Progress {
    Progress {
      progress: progress.into(),
      percentage: 0.5,
      device: "KOReader".into(),
      device_id: "K".into(),
      timestamp,
    }
  }

  #[test]
  fn newest_wins() {
    let local = Local {
      xpointer: "/body[1]/DocFragment[2]".into(),
      percentage: 0.1,
      last_read: Some(1000),
    };
    assert_eq!(compare(&local, None), Compare::NotOnServer);
    assert_eq!(
      compare(&local, Some(&server("/body[1]/DocFragment[2]", 5000))),
      Compare::Same
    );
    assert_eq!(
      compare(&local, Some(&server("/body[1]/DocFragment[9]", 999))),
      Compare::DeviceNewer
    );
    assert_eq!(
      compare(&local, Some(&server("/body[1]/DocFragment[9]", 1001))),
      Compare::ServerNewer
    );
    let undated = Local {
      last_read: None,
      ..local
    };
    assert_eq!(
      compare(&undated, Some(&server("/body[1]/DocFragment[9]", 1))),
      Compare::ServerNewer
    );
  }

  #[test]
  fn order_by_spine_then_percentage() {
    let x = |n: usize| format!("/body[1]/DocFragment[{n}]/body[1]/p[3]/text()[1].5");
    assert_eq!(order_in_book(&x(2), 0.9, &x(10), 0.1), Ordering::Less);
    assert_eq!(order_in_book(&x(10), 0.1, &x(2), 0.9), Ordering::Greater);
    assert_eq!(order_in_book(&x(5), 0.3, &x(5), 0.4), Ordering::Less);
    assert_eq!(order_in_book(&x(5), 0.4, &x(5), 0.4), Ordering::Equal);
    // Without a DocFragment only the percentage counts.
    assert_eq!(
      order_in_book("#_doc_fragment", 0.5, &x(2), 0.2),
      Ordering::Greater
    );
    assert_eq!(doc_fragment(&x(82)), Some(82));
    assert_eq!(doc_fragment("/body[1]/DocFragment[x]"), None);
  }

  /// The device at chapter 10, 50 %, last read at 1000.
  fn device() -> Local {
    Local {
      xpointer: "/body[1]/DocFragment[10]/body[1]/p[1]/text()[1].0".into(),
      percentage: 0.5,
      last_read: Some(1000),
    }
  }

  fn sp(n: usize, timestamp: i64) -> Progress {
    let mut p = server(
      &format!("/body[1]/DocFragment[{n}]/body[1]/p[1]/text()[1].0"),
      timestamp,
    );
    p.percentage = n as f64 / 20.0;
    p
  }

  #[test]
  fn confirm_never_is_newest_wins() {
    use Decision::*;
    use Direction::*;
    let d = device();
    let c = Confirm::Never;
    // Server older: push sends, also backwards; pull keeps.
    assert_eq!(decide(Push, &d, &sp(5, 999), c), Apply);
    assert_eq!(decide(Push, &d, &sp(15, 999), c), Apply);
    assert_eq!(decide(Pull, &d, &sp(15, 999), c), Keep);
    // Server newer: pull moves, also backwards; push keeps.
    assert_eq!(decide(Pull, &d, &sp(5, 1001), c), Apply);
    assert_eq!(decide(Pull, &d, &sp(15, 1001), c), Apply);
    assert_eq!(decide(Push, &d, &sp(5, 1001), c), Keep);
    assert_eq!(decide(Push, &d, &server(&d.xpointer, 1), c), InSync);
  }

  #[test]
  fn confirm_backwards_asks_before_going_back() {
    use Decision::*;
    use Direction::*;
    let d = device();
    let c = Confirm::default();
    assert_eq!(c, Confirm::Backwards);
    // The device was only opened after KOReader got further: push asks.
    assert_eq!(
      decide(Push, &d, &sp(15, 999), c),
      Ask {
        backwards: true,
        newer: true
      }
    );
    assert_eq!(decide(Push, &d, &sp(5, 999), c), Apply);
    assert_eq!(
      decide(Pull, &d, &sp(5, 1001), c),
      Ask {
        backwards: true,
        newer: true
      }
    );
    assert_eq!(decide(Pull, &d, &sp(15, 1001), c), Apply);
    // The other side is newer: nothing to ask, it is kept.
    assert_eq!(decide(Push, &d, &sp(15, 1001), c), Keep);
    assert_eq!(decide(Pull, &d, &sp(5, 999), c), Keep);
    // Same chapter: the percentage decides.
    let mut same = sp(10, 999);
    same.progress.push('7');
    same.percentage = 0.6;
    assert!(matches!(decide(Push, &d, &same, c), Ask { .. }));
    same.percentage = 0.4;
    assert_eq!(decide(Push, &d, &same, c), Apply);
  }

  #[test]
  fn confirm_always_asks_on_every_difference() {
    use Decision::*;
    use Direction::*;
    let d = device();
    let c = Confirm::Always;
    assert_eq!(
      decide(Push, &d, &sp(5, 999), c),
      Ask {
        backwards: false,
        newer: true
      }
    );
    assert_eq!(
      decide(Push, &d, &sp(15, 1001), c),
      Ask {
        backwards: true,
        newer: false
      }
    );
    assert_eq!(
      decide(Pull, &d, &sp(15, 999), c),
      Ask {
        backwards: false,
        newer: false
      }
    );
    assert_eq!(decide(Pull, &d, &server(&d.xpointer, 1), c), InSync);
  }

  #[test]
  fn confirm_from_config_text() {
    assert_eq!("never".parse(), Ok(Confirm::Never));
    assert_eq!("backwards".parse(), Ok(Confirm::Backwards));
    assert_eq!("always".parse(), Ok(Confirm::Always));
    assert_eq!("Always".parse::<Confirm>(), Err(()));
    assert_eq!(Confirm::Backwards.to_string(), "backwards");
  }

  #[test]
  fn ask_reads_device_or_server() {
    let answer = |text: &str| {
      let mut out = Vec::new();
      let side = ask(&mut text.as_bytes(), &mut out).unwrap();
      (side, String::from_utf8(out).unwrap())
    };
    assert_eq!(answer("d\n").0, Some(Side::Device));
    assert_eq!(answer("Server\n").0, Some(Side::Server));
    assert_eq!(answer("\n").0, None);
    assert_eq!(answer("").0, None);
    let (side, out) = answer("x\ns\n");
    assert_eq!(side, Some(Side::Server));
    assert_eq!(out.matches("[d]evice or [s]erver?").count(), 2);
  }
}
