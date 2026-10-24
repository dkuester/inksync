//! A small reader for Amazon Ion binary data, as KFX books store it.
//!
//! Written from the Ion 1.0 binary specification. Symbols stay numeric: the
//! KFX structure only needs the IDs of the shared `YJ_symbols` table and of
//! the book's local symbols, never their text. Decimals and timestamps are
//! kept as raw bytes.

use std::fmt;

/// The Ion binary version marker that starts a stream.
const BVM: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];

/// Symbol ID of `$ion_symbol_table`, the annotation of a local symbol table.
const ION_SYMBOL_TABLE: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
  /// A typed or untyped null.
  Null,
  Bool(bool),
  Int(i64),
  Float(f64),
  Symbol(u32),
  String(String),
  /// Blob or clob.
  Bytes(Vec<u8>),
  List(Vec<Value>),
  /// Fields as (field symbol, value), in stored order.
  Struct(Vec<(u32, Value)>),
  Annotated(Vec<u32>, Box<Value>),
  /// Decimal or timestamp, undecoded.
  Raw(u8, Vec<u8>),
}

impl Value {
  /// The value below any annotations.
  pub fn plain(&self) -> &Value {
    match self {
      Value::Annotated(_, v) => v.plain(),
      v => v,
    }
  }

  /// The first field `field` of a struct.
  pub fn get(&self, field: u32) -> Option<&Value> {
    match self.plain() {
      Value::Struct(fields) => fields.iter().find(|(k, _)| *k == field).map(|(_, v)| v),
      _ => None,
    }
  }

  pub fn as_int(&self) -> Option<i64> {
    match self.plain() {
      Value::Int(n) => Some(*n),
      _ => None,
    }
  }

  pub fn as_symbol(&self) -> Option<u32> {
    match self.plain() {
      Value::Symbol(s) => Some(*s),
      _ => None,
    }
  }

  pub fn as_str(&self) -> Option<&str> {
    match self.plain() {
      Value::String(s) => Some(s),
      _ => None,
    }
  }

  /// Elements of a list or S-expression.
  pub fn as_list(&self) -> Option<&[Value]> {
    match self.plain() {
      Value::List(items) => Some(items),
      _ => None,
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "Ion: {}", self.0)
  }
}

impl std::error::Error for Error {}

/// Reads all top-level values of a stream. Version markers and local symbol
/// tables are skipped.
pub fn read_stream(data: &[u8]) -> Result<Vec<Value>, Error> {
  let mut reader = Reader { data, pos: 0 };
  let mut values = Vec::new();
  while reader.pos < data.len() {
    if data[reader.pos..].starts_with(&BVM) {
      reader.pos += BVM.len();
      continue;
    }
    match reader.value()? {
      Some(Value::Annotated(anns, _)) if anns.first() == Some(&ION_SYMBOL_TABLE) => {}
      Some(v) => values.push(v),
      None => {}
    }
  }
  Ok(values)
}

struct Reader<'a> {
  data: &'a [u8],
  pos: usize,
}

impl<'a> Reader<'a> {
  fn error(&self, what: &str) -> Error {
    Error(format!("{what} at byte {}", self.pos))
  }

  fn byte(&mut self) -> Result<u8, Error> {
    let b = *self
      .data
      .get(self.pos)
      .ok_or_else(|| self.error("unexpected end"))?;
    self.pos += 1;
    Ok(b)
  }

  /// Big-endian base-128 number; the last byte has the high bit set.
  fn var_uint(&mut self) -> Result<usize, Error> {
    let mut v: usize = 0;
    loop {
      let b = self.byte()?;
      v = v
        .checked_mul(128)
        .ok_or_else(|| self.error("VarUInt too large"))?
        | usize::from(b & 0x7F);
      if b & 0x80 != 0 {
        return Ok(v);
      }
    }
  }

  fn bytes(&mut self, len: usize) -> Result<&'a [u8], Error> {
    let end = self
      .pos
      .checked_add(len)
      .filter(|&e| e <= self.data.len())
      .ok_or_else(|| self.error("value runs past the end"))?;
    let data: &'a [u8] = self.data;
    let slice = &data[self.pos..end];
    self.pos = end;
    Ok(slice)
  }

  /// One value; `None` for NOP padding.
  fn value(&mut self) -> Result<Option<Value>, Error> {
    let td = self.byte()?;
    let (t, l) = (td >> 4, td & 0x0F);
    if l == 15 {
      return Ok(Some(Value::Null));
    }
    if t == 1 {
      return match l {
        0 | 1 => Ok(Some(Value::Bool(l == 1))),
        _ => Err(self.error("bad bool")),
      };
    }
    // A struct with length 1 is sorted and has its length in a VarUInt.
    let len = if l == 14 || (t == 0xD && l == 1) {
      self.var_uint()?
    } else {
      usize::from(l)
    };
    let start = self.pos;
    let body = self.bytes(len)?;
    let value = match t {
      0 => return Ok(None),
      2 | 3 => {
        if body.len() > 8 {
          return Err(self.error("integer too large"));
        }
        let magnitude = body.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b));
        let n = i64::try_from(magnitude).map_err(|_| self.error("integer too large"))?;
        Value::Int(if t == 3 { -n } else { n })
      }
      4 => match body.len() {
        0 => Value::Float(0.0),
        4 => Value::Float(f64::from(f32::from_be_bytes(body.try_into().unwrap()))),
        8 => Value::Float(f64::from_be_bytes(body.try_into().unwrap())),
        _ => return Err(self.error("bad float")),
      },
      5 | 6 => Value::Raw(t, body.to_vec()),
      7 => {
        if body.len() > 4 {
          return Err(self.error("symbol ID too large"));
        }
        Value::Symbol(body.iter().fold(0u32, |v, &b| (v << 8) | u32::from(b)))
      }
      8 => Value::String(
        std::str::from_utf8(body)
          .map_err(|_| self.error("string is not UTF-8"))?
          .to_string(),
      ),
      9 | 0xA => Value::Bytes(body.to_vec()),
      0xB | 0xC => {
        let mut inner = Reader {
          data: &self.data[..start + len],
          pos: start,
        };
        let mut items = Vec::new();
        while inner.pos < start + len {
          items.extend(inner.value()?);
        }
        Value::List(items)
      }
      0xD => {
        let mut inner = Reader {
          data: &self.data[..start + len],
          pos: start,
        };
        let mut fields = Vec::new();
        while inner.pos < start + len {
          let field =
            u32::try_from(inner.var_uint()?).map_err(|_| inner.error("field ID too large"))?;
          if let Some(v) = inner.value()? {
            fields.push((field, v));
          }
        }
        Value::Struct(fields)
      }
      0xE => {
        let mut inner = Reader {
          data: &self.data[..start + len],
          pos: start,
        };
        let anns_len = inner.var_uint()?;
        let anns_end = inner.pos + anns_len;
        let mut anns = Vec::new();
        while inner.pos < anns_end {
          anns.push(
            u32::try_from(inner.var_uint()?).map_err(|_| inner.error("annotation too large"))?,
          );
        }
        let v = inner
          .value()?
          .ok_or_else(|| inner.error("annotation on padding"))?;
        Value::Annotated(anns, Box::new(v))
      }
      _ => {
        return Err(Error(format!(
          "reserved type 0x{td:02x} at byte {}",
          start - 1
        )))
      }
    };
    Ok(Some(value))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_values() {
    // $ion_1_0, then {4: 'sym 874', 146: ["ab", -2, 1.0e0, true, null]}, with
    // one byte of NOP padding between the fields.
    let list = [
      &[0x82, b'a', b'b'][..],
      &[0x31, 0x02],
      &[0x48, 0x3F, 0xF0, 0, 0, 0, 0, 0, 0],
      &[0x11],
      &[0x0F],
    ]
    .concat();
    let mut fields = vec![0x84, 0x72, 0x03, 0x6A];
    fields.extend([0x81, 0x01, 0x00]); // field 1: one byte of NOP padding
                                       // field 146 (VarUInt 0x01 0x92): a list with its length in a VarUInt
    fields.extend([0x01, 0x92, 0xBE, 0x80 | list.len() as u8]);
    fields.extend(&list);
    let mut stream = BVM.to_vec();
    stream.extend([0xDE, 0x80 | fields.len() as u8]);
    stream.extend(&fields);
    let values = read_stream(&stream).unwrap();
    assert_eq!(
      values,
      [Value::Struct(vec![
        (4, Value::Symbol(874)),
        (
          146,
          Value::List(vec![
            Value::String("ab".into()),
            Value::Int(-2),
            Value::Float(1.0),
            Value::Bool(true),
            Value::Null,
          ])
        ),
      ])]
    );
    assert_eq!(values[0].get(4).and_then(Value::as_symbol), Some(874));
  }

  #[test]
  fn skips_symbol_tables_and_unwraps_annotations() {
    // $ion_symbol_table::{} then $419::7
    let stream = [0xE3, 0x81, 0x83, 0xD0, 0xE5, 0x82, 0x03, 0xA3, 0x21, 0x07];
    let values = read_stream(&stream).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].as_int(), Some(7));
  }

  #[test]
  fn rejects_truncated_data() {
    assert!(read_stream(&[0x84, b'a']).is_err());
  }
}
