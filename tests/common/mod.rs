//! Shared test helpers: a fake mounted device built from the fixtures and a
//! local KOSync mock server, so no test needs a real device or account.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::{json, Value};

pub fn fixtures() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub const KOBO_VERSION: &str =
  "N000000000000,4.9.77,4.46.23836,4.9.77,4.9.77,00000000-0000-0000-0000-000000000391\n";
pub const TOLINO_VERSION: &str =
  "N000000000000,4.9.77,5.18.264769,4.9.77,4.9.77,00000000-0000-0000-0000-000000000693\n";

/// A fake device in a fresh temporary directory: `.kobo/version`,
/// `.kobo/KoboReader.sqlite` from a SQL fixture and the placeholder EPUBs
/// under `Books/`, the layout of a mounted Kobo or Tolino.
pub fn fake_device(name: &str, sql: &str, version: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!("nickelinksync-{}-{name}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(root.join(".kobo")).unwrap();
  fs::create_dir_all(root.join("Books")).unwrap();
  fs::write(root.join(".kobo/version"), version).unwrap();
  let db = Connection::open(root.join(".kobo/KoboReader.sqlite")).unwrap();
  db.execute_batch(&fs::read_to_string(fixtures().join("db").join(sql)).unwrap())
    .unwrap();
  for file in ["book-a.epub", "book-a.kepub.epub", "book-c.epub"] {
    fs::copy(
      fixtures().join("epub").join(file),
      root.join("Books").join(file),
    )
    .unwrap();
  }
  root
}

pub const USER: &str = "alice";
pub const PASSWORD: &str = "secret";
/// MD5 of `PASSWORD`.
pub const KEY: &str = "5ebe2294ecd0e0f08eab7690d2a6ee69";

/// One request as the mock saw it.
#[derive(Debug, Clone)]
pub struct Request {
  pub method: String,
  pub path: String,
  pub headers: HashMap<String, String>,
  pub body: String,
}

/// A KOSync server on 127.0.0.1 with one user (`USER` / `PASSWORD`), kept in
/// memory. Its clock is set by the test.
pub struct MockServer {
  pub url: String,
  pub requests: Arc<Mutex<Vec<Request>>>,
  pub documents: Arc<Mutex<HashMap<String, Value>>>,
  pub now: Arc<AtomicI64>,
}

impl MockServer {
  pub fn start() -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = MockServer {
      url,
      requests: Default::default(),
      documents: Default::default(),
      now: Arc::new(AtomicI64::new(1_800_000_000)),
    };
    let (requests, documents, now) = (
      server.requests.clone(),
      server.documents.clone(),
      server.now.clone(),
    );
    std::thread::spawn(move || {
      for stream in listener.incoming() {
        let Ok(stream) = stream else { break };
        handle(stream, &requests, &documents, &now);
      }
    });
    server
  }

  /// Stores a progress as if another device had pushed it at `timestamp`.
  pub fn store(&self, document: &str, progress: &str, percentage: f64, timestamp: i64) {
    self.documents.lock().unwrap().insert(
      document.to_string(),
      json!({
        "document": document,
        "progress": progress,
        "percentage": percentage,
        "device": "KOReader",
        "device_id": "KOREADER0000000000000000000000",
        "timestamp": timestamp,
      }),
    );
  }

  pub fn document(&self, document: &str) -> Option<Value> {
    self.documents.lock().unwrap().get(document).cloned()
  }

  pub fn puts(&self) -> Vec<Request> {
    self
      .requests
      .lock()
      .unwrap()
      .iter()
      .filter(|r| r.method == "PUT")
      .cloned()
      .collect()
  }

  /// A configuration file for this server in `dir`.
  pub fn config(&self, dir: &Path, password: &str) -> PathBuf {
    let path = dir.join("config.ini");
    fs::write(
      &path,
      format!(
        "server = {}\nuser = {USER}\npassword = {password}\n",
        self.url
      ),
    )
    .unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
  }
}

fn handle(
  stream: TcpStream,
  requests: &Mutex<Vec<Request>>,
  documents: &Mutex<HashMap<String, Value>>,
  now: &AtomicI64,
) {
  let mut reader = BufReader::new(stream.try_clone().unwrap());
  let mut line = String::new();
  if reader.read_line(&mut line).unwrap_or(0) == 0 {
    return;
  }
  let mut parts = line.split_whitespace();
  let method = parts.next().unwrap_or_default().to_string();
  let path = parts.next().unwrap_or_default().to_string();
  let mut headers = HashMap::new();
  loop {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let line = line.trim_end();
    if line.is_empty() {
      break;
    }
    if let Some((name, value)) = line.split_once(':') {
      headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
  }
  let length = headers
    .get("content-length")
    .and_then(|l| l.parse().ok())
    .unwrap_or(0);
  let mut body = vec![0; length];
  reader.read_exact(&mut body).unwrap();
  let request = Request {
    method,
    path,
    headers,
    body: String::from_utf8(body).unwrap(),
  };
  requests.lock().unwrap().push(request.clone());

  let authorized = request.headers.get("x-auth-user").map(String::as_str) == Some(USER)
    && request.headers.get("x-auth-key").map(String::as_str) == Some(KEY);
  let (status, reply) = if !authorized {
    (401, json!({"code": 2001, "message": "Unauthorized"}))
  } else {
    match (request.method.as_str(), request.path.as_str()) {
      ("GET", "/users/auth") => (200, json!({"authorized": "OK"})),
      ("GET", path) if path.starts_with("/syncs/progress/") => {
        let document = &path["/syncs/progress/".len()..];
        let stored = documents.lock().unwrap().get(document).cloned();
        // Like koreader-sync-server: an empty Lua table, encoded as `[]`.
        (200, stored.unwrap_or_else(|| json!([])))
      }
      ("PUT", "/syncs/progress") => {
        let mut update: Value = serde_json::from_str(&request.body).unwrap();
        let timestamp = now.load(Ordering::SeqCst);
        update["timestamp"] = json!(timestamp);
        let document = update["document"].as_str().unwrap().to_string();
        documents.lock().unwrap().insert(document.clone(), update);
        (200, json!({"document": document, "timestamp": timestamp}))
      }
      _ => (404, json!({"code": 2000, "message": "Not found"})),
    }
  };
  let body = reply.to_string();
  let reason = match status {
    200 => "OK",
    401 => "Unauthorized",
    _ => "Not Found",
  };
  let mut stream = stream;
  let _ = write!(
    stream,
    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
    body.len()
  );
}
