//! Snapshot tests for stable CLI help surfaces.

use clap::{CommandFactory, error::ErrorKind};
use opencode_cli::cli::Cli;

fn render_help(args: &[&str]) -> String {
    let mut command = Cli::command();
    let error = command
        .try_get_matches_from_mut(args)
        .expect_err("help flag should short-circuit with display output");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    error.to_string()
}

#[test]
fn root_help_snapshot() {
    insta::assert_snapshot!("root_help", render_help(&["opencode", "--help"]));
}

#[test]
fn serve_help_snapshot() {
    insta::assert_snapshot!("serve_help", render_help(&["opencode", "serve", "--help"]));
}

#[test]
fn providers_list_help_snapshot() {
    insta::assert_snapshot!(
        "providers_list_help",
        render_help(&["opencode", "providers", "list", "--help"])
    );
}

#[test]
fn session_list_help_snapshot() {
    insta::assert_snapshot!(
        "session_list_help",
        render_help(&["opencode", "session", "list", "--help"])
    );
}

#[test]
fn run_help_snapshot() {
    insta::assert_snapshot!("run_help", render_help(&["opencode", "run", "--help"]));
}

#[test]
fn prompt_help_snapshot() {
    insta::assert_snapshot!(
        "prompt_help",
        render_help(&["opencode", "prompt", "--help"])
    );
}
