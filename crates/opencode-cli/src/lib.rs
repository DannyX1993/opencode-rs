//! # opencode-cli
//!
//! Thin command-line layer: parses subcommands, bootstraps all services,
//! and launches the TUI, server, or one-shot prompt as requested.

#![warn(missing_docs)]

pub mod backend_client;
pub mod bootstrap;
pub mod cli;
pub mod commands;
pub mod tool_cmd;
