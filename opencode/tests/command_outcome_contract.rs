//! Contract tests for deterministic command outcomes.

use clap::Parser;
use opencode_cli::cli::Cli;
use tempfile::TempDir;

#[tokio::test]
async fn run_command_returns_stdout_stderr_and_exit_code_for_success() {
    let cli = Cli::try_parse_from(["opencode", "version"]).expect("version cli parse");
    let cwd = TempDir::new().expect("temp dir");

    let outcome = opencode::run_command(cli, cwd.path()).await;

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        outcome.stdout,
        format!("opencode {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(outcome.stderr.is_empty());
}

#[tokio::test]
async fn run_command_returns_error_outcome_without_process_exit() {
    let cwd = TempDir::new().expect("temp dir");
    let bad_tool =
        Cli::try_parse_from(["opencode", "tool", "no_such_tool"]).expect("tool cli parse");

    let failed = opencode::run_command(bad_tool, cwd.path()).await;

    assert_ne!(failed.exit_code, 0);
    assert!(failed.stdout.is_empty());
    assert!(failed.stderr.contains("no_such_tool"));

    // If `run_command` called `process::exit`, this follow-up assertion would never run.
    let version = Cli::try_parse_from(["opencode", "version"]).expect("version cli parse");
    let recovered = opencode::run_command(version, cwd.path()).await;
    assert_eq!(recovered.exit_code, 0);
}
