use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
  /// Show the local and the server position of each book
  Status,
  /// Send the device position to the KOSync server
  Push,
  /// Fetch a newer position from the KOSync server
  Pull,
}

impl Command {
  fn name(&self) -> &'static str {
    match self {
      Command::Status => "status",
      Command::Push => "push",
      Command::Pull => "pull",
    }
  }
}

fn main() -> ExitCode {
  let cli = Cli::parse();
  eprintln!(
    "nickelinksync: `{}` is not implemented yet",
    cli.command.name()
  );
  ExitCode::FAILURE
}
