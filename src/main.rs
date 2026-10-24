use std::error::Error;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use nickelinksync::config::{self, Config};
use nickelinksync::db::{Book, Device, Write};
use nickelinksync::epub::Epub;
use nickelinksync::hash::partial_md5;
use nickelinksync::kosync::{self, Client, Progress, Update};
use nickelinksync::position::{
  from_xpointer, to_xpointer, PointFlavor, Position, Translated, XPointer,
};
use nickelinksync::sync::{
  self, decide, format_timestamp, iso_timestamp, parse_timestamp, Confirm, Decision, Direction,
  Local, Side,
};

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
    /// Send the device position without asking, also if the server's is newer
    #[arg(long)]
    force: bool,
    /// Only show what would be sent
    #[arg(long)]
    dry_run: bool,
  },
  /// Show what a newer position on the KOSync server would change, and with
  /// --write write it to the device
  Pull {
    #[command(flatten)]
    target: Target,
    /// Take the server position without asking, also if the device's is newer
    #[arg(long)]
    force: bool,
    /// Write the positions into the device database (Tolino only so far),
    /// after a backup copy of it
    #[arg(long)]
    write: bool,
    /// For the plugin: print each position to take as one line
    /// `NICKELINKSYNC_POSITION<tab>percent<tab>ChapterIDBookmarked`
    #[arg(long)]
    emit: bool,
  },
}

#[derive(Args)]
struct Target {
  /// Mount point of the device, e.g. /Volumes/KOBOeReader (/mnt/onboard on
  /// the device itself)
  #[arg(long)]
  device: PathBuf,
  /// Configuration file [default: ~/.config/nickelinksync/config.ini]
  #[arg(long)]
  config: Option<PathBuf>,
  /// Only books whose title or file path contains this text
  #[arg(long)]
  book: Option<String>,
  /// Only the book with exactly this ContentID (file:///mnt/onboard/…)
  #[arg(long)]
  content_id: Option<String>,
  /// Nickel is running (on the device): read its database as it is being
  /// written, never write it
  #[arg(long)]
  live: bool,
  /// Seconds each request to the server may take
  #[arg(long, default_value_t = 30)]
  timeout: u64,
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
    Command::Pull {
      target,
      force,
      write,
      emit,
    } => pull(&target, force, write, emit),
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
    let device = Device::open(&target.device, target.live)?;
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
    if let Some(id) = &target.content_id {
      books.retain(|b| &b.content_id == id);
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

  /// A server XPointer as the device would store it.
  fn translate(&self, book: &Book, xpointer: &str) -> std::result::Result<Translated, String> {
    Epub::open(&self.device.book_file(book))
      .map_err(|e| e.to_string())
      .and_then(|mut epub| {
        from_xpointer(&mut epub, xpointer, self.flavor).map_err(|e| e.to_string())
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
  fn open(target: &Target) -> Result<Option<Self>> {
    let path = target.config.as_deref();
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
    // On the device's FAT storage every file looks world-readable.
    if !target.live {
      if let Some(warning) = config::permission_warning(&path) {
        eprintln!("nickelinksync: warning: {warning}");
      }
    }
    let timeout = std::time::Duration::from_secs(target.timeout.max(1));
    let client = Client::with_timeout(&config.server, &config.user, &config.key, timeout);
    Ok(Some(Server { config, client }))
  }

  /// Like [`Server::open`], but a configuration is required.
  fn require(target: &Target) -> Result<Self> {
    Server::open(target)?.ok_or_else(|| {
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
  let server = Server::open(target)?;
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
          println!(
            "  sync      {}",
            sync_note(local, remote.as_ref(), server.config.confirm)
          );
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

/// What `push` and `pull` would do with a book, for `status`.
fn sync_note(local: &Local, remote: Option<&Progress>, confirm: Confirm) -> String {
  let Some(remote) = remote else {
    return "device is newer: push sends it".into();
  };
  let push = decide(Direction::Push, local, remote, confirm);
  let pull = decide(Direction::Pull, local, remote, confirm);
  // Otherwise one of them asks: the one moving to the newer position.
  let (newer, other, command, ask) = match (push, pull) {
    (Decision::InSync, _) => return "in sync".into(),
    (Decision::Apply, _) => return "device is newer: push sends it".into(),
    (_, Decision::Apply) => return "server is newer: pull would move the device".into(),
    (Decision::Ask { newer: true, .. }, _) => ("device", "server", "push", push),
    _ => ("server", "device", "pull", pull),
  };
  if matches!(
    ask,
    Decision::Ask {
      backwards: true,
      ..
    }
  ) {
    format!("{newer} is newer, but behind the {other}: {command} asks first")
  } else {
    format!("{newer} is newer: {command} asks first (confirm = {confirm})")
  }
}

/// Shows why a book needs an answer and gets one: from the terminal, or,
/// in a dry run or without a terminal, none. `true` if `direction` should
/// overwrite the other side.
fn confirm(
  direction: Direction,
  local: &Local,
  remote: &Progress,
  (backwards, confirm): (bool, Confirm),
  dry_run: bool,
  unanswered: &mut usize,
) -> Result<bool> {
  match local.last_read {
    Some(t) => println!(
      "  device    {} ({}), last read {}",
      local.xpointer,
      percent(local.percentage),
      format_timestamp(t)
    ),
    None => println!(
      "  device    {} ({})",
      local.xpointer,
      percent(local.percentage)
    ),
  }
  println!("  server    {}", describe(remote));
  let (new, old) = match direction {
    Direction::Push => ("device", "server"),
    Direction::Pull => ("server", "device"),
  };
  if backwards {
    println!("  ask       the {new} position lies before the {old}'s (confirm = {confirm})");
  } else {
    println!("  ask       the positions differ (confirm = {confirm})");
  }
  if dry_run {
    println!("            would ask first");
    return Ok(false);
  }
  if !std::io::stdin().is_terminal() {
    println!("  skipped   needs an answer, but there is no terminal; --force overwrites anyway");
    *unanswered += 1;
    return Ok(false);
  }
  let side = sync::ask(&mut std::io::stdin().lock(), &mut std::io::stdout())?;
  let apply = match (side, direction) {
    (None, _) => {
      println!("  skipped   no answer");
      return Ok(false);
    }
    (Some(Side::Device), Direction::Push) | (Some(Side::Server), Direction::Pull) => true,
    (Some(_), _) => false,
  };
  if !apply {
    println!("  kept      the {old} position");
  }
  Ok(apply)
}

fn unanswered_note(unanswered: usize) {
  if unanswered > 0 {
    println!("\n{unanswered} book(s) need an answer: run again in a terminal, or with --force.");
  }
}

fn push(target: &Target, force: bool, dry_run: bool) -> Result<bool> {
  let session = Session::open(target)?;
  let server = Server::require(target)?;
  let (device, device_id) = server.config.identity(&session.model);
  server
    .client
    .authorize()
    .map_err(|e| format!("{}: {e}", server.config.server))?;
  println!(
    "Server {} as {}, device \"{device}\"",
    server.config.server, server.config.user
  );
  let confirm_setting = server.config.confirm;
  let mut unanswered = 0;
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
    if let Some(remote) = &remote {
      match decide(Direction::Push, &local, remote, confirm_setting) {
        Decision::InSync => {
          println!("  in sync   {}", describe(remote));
          continue;
        }
        Decision::Apply => {}
        _ if force => {}
        Decision::Keep => {
          println!("  skipped   server is newer: {}", describe(remote));
          println!("            --force sends the device position anyway");
          continue;
        }
        Decision::Ask { backwards, .. } => {
          let answer = (backwards, confirm_setting);
          if !confirm(
            Direction::Push,
            &local,
            remote,
            answer,
            dry_run,
            &mut unanswered,
          )? {
            continue;
          }
        }
      }
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
  unanswered_note(unanswered);
  if dry_run {
    println!("\nDry run: nothing was sent.");
  }
  Ok(ok)
}

fn pull(target: &Target, force: bool, write: bool, emit: bool) -> Result<bool> {
  let session = Session::open(target)?;
  let server = Server::require(target)?;
  println!("Server {} as {}", server.config.server, server.config.user);
  let confirm_setting = server.config.confirm;
  let mut unanswered = 0;
  let mut writes = Vec::new();
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
    println!(
      "  stored    {}",
      book.position.as_deref().unwrap_or("(never opened)")
    );
    // Without a device position there is nothing on the device to keep.
    let local = session.local(book).ok().map(|(local, _)| local);
    let decision = match &local {
      Some(local) => decide(Direction::Pull, local, &remote, confirm_setting),
      None => Decision::Apply,
    };
    let apply = match (decision, &local) {
      (Decision::InSync, _) => {
        println!("  in sync   {}", describe(&remote));
        false
      }
      (Decision::Apply, _) => true,
      (_, Some(_)) if force => true,
      (Decision::Keep, _) => {
        println!("  keep      device is newer than {}", describe(&remote));
        println!("            --force takes the server position anyway");
        false
      }
      (Decision::Ask { backwards, .. }, Some(local)) => {
        let answer = (backwards, confirm_setting);
        confirm(
          Direction::Pull,
          local,
          &remote,
          answer,
          !write,
          &mut unanswered,
        )?
      }
      (Decision::Ask { .. }, None) => unreachable!("asks only with a device position"),
    };
    if !apply {
      continue;
    }
    let verb = if write { "move to  " } else { "would move to" };
    println!("  {verb} {}", describe(&remote));
    let translated = match session.translate(book, &remote.progress) {
      Ok(t) => t,
      Err(e) => {
        println!("  skipped   not translated: {e}");
        continue;
      }
    };
    let only = if translated.exact {
      ""
    } else {
      " (chapter only)"
    };
    println!("  on device {}{only}", translated.position);
    // Keep the server the newer side, so the next push does not send the
    // old device position back: DateLastRead stays as it is unless it is
    // newer than the server's position.
    let last_read = local
      .and_then(|l| l.last_read)
      .filter(|&t| t >= remote.timestamp)
      .map(|_| iso_timestamp(remote.timestamp - 1));
    let percent = (remote.percentage * 100.0).round().clamp(0.0, 100.0) as i64;
    if emit {
      println!("NICKELINKSYNC_POSITION\t{percent}\t{}", translated.position);
    }
    writes.push(Write {
      content_id: book.content_id.clone(),
      position: translated.position.to_string(),
      percent,
      last_read,
    });
  }
  unanswered_note(unanswered);
  if !write && target.live {
    println!("\nNothing was written to the device database (Nickel is running).");
  } else if !write {
    println!("\nDry run: nothing was written to the device; --write writes it.");
  } else if writes.is_empty() {
    println!("\nNothing to write to the device.");
  } else {
    let dir = Config::backup_dir().ok_or("no directory for the backup (HOME is not set)")?;
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)?
      .as_secs() as i64;
    let stamp: String = iso_timestamp(now)
      .chars()
      .filter(|c| !"-:".contains(*c))
      .collect();
    if target.live {
      // Only the one book the plugin is about to open.
      if target.content_id.is_none() {
        return Err("--write with --live needs --content-id".into());
      }
      let made = session.device.write_live(&writes, &dir, &stamp[..8])?;
      println!(
        "\nWrote {} position(s) to the device database while Nickel runs.",
        writes.len()
      );
      if let Some(backup) = made {
        println!("Backup of the database (one a day): {}", backup.display());
      }
      return Ok(ok);
    }
    let backup = dir.join(format!("KoboReader-{stamp}.sqlite"));
    session.device.write_positions(&writes, &backup)?;
    println!("\nWrote {} position(s) to the device.", writes.len());
    println!("Backup of the database: {}", backup.display());
    println!("Eject the device before you open the book.");
  }
  Ok(ok)
}
