//! The local configuration file: KOSync server, user, password and how this
//! device names itself on the server.
//!
//! One `key = value` per line; empty lines and lines starting with `#` or
//! `;` are ignored. Keys: `server`, `user`, `password` or `key` (the MD5 of
//! the password), and optionally `device`, `device_id` and `confirm`
//! (`never`, `backwards` or `always`, default `backwards`). The password is
//! only ever read from this file, never from the command line.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

use crate::kosync;
use crate::sync::Confirm;

#[derive(Debug)]
pub enum Error {
  Io(PathBuf, io::Error),
  /// Line number (1-based) and what is wrong with it.
  Syntax(PathBuf, usize, String),
  Missing(PathBuf, &'static str),
  Invalid(PathBuf, &'static str),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Io(path, e) => write!(f, "{}: {e}", path.display()),
      Error::Syntax(path, line, e) => write!(f, "{}, line {line}: {e}", path.display()),
      Error::Missing(path, key) => write!(f, "{}: `{key}` is not set", path.display()),
      Error::Invalid(path, e) => write!(f, "{}: {e}", path.display()),
    }
  }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
  /// Base URL, e.g. `https://kosync.eu`.
  pub server: String,
  pub user: String,
  /// MD5 of the password, as sent in `x-auth-key`.
  pub key: String,
  /// Device name shown on the server; defaults to the model of the device.
  pub device: Option<String>,
  /// Defaults to a value derived from the device name.
  pub device_id: Option<String>,
  /// When to ask before overwriting a position.
  pub confirm: Confirm,
}

impl Config {
  /// Where the configuration is looked up without `--config`:
  /// `$NICKELINKSYNC_CONFIG`, else `$XDG_CONFIG_HOME/nickelinksync/config.ini`,
  /// else `~/.config/nickelinksync/config.ini`.
  pub fn default_path() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(path) = var("NICKELINKSYNC_CONFIG") {
      return Some(PathBuf::from(path));
    }
    let base = match var("XDG_CONFIG_HOME") {
      Some(dir) => PathBuf::from(dir),
      None => PathBuf::from(var("HOME")?).join(".config"),
    };
    Some(base.join("nickelinksync/config.ini"))
  }

  /// Where `pull --write` keeps database backups:
  /// `$NICKELINKSYNC_BACKUP_DIR`, else `$XDG_DATA_HOME/nickelinksync/backups`,
  /// else `~/.local/share/nickelinksync/backups`.
  pub fn backup_dir() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = var("NICKELINKSYNC_BACKUP_DIR") {
      return Some(PathBuf::from(dir));
    }
    let base = match var("XDG_DATA_HOME") {
      Some(dir) => PathBuf::from(dir),
      None => PathBuf::from(var("HOME")?).join(".local/share"),
    };
    Some(base.join("nickelinksync/backups"))
  }

  pub fn load(path: &Path) -> Result<Self, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    Self::parse(path, &text)
  }

  /// `path` is only used in error messages.
  pub fn parse(path: &Path, text: &str) -> Result<Self, Error> {
    let syntax = |line, e: String| Error::Syntax(path.to_path_buf(), line, e);
    let (mut server, mut user, mut password, mut key, mut device, mut device_id, mut confirm) =
      (None, None, None, None, None, None, None);
    for (n, line) in text.lines().enumerate() {
      let line = line.trim();
      if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        continue;
      }
      let Some((name, value)) = line.split_once('=') else {
        return Err(syntax(n + 1, "expected `key = value`".into()));
      };
      let value = Some(value.trim().to_string()).filter(|v| !v.is_empty());
      let slot = match name.trim() {
        "server" => &mut server,
        "user" => &mut user,
        "password" => &mut password,
        "key" => &mut key,
        "device" => &mut device,
        "device_id" => &mut device_id,
        "confirm" => &mut confirm,
        other => return Err(syntax(n + 1, format!("unknown key `{other}`"))),
      };
      *slot = value;
    }
    let missing = |key| Error::Missing(path.to_path_buf(), key);
    let key = match (password, key) {
      (Some(_), Some(_)) => {
        return Err(Error::Invalid(
          path.to_path_buf(),
          "set either `password` or `key`, not both",
        ))
      }
      (Some(password), None) => kosync::auth_key(&password),
      (None, Some(key)) if is_md5(&key) => key.to_ascii_lowercase(),
      (None, Some(_)) => {
        return Err(Error::Invalid(
          path.to_path_buf(),
          "`key` must be 32 hex digits",
        ))
      }
      (None, None) => return Err(missing("password")),
    };
    let confirm = match confirm {
      None => Confirm::default(),
      Some(value) => value.parse().map_err(|()| {
        Error::Invalid(
          path.to_path_buf(),
          "`confirm` must be `never`, `backwards` or `always`",
        )
      })?,
    };
    Ok(Config {
      server: server.ok_or_else(|| missing("server"))?,
      user: user.ok_or_else(|| missing("user"))?,
      key,
      device,
      device_id,
      confirm,
    })
  }

  /// Device name and ID to report: from the file, else `model` and an ID
  /// derived from the name, so each e-reader keeps its own stable ID.
  pub fn identity(&self, model: &str) -> (String, String) {
    let name = self.device.clone().unwrap_or_else(|| model.to_string());
    let id = self.device_id.clone().unwrap_or_else(|| device_id(&name));
    (name, id)
  }
}

/// 32 uppercase hex digits like KOReader's random device IDs, but derived
/// from the device name.
pub fn device_id(name: &str) -> String {
  Md5::digest(format!("nickelinksync:{name}").as_bytes())
    .iter()
    .map(|b| format!("{b:02X}"))
    .collect()
}

fn is_md5(s: &str) -> bool {
  s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A warning if the file holds a password and others can read it.
pub fn permission_warning(path: &Path) -> Option<String> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    if mode & 0o077 != 0 {
      return Some(format!(
        "{} can be read by other users; run `chmod 600` on it",
        path.display()
      ));
    }
  }
  #[cfg(not(unix))]
  let _ = path;
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse(text: &str) -> Result<Config, Error> {
    Config::parse(Path::new("config.ini"), text)
  }

  #[test]
  fn password_becomes_md5_key() {
    let c = parse(
      "# KOSync\nserver = https://kosync.eu/\nuser = alice\npassword = secret\n\n; comment\n",
    )
    .unwrap();
    assert_eq!(c.server, "https://kosync.eu/");
    assert_eq!(c.user, "alice");
    assert_eq!(c.key, "5ebe2294ecd0e0f08eab7690d2a6ee69");
    assert_eq!(c.device, None);
    assert_eq!(c.confirm, Confirm::Backwards);
  }

  #[test]
  fn confirm_setting() {
    let c = parse("server=s\nuser=u\npassword=p\nconfirm = always").unwrap();
    assert_eq!(c.confirm, Confirm::Always);
    let c = parse("server=s\nuser=u\npassword=p\nconfirm=never").unwrap();
    assert_eq!(c.confirm, Confirm::Never);
    let e = parse("server=s\nuser=u\npassword=p\nconfirm=sometimes")
      .unwrap_err()
      .to_string();
    assert!(e.contains("`confirm` must be"), "{e}");
  }

  #[test]
  fn key_instead_of_password() {
    let c = parse("server=s\nuser=u\nkey=5EBE2294ECD0E0F08EAB7690D2A6EE69").unwrap();
    assert_eq!(c.key, "5ebe2294ecd0e0f08eab7690d2a6ee69");
    assert!(parse("server=s\nuser=u\nkey=nope").is_err());
    assert!(parse("server=s\nuser=u\nkey=5ebe2294ecd0e0f08eab7690d2a6ee69\npassword=x").is_err());
  }

  #[test]
  fn errors_name_the_problem() {
    let e = parse("server = s\nuser = u\n").unwrap_err().to_string();
    assert!(e.contains("`password` is not set"), "{e}");
    let e = parse("server = s\nusr = u\n").unwrap_err().to_string();
    assert!(e.contains("line 2: unknown key `usr`"), "{e}");
    let e = parse("server s\n").unwrap_err().to_string();
    assert!(e.contains("line 1"), "{e}");
  }

  #[test]
  fn identity_defaults_to_the_model() {
    let mut c = parse("server=s\nuser=u\npassword=p").unwrap();
    let (name, id) = c.identity("Tolino Shine Color");
    assert_eq!(name, "Tolino Shine Color");
    assert_eq!(id, device_id("Tolino Shine Color"));
    assert_eq!(id.len(), 32);
    c.device = Some("Shelf".into());
    c.device_id = Some("ABC".into());
    assert_eq!(c.identity("x"), ("Shelf".into(), "ABC".into()));
  }
}
