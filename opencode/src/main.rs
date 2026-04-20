//! opencode binary entry point.
//!
//! Thin shim: parse CLI args, run bootstrap, then render [`opencode::run_command`] outcomes.

use anyhow::Result;
use opencode_cli::{bootstrap::bootstrap, cli::Cli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse_args();
    let cwd = std::env::current_dir()?;

    if let Err(error) = bootstrap(&cwd).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }

    let outcome = opencode::run_command(cli, &cwd).await;
    if !outcome.stdout.is_empty() {
        print!("{}", outcome.stdout);
    }
    if !outcome.stderr.is_empty() {
        eprint!("{}", outcome.stderr);
    }

    if outcome.exit_code == 0 {
        return Ok(());
    }

    std::process::exit(outcome.exit_code);
}
