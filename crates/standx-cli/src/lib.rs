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

/// Process-global lock serializing every test that reads or mutates process
/// environment variables.
///
/// `std::env::set_var`/`remove_var` mutate a process-global table that is not
/// synchronized against concurrent `std::env::var` reads (the reason
/// `set_var` becomes `unsafe` in edition 2024). On glibc a concurrent
/// mutation can realloc `environ` mid-read, so an *unrelated* `var()` lookup
/// spuriously returns "unset". Every env-touching test across all modules must
/// hold THIS single lock for the duration it depends on the environment —
/// per-module locks do not exclude cross-module races (e.g. a pipeline test
/// mutating `STANDX_JWT` while the maker cleanup test reads it).
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
