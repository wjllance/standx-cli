//! StandX CLI library
//!
//! Command definitions, handlers, output formatting, configuration, and
//! telemetry for the `standx` binary. Protocol types and API access live in
//! the [`standx_sdk`] crate and are re-exported here for convenience (and so
//! existing `standx_cli::models::...` imports keep working).

pub mod cli;
pub mod commands;
pub mod config;
pub mod output;
pub mod telemetry;

// Re-export the SDK surface so `standx_cli::{models, client, ...}` and
// `crate::{models, error}` (used by output.rs/config.rs) keep resolving.
pub use standx_sdk::{auth, client, error, models, websocket};
pub use standx_sdk::{Error, Result};
