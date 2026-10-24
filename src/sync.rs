//! The conflict rule between the device and the server: the newest position
//! wins, and nothing moves backwards in time without being asked to.
//!
//! The device side is the stored position with `DateLastRead`; the server
//! side is the stored progress with the time the server stored it.
//! `ReadStatus` and `___PercentRead` are not compared: jumping to the last
//! page marks a book finished at 100 % although the reader is elsewhere.

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
}
