//! Client for a KOSync server (koreader-sync-server protocol).
//!
//! Authenticates the user and reads and writes the progress of a document.
//! Re-implemented from the documented protocol:
//!
//! - every request carries `x-auth-user` (user name) and `x-auth-key` (MD5 of
//!   the password, 32 lowercase hex digits) and accepts
//!   `application/vnd.koreader.v1+json`;
//! - `GET /users/auth` answers 200 if the user and key are valid, 401 if not;
//! - `GET /syncs/progress/:document` answers the stored progress, or `{}` if
//!   there is none;
//! - `PUT /syncs/progress` with `{document, progress, percentage, device,
//!   device_id}` stores it; the server sets the timestamp (Unix seconds).
//!
//! HTTPS goes through `rustls` with the Mozilla root certificates compiled
//! in, so the static device build needs neither OpenSSL nor a certificate
//! store on the device.

use std::fmt;
use std::time::Duration;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ureq::Agent;

const ACCEPT: &str = "application/vnd.koreader.v1+json";

#[derive(Debug)]
pub enum Error {
  /// No connection, TLS failure, timeout, …
  Http(ureq::Error),
  /// 401: unknown user or wrong password.
  Unauthorized,
  /// Any other status that is not 2xx, with the response body.
  Status(u16, String),
  /// The response is not the JSON the protocol describes.
  Response(String),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Http(e) => write!(f, "{e}"),
      Error::Unauthorized => write!(f, "unauthorized: check user and password"),
      Error::Status(code, body) => write!(f, "HTTP {code}: {}", body.trim()),
      Error::Response(e) => write!(f, "unexpected response: {e}"),
    }
  }
}

impl std::error::Error for Error {}

impl From<ureq::Error> for Error {
  fn from(e: ureq::Error) -> Self {
    Error::Http(e)
  }
}

/// What is sent with `PUT /syncs/progress`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Update {
  /// KOReader partial MD5 of the book file.
  pub document: String,
  /// KOReader XPointer.
  pub progress: String,
  /// 0.0 to 1.0.
  pub percentage: f64,
  pub device: String,
  pub device_id: String,
}

/// The progress stored on the server for a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
  pub progress: String,
  pub percentage: f64,
  pub device: String,
  pub device_id: String,
  /// When the server stored it, Unix seconds.
  pub timestamp: i64,
}

/// The MD5 key KOSync expects in `x-auth-key` for a password.
pub fn auth_key(password: &str) -> String {
  Md5::digest(password.as_bytes())
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect()
}

pub struct Client {
  agent: Agent,
  base: String,
  user: String,
  key: String,
}

impl Client {
  /// `server` is the base URL, e.g. `https://kosync.eu`; `key` is the MD5 of
  /// the password (see [`auth_key`]).
  pub fn new(server: &str, user: &str, key: &str) -> Self {
    let agent = Agent::config_builder()
      .http_status_as_error(false)
      .timeout_global(Some(Duration::from_secs(30)))
      .user_agent(concat!("nickelinksync/", env!("CARGO_PKG_VERSION")))
      .build()
      .into();
    Client {
      agent,
      base: server.trim_end_matches('/').to_string(),
      user: user.to_string(),
      key: key.to_string(),
    }
  }

  /// Checks user and key (`GET /users/auth`).
  pub fn authorize(&self) -> Result<(), Error> {
    let response = self
      .agent
      .get(format!("{}/users/auth", self.base))
      .header("x-auth-user", &self.user)
      .header("x-auth-key", &self.key)
      .header("accept", ACCEPT)
      .call()?;
    body(response).map(|_| ())
  }

  /// The stored progress of `document`, `None` if the server has none.
  pub fn progress(&self, document: &str) -> Result<Option<Progress>, Error> {
    let response = self
      .agent
      .get(format!("{}/syncs/progress/{document}", self.base))
      .header("x-auth-user", &self.user)
      .header("x-auth-key", &self.key)
      .header("accept", ACCEPT)
      .call()?;
    parse_progress(&body(response)?)
  }

  /// Stores `update` and returns the server's timestamp for it, if it sent
  /// one.
  pub fn update(&self, update: &Update) -> Result<Option<i64>, Error> {
    let response = self
      .agent
      .put(format!("{}/syncs/progress", self.base))
      .header("x-auth-user", &self.user)
      .header("x-auth-key", &self.key)
      .header("accept", ACCEPT)
      .send_json(update)?;
    Ok(body(response)?.get("timestamp").and_then(number))
  }
}

/// The JSON body of a 2xx response.
fn body(mut response: ureq::http::Response<ureq::Body>) -> Result<Value, Error> {
  let status = response.status().as_u16();
  let text = response.body_mut().read_to_string()?;
  match status {
    200..=299 if text.trim().is_empty() => Ok(Value::Object(Default::default())),
    200..=299 => serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string())),
    401 => Err(Error::Unauthorized),
    _ => Err(Error::Status(status, text)),
  }
}

/// Numbers may come as JSON numbers or, from some servers, as strings.
fn number<T: std::str::FromStr>(value: &Value) -> Option<T> {
  match value {
    Value::Number(n) => n.to_string().parse().ok(),
    Value::String(s) => s.trim().parse().ok(),
    _ => None,
  }
}

#[derive(Deserialize)]
struct Raw {
  progress: Option<String>,
  percentage: Option<Value>,
  device: Option<String>,
  device_id: Option<String>,
  timestamp: Option<Value>,
}

fn parse_progress(value: &Value) -> Result<Option<Progress>, Error> {
  let raw = Raw::deserialize(value).map_err(|e| Error::Response(e.to_string()))?;
  let Some(progress) = raw.progress.filter(|p| !p.is_empty()) else {
    return Ok(None);
  };
  let timestamp = raw
    .timestamp
    .as_ref()
    .and_then(number::<f64>)
    .ok_or_else(|| Error::Response("progress without timestamp".into()))?;
  Ok(Some(Progress {
    progress,
    percentage: raw.percentage.as_ref().and_then(number).unwrap_or(0.0),
    device: raw.device.unwrap_or_default(),
    device_id: raw.device_id.unwrap_or_default(),
    timestamp: timestamp as i64,
  }))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_is_md5_of_password() {
    assert_eq!(auth_key("secret"), "5ebe2294ecd0e0f08eab7690d2a6ee69");
  }

  #[test]
  fn empty_progress_is_none() {
    assert_eq!(parse_progress(&serde_json::json!({})).unwrap(), None);
  }

  #[test]
  fn progress_with_numbers_as_strings() {
    let p = parse_progress(&serde_json::json!({
      "document": "abc",
      "progress": "/body[1]/DocFragment[3]/body[1]/p[1]/text()[1].0",
      "percentage": "0.25",
      "device": "KOReader",
      "device_id": "X",
      "timestamp": 1_700_000_000
    }))
    .unwrap()
    .unwrap();
    assert_eq!(p.percentage, 0.25);
    assert_eq!(p.timestamp, 1_700_000_000);
    assert_eq!(p.device, "KOReader");
  }
}
