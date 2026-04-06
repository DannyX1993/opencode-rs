//! opencode binary entry point.
//!
//! Thin shim: parse CLI args, run bootstrap, then delegate to [`opencode::dispatch`].

use anyhow::Result;
use opencode_cli::{bootstrap::bootstrap, cli::Cli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse_args();
    let cwd = std::env::current_dir()?;
    let _cfg = bootstrap(&cwd).await?;
    opencode::dispatch(cli, &cwd).await
}
