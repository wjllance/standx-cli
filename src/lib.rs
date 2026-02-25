//! StandX Agent Toolkit
//!
//! The first AI Agent-native trading infrastructure for StandX.
//!
//! This crate provides a Rust client designed specifically for AI Agents and automated systems:
//! - Native MCP (Model Context Protocol) support
//! - Structured output by default
//! - Non-interactive design for automation
//! - Sub-100ms latency for real-time decisions
//!
//! ## Quick Start
//!
//! ```rust
//! use standx_cli::client::StandXClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = StandXClient::new()?;
//!     let ticker = client.get_symbol_market("BTC-USD").await?;
//!     println!("BTC Price: {}", ticker.mark_price);
//!     Ok(())
//! }
//! ```
//!
//! ## Agent Integration
//!
//! This library is designed to be used by AI Agents through MCP:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "standx": {
//!       "command": "standx",
//!       "args": ["mcp", "serve"]
//!     }
//!   }
//! }
//! ```
//!
//! ## Design Principles
//!
//! 1. **Agent-First**: Every API is designed for machine consumption
//! 2. **Structured Everything**: All outputs are JSON-serializable
//! 3. **Zero-Config**: Works out of the box with environment variables
//! 4. **Composable**: Tools can be combined into complex workflows

#![allow(clippy::result_large_err)]

pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod models;
pub mod output;
pub mod websocket;

pub use error::{Error, Result};
