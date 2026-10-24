//! The KOSync client against the local mock server.

mod common;

use common::{MockServer, KEY, USER};
use nickelinksync::kosync::{auth_key, Client, Error, Update};

#[test]
fn authorize() {
  let server = MockServer::start();
  Client::new(&server.url, USER, KEY).authorize().unwrap();
  let wrong = Client::new(&server.url, USER, &auth_key("wrong"));
  assert!(matches!(wrong.authorize(), Err(Error::Unauthorized)));
  let request = &server.requests.lock().unwrap()[0];
  assert_eq!(request.path, "/users/auth");
  assert_eq!(request.headers["x-auth-user"], USER);
  assert_eq!(request.headers["x-auth-key"], KEY);
  assert_eq!(
    request.headers["accept"],
    "application/vnd.koreader.v1+json"
  );
}

#[test]
fn update_then_read_back() {
  let server = MockServer::start();
  // A trailing slash on the base URL does not double the slash.
  let client = Client::new(&format!("{}/", server.url), USER, KEY);
  let document = "0123456789abcdef0123456789abcdef";
  assert_eq!(client.progress(document).unwrap(), None);

  let update = Update {
    document: document.into(),
    progress: "/body[1]/DocFragment[3]/body[1]/p[2]/text()[1].5".into(),
    percentage: 0.25,
    device: "Tolino Shine Color".into(),
    device_id: "ABC".into(),
  };
  assert_eq!(client.update(&update).unwrap(), Some(1_800_000_000));
  let put = &server.puts()[0];
  assert_eq!(put.path, "/syncs/progress");
  assert!(put.headers["content-type"].starts_with("application/json"));
  let body: serde_json::Value = serde_json::from_str(&put.body).unwrap();
  assert_eq!(
    body,
    serde_json::json!({
      "document": document,
      "progress": "/body[1]/DocFragment[3]/body[1]/p[2]/text()[1].5",
      "percentage": 0.25,
      "device": "Tolino Shine Color",
      "device_id": "ABC",
    })
  );

  let stored = client.progress(document).unwrap().unwrap();
  assert_eq!(stored.progress, update.progress);
  assert_eq!(stored.percentage, 0.25);
  assert_eq!(stored.device, "Tolino Shine Color");
  assert_eq!(stored.device_id, "ABC");
  assert_eq!(stored.timestamp, 1_800_000_000);
  assert_eq!(
    server.requests.lock().unwrap().last().unwrap().path,
    format!("/syncs/progress/{document}")
  );
}

#[test]
fn unreachable_server() {
  // Nothing listens on port 9 (discard) on 127.0.0.1 in CI or here.
  let client = Client::new("http://127.0.0.1:9", USER, KEY);
  assert!(matches!(client.authorize(), Err(Error::Http(_))));
}
