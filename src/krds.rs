//! The Kindle's per-book state files: KRDS key/value files in the book's
//! `.sdr` folder, `<name><device hash>.yjf` (reading position) and `.yjr`
//! (highlights and settings), as Kindle firmware 5.19 writes them for a
//! Send-to-Kindle KFX.
//!
//! A KRDS file is a magic number followed by typed values: a version
//! (`long`), the number of top-level objects (`int`), then the objects. An
//! object is a name and values up to its end marker; objects nest.
//!
//! Positions are `base64(01, eid u32le, offset u32le):pid`: a block of the
//! KFX (its `eid`), an offset in code points into the block's text, and the
//! linear position `pid` in the book (see [`crate::kfx`]).

use std::fmt;

const MAGIC: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x1A, 0xB1, 0x26];

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
  /// Type 0x00 (bool) and 0x07 (byte).
  Byte(u8),
  Int(i32),
  Long(i64),
  Double(f64),
  /// `None` is a string marked absent.
  String(Option<String>),
  Object(Object),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
  pub name: String,
  pub values: Vec<Value>,
}

impl Object {
  /// Objects with this name, here and in nested objects, in file order.
  pub fn find<'a>(&'a self, name: &str, out: &mut Vec<&'a Object>) {
    for v in &self.values {
      if let Value::Object(o) = v {
        if o.name == name {
          out.push(o);
        }
        o.find(name, out);
      }
    }
  }

  fn strings(&self) -> impl Iterator<Item = &str> {
    self.values.iter().filter_map(|v| match v {
      Value::String(Some(s)) => Some(s.as_str()),
      _ => None,
    })
  }

  fn longs(&self) -> impl Iterator<Item = i64> + '_ {
    self.values.iter().filter_map(|v| match v {
      Value::Long(n) => Some(*n),
      _ => None,
    })
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "KRDS: {}", self.0)
  }
}

impl std::error::Error for Error {}

/// Parses a KRDS file into its top-level objects.
pub fn parse(data: &[u8]) -> Result<Vec<Object>, Error> {
  let rest = data
    .strip_prefix(&MAGIC)
    .ok_or_else(|| Error("not a KRDS file".into()))?;
  let mut reader = Reader { data: rest, pos: 0 };
  let root = reader.values(false)?;
  Ok(
    root
      .into_iter()
      .filter_map(|v| match v {
        Value::Object(o) => Some(o),
        _ => None,
      })
      .collect(),
  )
}

struct Reader<'a> {
  data: &'a [u8],
  pos: usize,
}

impl Reader<'_> {
  fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
    let bytes = self
      .data
      .get(self.pos..self.pos + N)
      .ok_or_else(|| Error(format!("unexpected end at byte {}", self.pos + MAGIC.len())))?;
    self.pos += N;
    Ok(bytes.try_into().unwrap())
  }

  /// A string: flag byte (1 = absent), else u16 BE length and UTF-8.
  fn string(&mut self) -> Result<Option<String>, Error> {
    let [flag] = self.take()?;
    if flag == 1 {
      return Ok(None);
    }
    let len = usize::from(u16::from_be_bytes(self.take()?));
    let bytes = self
      .data
      .get(self.pos..self.pos + len)
      .ok_or_else(|| Error("string runs past the end".into()))?;
    self.pos += len;
    let s = std::str::from_utf8(bytes).map_err(|_| Error("string is not UTF-8".into()))?;
    Ok(Some(s.to_string()))
  }

  /// Values up to the end of the file, or of the current object.
  fn values(&mut self, in_object: bool) -> Result<Vec<Value>, Error> {
    let mut values = Vec::new();
    while self.pos < self.data.len() {
      let at = self.pos + MAGIC.len();
      let [t] = self.take()?;
      values.push(match t {
        0x00 | 0x07 => Value::Byte(self.take::<1>()?[0]),
        0x01 => Value::Int(i32::from_be_bytes(self.take()?)),
        0x02 => Value::Long(i64::from_be_bytes(self.take()?)),
        0x03 => Value::String(self.string()?),
        0x04 => Value::Double(f64::from_be_bytes(self.take()?)),
        0xFE => {
          let name = self
            .string()?
            .ok_or_else(|| Error(format!("object without a name at byte {at}")))?;
          let values = self.values(true)?;
          Value::Object(Object { name, values })
        }
        0xFF if in_object => return Ok(values),
        _ => return Err(Error(format!("unknown type 0x{t:02x} at byte {at}"))),
      });
    }
    if in_object {
      return Err(Error("object not closed".into()));
    }
    Ok(values)
  }
}

/// A position in a KFX book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
  /// The block (`$155` of a storyline entry).
  pub eid: u32,
  /// Code points into the block's text.
  pub offset: u32,
  /// Linear position in the book, from the KFX position maps.
  pub pid: u32,
}

impl Position {
  /// Parses `AQYAAAAqAAAA:102`. `None` for anything else, such as the
  /// older numeric locations.
  pub fn parse(s: &str) -> Option<Self> {
    let (code, pid) = s.split_once(':')?;
    let bytes = base64_decode(code)?;
    let [1, e0, e1, e2, e3, o0, o1, o2, o3] = bytes[..] else {
      return None;
    };
    Some(Position {
      eid: u32::from_le_bytes([e0, e1, e2, e3]),
      offset: u32::from_le_bytes([o0, o1, o2, o3]),
      pid: pid.parse().ok()?,
    })
  }
}

impl fmt::Display for Position {
  /// As the Kindle stores it.
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut bytes = vec![1];
    bytes.extend(self.eid.to_le_bytes());
    bytes.extend(self.offset.to_le_bytes());
    write!(f, "{}:{}", base64_encode(&bytes), self.pid)
  }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_decode(s: &str) -> Option<Vec<u8>> {
  let s = s.trim_end_matches('=');
  let mut out = Vec::new();
  let (mut acc, mut bits) = (0u32, 0);
  for c in s.bytes() {
    let v = BASE64.iter().position(|&b| b == c)? as u32;
    acc = (acc << 6) | v;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((acc >> bits) as u8);
      acc &= (1 << bits) - 1;
    }
  }
  Some(out)
}

fn base64_encode(bytes: &[u8]) -> String {
  let mut out = String::new();
  for chunk in bytes.chunks(3) {
    let n = chunk
      .iter()
      .enumerate()
      .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
    for i in 0..4 {
      if i <= chunk.len() {
        out.push(char::from(BASE64[(n >> (18 - 6 * i)) as usize & 63]));
      } else {
        out.push('=');
      }
    }
  }
  out
}

/// A position with the time the Kindle stored it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamped {
  pub position: Position,
  /// Milliseconds since 1970 (UTC), if stored.
  pub time_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
  pub start: Position,
  /// The last highlighted character.
  pub end: Position,
  /// Milliseconds since 1970 (UTC).
  pub created_ms: Option<i64>,
}

/// What the `.yjf` and `.yjr` of a book say about reading.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReadingState {
  /// `lpr`, last page read: the first character of the page.
  pub last_read: Option<Stamped>,
  /// `fpr`, furthest page read.
  pub furthest: Option<Position>,
  /// `page.history.record`, earlier pages, oldest first.
  pub history: Vec<Stamped>,
  /// `annotation.personal.highlight`, in file order.
  pub highlights: Vec<Highlight>,
}

impl ReadingState {
  /// Collects the state from the objects of one or more KRDS files.
  pub fn from_objects<'a>(objects: impl IntoIterator<Item = &'a Object>) -> Self {
    let mut state = ReadingState::default();
    for o in objects {
      let wrapped = Object {
        name: String::new(),
        values: vec![Value::Object(o.clone())],
      };
      let find = |name| {
        let mut found = Vec::new();
        wrapped.find(name, &mut found);
        found
      };
      for lpr in find("lpr") {
        if let Some(position) = lpr.strings().find_map(Position::parse) {
          state.last_read = Some(Stamped {
            position,
            time_ms: lpr.longs().next(),
          });
        }
      }
      for fpr in find("fpr") {
        state.furthest = fpr.strings().find_map(Position::parse).or(state.furthest);
      }
      for record in find("page.history.record") {
        if let Some(position) = record.strings().find_map(Position::parse) {
          state.history.push(Stamped {
            position,
            time_ms: record.longs().next(),
          });
        }
      }
      for h in find("annotation.personal.highlight") {
        let mut positions = h.strings().filter_map(Position::parse);
        if let (Some(start), Some(end)) = (positions.next(), positions.next()) {
          state.highlights.push(Highlight {
            start,
            end,
            created_ms: h.longs().next(),
          });
        }
      }
    }
    state.history.sort_by_key(|s| s.time_ms);
    state
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn positions_round_trip() {
    let p = Position::parse("AQYAAAAqAAAA:102").unwrap();
    assert_eq!(
      p,
      Position {
        eid: 6,
        offset: 42,
        pid: 102
      }
    );
    assert_eq!(p.to_string(), "AQYAAAAqAAAA:102");
    let r = Position::parse("AQECAAC7AAAA:126677").unwrap();
    assert_eq!((r.eid, r.offset), (513, 187));
    assert_eq!(r.to_string(), "AQECAAC7AAAA:126677");
    assert_eq!(Position::parse("1234"), None);
    assert_eq!(Position::parse("AgYAAAAqAAAA:102"), None);
    assert_eq!(Position::parse("AQYAAAAq:102"), None);
  }

  #[test]
  fn parses_objects() {
    let mut data = MAGIC.to_vec();
    data.extend([0x02, 0, 0, 0, 0, 0, 0, 0, 1]); // long 1
    data.extend([0x01, 0, 0, 0, 1]); // int 1
    data.extend([0xFE, 0x00, 0x00, 0x03]);
    data.extend(b"lpr");
    data.extend([0x07, 0x02]);
    data.extend([0x03, 0x00, 0x00, 0x0F]);
    data.extend(b"AQMAAAAAAAAA:49");
    data.extend([0x02, 0, 0, 0x01, 0x9B, 0x79, 0x6D, 0xF1, 0x58]);
    data.extend([0x03, 0x01]); // absent string
    data.push(0xFF);
    let objects = parse(&data).unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].values[3], Value::String(None));
    let state = ReadingState::from_objects(&objects);
    let lpr = state.last_read.unwrap();
    assert_eq!((lpr.position.eid, lpr.position.pid), (3, 49));
    assert_eq!(lpr.time_ms, Some(1_767_268_807_000));

    data.pop();
    assert!(parse(&data).is_err());
    assert!(parse(b"KRDS").is_err());
  }
}
