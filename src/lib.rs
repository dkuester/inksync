//! NickelInkSync syncs the reading progress of sideloaded ebooks between Kobo,
//! Tolino and KOReader through a KOSync server.
//!
//! The library holds all sync logic; the `nickelinksync` binary is a thin CLI
//! on top of it, used on the desktop and on the device. See `SCOPE.md` for the
//! architecture.

pub mod config;
pub mod db;
pub mod dom;
pub mod epub;
pub mod hash;
pub mod kosync;
pub mod position;
pub mod sync;
