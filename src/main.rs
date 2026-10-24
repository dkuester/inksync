use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use nickelinksync::config::{self, Config};
use nickelinksync::db::{Book, Device};
use nickelinksync::epub::Epub;
use nickelinksync::hash::partial_md5;
use nickelinksync::kosync::{self, Client, Progress, Update};
use nickelinksync::position::{to_xpointer, PointFlavor, Position, XPointer};
use nickelinksync::sync::{compare, format_timestamp, parse_timestamp, Compare, Local};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Sync the reading progress of sideloaded ebooks between Kobo, Tolino and
/// KOReader through a KOSync server.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Show the position of each sideloaded book on the device and the server
  Status(Target),
  /// Send the device position to the KOSync server
  Push {
    #[command(flatten)]
    target: Target,
    /// Also send positions that are older than the server's
    #[arg(long)]
    force: bool,
    /// Only show what would be sent
    #[arg(long)]
    dry_run: bool,
  },
  /// Show what a newer position on the KOSync server would change (dry run)
  Pull(Target),
}

#[derive(Args)]
struct Target {
  /// Mount point of the device, e.g. /Volumes/KOBOeReader
  #[arg(long)]
  device: PathBuf,
  /// Configuration file [default: ~/.config/nickelinksync/config.ini]
  #[arg(long)]
  config: Option<PathBuf>,
  /// Only books whose title or file path contains this text
  #[arg(long)]
  book: Option<String>,
}

fn main() -> ExitCode {
  let cli = Cli::parse();
  let result = match cli.command {
    Command::Status(target) => status(&target),
    Command::Push {
      target,
      force,
      dry_run,
    } => push(&target, force, dry_run),
    Command::Pull(target) => pull(&target),
  };
  match result {
    Ok(true) => ExitCode::SUCCESS,
    Ok(false) => ExitCode::FAILURE,
    Err(e) => {
      eprintln!("nickelinksync: {e}");
      ExitCode::FAILURE
    }
  }
}

/// The opened device and its sideloaded books, with the header printed.
struct Session {
  device: Device,
  /// Model name, or "Kobo" / "Tolino".
  model: String,
  flavor: PointFlavor,
  books: Vec<Book>,
}

impl Session {
  fn open(target: &Target) -> Result<Self> {
    let device = Device::open(&target.device)?;
    let family = if device.db.is_tolino()? {
      "Tolino"
    } else {
      "Kobo"
    };
    let model = device
      .firmware
      .as_ref()
      .and_then(|fw| fw.model_name())
      .unwrap_or(family)
      .to_string();
    match &device.firmware {
      Some(fw) => println!(
        "{model}, firmware {} (database version {})",
        fw.version,
        device.db.version()?
      ),
      None => println!("{model}, database version {}", device.db.version()?),
    }
    // How Nickel counts nodes in an Adobe point depends on the firmware line.
    let flavor = match &device.firmware {
      Some(fw) => PointFlavor::for_firmware(&fw.version),
      None if family == "Tolino" => PointFlavor::AllNodes,
      None => PointFlavor::JoinText,
    };
    let mut books = device.db.sideloaded_books()?;
    if let Some(filter) = &target.book {
      let filter = filter.to_lowercase();
      books.retain(|b| {
        b.onboard_path().to_lowercase().contains(&filter)
          || b
            .title
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains(&filter))
      });
    }
    if books.is_empty() {
      println!("\nNo sideloaded books.");
    }
    Ok(Session {
      device,
      model,
      flavor,
      books,
    })
  }

  fn hash(&self, book: &Book) -> std::result::Result<String, String> {
    partial_md5(&self.device.book_file(book)).map_err(|e| format!("file not readable: {e}"))
  }

  /// The stored position as a KOReader XPointer.
  fn xpointer(&self, book: &Book, stored: &str) -> std::result::Result<XPointer, String> {
    Epub::open(&self.device.book_file(book))
      .map_err(|e| e.to_string())
      .and_then(|mut epub| {
        to_xpointer(&mut epub, &Position::parse(stored), self.flavor).map_err(|e| e.to_string())
      })
  }

  /// The device position for KOSync, or why there is none.
  fn local(&self, book: &Book) -> std::result::Result<(Local, bool), String> {
    let stored = book.position.as_deref().ok_or("never opened")?;
    let x = self
      .xpointer(book, stored)
      .map_err(|e| format!("not translated: {e}"))?;
    let local = Local {
      xpointer: x.xpointer,
      percentage: book.percent_read as f64 / 100.0,
      last_read: book.last_read.as_deref().and_then(parse_timestamp),
    };
    Ok((local, x.exact))
  }
}

fn heading(book: &Book) {
  println!();
  let title = book.title.as_deref().unwrap_or("(no title)");
  match &book.author {
    Some(author) => println!("{title} — {author}"),
    None => println!("{title}"),
  }
}

/// The configuration and a client for its server.
struct Server {
  config: Config,
  client: Client,
}

impl Server {
  /// With `--config` the file must exist; the default file may be missing,
  /// then `None`.
  fn open(path: Option<&Path>) -> Result<Option<Self>> {
    let (path, required) = match path {
      Some(path) => (path.to_path_buf(), true),
      None => match Config::default_path() {
        Some(path) => (path, false),
        None => return Ok(None),
      },
    };
    if !required && !path.exists() {
      return Ok(None);
    }
    let config = Config::load(&path)?;
    if let Some(warning) = config::permission_warning(&path) {
      eprintln!("nickelinksync: warning: {warning}");
    }
    let client = Client::new(&config.server, &config.user, &config.key);
    Ok(Some(Server { config, client }))
  }

  /// Like [`Server::open`], but a configuration is required.
  fn require(path: Option<&Path>) -> Result<Self> {
    Server::open(path)?.ok_or_else(|| {
      let path = Config::default_path().unwrap_or_else(|| "config.ini".into());
      format!(
        "no configuration at {}; see README.md for the format",
        path.display()
      )
      .into()
    })
  }
}

fn percent(p: f64) -> String {
  format!("{:.0} %", p * 100.0)
}

fn describe(p: &Progress) -> String {
  format!(
    "{} ({}), from {} at {}",
    p.progress,
    percent(p.percentage),
    if p.device.is_empty() {
      "(unknown device)"
    } else {
      &p.device
    },
    format_timestamp(p.timestamp)
  )
}

fn status(target: &Target) -> Result<bool> {
  let session = Session::open(target)?;
  let server = Server::open(target.config.as_deref())?;
  match &server {
    Some(s) => println!("Server {} as {}", s.config.server, s.config.user),
    None => println!("Server not configured"),
  }
  // After the first failed request the server is not asked again.
  let mut server_error: Option<String> = None;
  for book in &session.books {
    heading(book);
    let format = if book.is_kepub() { "kepub" } else { "EPUB" };
    println!("  file      {} ({format})", book.onboard_path());
    let hash = session.hash(book);
    println!(
      "  hash      {}",
      hash.as_deref().unwrap_or_else(|e| e.as_str())
    );
    println!(
      "  position  {}",
      book.position.as_deref().unwrap_or("(never opened)")
    );
    let local = session.local(book);
    if let Some(stored) = &book.position {
      let koreader = match session.xpointer(book, stored) {
        Ok(x) if x.exact => x.xpointer,
        Ok(x) => format!("{} (chapter only)", x.xpointer),
        Err(e) => format!("(not translated: {e})"),
      };
      println!("  koreader  {koreader}");
    }
    let state = match book.read_status {
      2 => "finished",
      1 => "reading",
      _ => "unread",
    };
    match &book.last_read {
      Some(date) => println!(
        "  progress  {} %, {state}, last read {date}",
        book.percent_read
      ),
      None => println!("  progress  {} %, {state}", book.percent_read),
    }
    let (Some(server), Ok(hash)) = (&server, &hash) else {
      continue;
    };
    if let Some(e) = &server_error {
      println!("  server    (not checked: {e})");
      continue;
    }
    match server.client.progress(hash) {
      Ok(remote) => {
        match &remote {
          Some(p) => println!("  server    {}", describe(p)),
          None => println!("  server    (nothing stored)"),
        }
        if let Ok((local, _)) = &local {
          let sync = match compare(local, remote.as_ref()) {
            Compare::NotOnServer | Compare::DeviceNewer => "device is newer: push sends it",
            Compare::Same => "in sync",
            Compare::ServerNewer => "server is newer: pull would move the device",
          };
          println!("  sync      {sync}");
        }
      }
      Err(e) => {
        println!("  server    (failed: {e})");
        server_error = Some(e.to_string());
      }
    }
  }
  Ok(true)
}

fn push(target: &Target, force: bool, dry_run: bool) -> Result<bool> {
  let session = Session::open(target)?;
  let server = Server::require(target.config.as_deref())?;
  let (device, device_id) = server.config.identity(&session.model);
  server
    .client
    .authorize()
    .map_err(|e| format!("{}: {e}", server.config.server))?;
  println!(
    "Server {} as {}, device \"{device}\"",
    server.config.server, server.config.user
  );
  let mut ok = true;
  for book in &session.books {
    heading(book);
    let (hash, (local, exact)) = match (session.hash(book), session.local(book)) {
      (Ok(hash), Ok(local)) => (hash, local),
      (Err(e), _) | (_, Err(e)) => {
        println!("  skipped   {e}");
        continue;
      }
    };
    println!("  hash      {hash}");
    let remote = match server.client.progress(&hash) {
      Ok(remote) => remote,
      Err(e) => {
        println!("  failed    {e}");
        ok = false;
        continue;
      }
    };
    match compare(&local, remote.as_ref()) {
      Compare::Same => {
        println!("  in sync   {}", describe(remote.as_ref().unwrap()));
        continue;
      }
      Compare::ServerNewer if !force => {
        println!(
          "  skipped   server is newer: {}",
          describe(remote.as_ref().unwrap())
        );
        println!("            --force sends the device position anyway");
        continue;
      }
      _ => {}
    }
    let update = Update {
      document: hash,
      progress: local.xpointer,
      percentage: local.percentage,
      device: device.clone(),
      device_id: device_id.clone(),
    };
    let what = format!(
      "{}{} ({})",
      update.progress,
      if exact { "" } else { " (chapter only)" },
      percent(update.percentage)
    );
    if dry_run {
      println!("  would send {what}");
      continue;
    }
    match server.client.update(&update) {
      Ok(Some(t)) => println!("  sent      {what} at {}", format_timestamp(t)),
      Ok(None) => println!("  sent      {what}"),
      Err(e) => {
        println!("  failed    {e}");
        ok = false;
        if matches!(e, kosync::Error::Unauthorized) {
          break;
        }
      }
    }
  }
  if dry_run {
    println!("\nDry run: nothing was sent.");
  }
  Ok(ok)
}

fn pull(target: &Target) -> Result<bool> {
  let session = Session::open(target)?;
  let server = Server::require(target.config.as_deref())?;
  println!("Server {} as {}", server.config.server, server.config.user);
  let mut ok = true;
  for book in &session.books {
    heading(book);
    let hash = match session.hash(book) {
      Ok(hash) => hash,
      Err(e) => {
        println!("  skipped   {e}");
        continue;
      }
    };
    let remote = match server.client.progress(&hash) {
      Ok(Some(remote)) => remote,
      Ok(None) => {
        println!("  server    (nothing stored)");
        continue;
      }
      Err(e) => {
        println!("  failed    {e}");
        ok = false;
        if matches!(e, kosync::Error::Unauthorized) {
          break;
        }
        continue;
      }
    };
    let local = session.local(book);
    let comparison = match &local {
      Ok((local, _)) => compare(local, Some(&remote)),
      // Nothing on the device to keep.
      Err(_) => Compare::ServerNewer,
    };
    match comparison {
      Compare::Same => println!("  in sync   {}", describe(&remote)),
      Compare::DeviceNewer | Compare::NotOnServer => {
        println!("  keep      device is newer than {}", describe(&remote))
      }
      Compare::ServerNewer => {
        println!("  would move to {}", describe(&remote));
        if let Ok((local, _)) = &local {
          if remote.percentage < local.percentage {
            println!(
              "            backwards from {}: would ask first",
              percent(local.percentage)
            );
          }
        }
      }
    }
  }
  println!("\nDry run: nothing was written to the device (write-back is not implemented yet).");
  Ok(ok)
}
