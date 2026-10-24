use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nickelinksync::db::{Device, Error};
use nickelinksync::hash::partial_md5;

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
  /// Show the position of each sideloaded book on the device
  Status {
    /// Mount point of the device, e.g. /Volumes/KOBOeReader
    #[arg(long)]
    device: PathBuf,
  },
  /// Send the device position to the KOSync server
  Push,
  /// Fetch a newer position from the KOSync server
  Pull,
}

fn main() -> ExitCode {
  let cli = Cli::parse();
  let result = match cli.command {
    Command::Status { device } => status(&device),
    Command::Push => not_implemented("push"),
    Command::Pull => not_implemented("pull"),
  };
  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("nickelinksync: {e}");
      ExitCode::FAILURE
    }
  }
}

fn not_implemented(name: &str) -> Result<(), Error> {
  eprintln!("nickelinksync: `{name}` is not implemented yet");
  std::process::exit(1)
}

fn status(root: &Path) -> Result<(), Error> {
  let device = Device::open(root)?;
  let family = if device.db.is_tolino()? {
    "Tolino"
  } else {
    "Kobo"
  };
  match &device.firmware {
    Some(fw) => println!(
      "{}, firmware {} (database version {})",
      fw.model_name().unwrap_or(family),
      fw.version,
      device.db.version()?
    ),
    None => println!("{family}, database version {}", device.db.version()?),
  }
  let books = device.db.sideloaded_books()?;
  if books.is_empty() {
    println!("\nNo sideloaded books.");
  }
  for book in &books {
    println!();
    let title = book.title.as_deref().unwrap_or("(no title)");
    match &book.author {
      Some(author) => println!("{title} — {author}"),
      None => println!("{title}"),
    }
    let format = if book.is_kepub() { "kepub" } else { "EPUB" };
    println!("  file      {} ({format})", book.onboard_path());
    let hash = match partial_md5(&device.book_file(book)) {
      Ok(hash) => hash,
      Err(e) => format!("(file not readable: {e})"),
    };
    println!("  hash      {hash}");
    println!(
      "  position  {}",
      book.position.as_deref().unwrap_or("(never opened)")
    );
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
  }
  Ok(())
}
