//! Document identity: the KOSync document hash of a book file, computed the
//! same way as KOReader's default "binary" hash so both sides agree on the
//! book.
//!
//! KOReader hashes up to twelve 1 KiB samples of the file with MD5: at offset
//! 0, then at 1 KiB, 4 KiB, 16 KiB, … (each four times the one before, up to
//! 1 GiB), stopping at the first offset past the end of the file. The last
//! sample may be shorter than 1 KiB. Checked against the
//! `partial_md5_checksum` KOReader stored for the EPUB fixtures.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use md5::{Digest, Md5};

const SAMPLE: u64 = 1024;

/// Sample offsets in the order they are hashed.
fn offsets() -> impl Iterator<Item = u64> {
  std::iter::once(0).chain((0..=10).map(|i| SAMPLE << (2 * i)))
}

/// The KOReader partial MD5 of the file at `path`, as 32 lowercase hex digits.
pub fn partial_md5(path: &Path) -> io::Result<String> {
  let mut file = File::open(path)?;
  let len = file.metadata()?.len();
  let mut md5 = Md5::new();
  let mut buf = Vec::with_capacity(SAMPLE as usize);
  for offset in offsets() {
    if offset >= len {
      break;
    }
    file.seek(SeekFrom::Start(offset))?;
    buf.clear();
    (&mut file).take(SAMPLE).read_to_end(&mut buf)?;
    md5.update(&buf);
  }
  Ok(md5.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sample_offsets() {
    let all: Vec<_> = offsets().collect();
    assert_eq!(&all[..4], [0, 1024, 4096, 16384]);
    assert_eq!(all.last(), Some(&(1 << 30)));
    assert_eq!(all.len(), 12);
  }
}
