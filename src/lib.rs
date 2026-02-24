//! StandX API client library
//!
//! This crate provides a Rust client for the StandX perpetual DEX API.

pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod models;

pub use error::{Error, Result};
