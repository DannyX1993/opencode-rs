//! Clap CLI definition.

use clap::{Parser, Subcommand};

/// opencode — an open source AI coding agent.
#[derive(Parser, Debug)]
#[command(
    name    = "opencode",
    version = env!("CARGO_PKG_VERSION"),
    about   = "An open source AI coding agent",
    long_about = None,
)]
pub struct Cli {
    /// Subcommand to run (default: interactive TUI).
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Log level override (trace|debug|info|warn|error).
    #[arg(long, env = "OPENCODE_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Emit JSON-structured logs.
    #[arg(long)]
    pub log_json: bool,
}

/// Available opencode subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the interactive TUI (default).
    Run,

    /// Start the HTTP API server in headless mode.
    Server {
        /// Port to listen on.
        #[arg(short, long, default_value_t = 4141)]
        port: u16,
    },

    /// Run a single one-shot prompt and exit.
    Prompt {
        /// The prompt text.
        text: String,
        /// Output format: text | json.
        #[arg(long, default_value = "text")]
        output: String,
    },

    /// Print version information.
    Version,

    /// Show or edit configuration.
    Config {
        /// Print the merged config as JSON.
        #[arg(long)]
        show: bool,
    },

    /// Invoke a built-in tool from the command line.
    ///
    /// Example:
    ///   opencode tool read --args-json '{"filePath":"Cargo.toml","limit":5}'
    ///   opencode tool bash --args-json '{"command":"pwd","description":"print cwd"}'
    Tool {
        /// Tool name (e.g. read, bash, list, glob, grep, write).
        name: String,

        /// JSON-encoded arguments for the tool (default: `{}`).
        #[arg(long, value_name = "JSON")]
        args_json: Option<String>,

        /// Output format: text | json.
        #[arg(long, default_value = "text")]
        output: String,
    },
}

impl Cli {
    /// Parse CLI arguments from `std::env::args`.
    #[must_use]
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn default_no_subcommand() {
        let cli = Cli::try_parse_from(["opencode"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.log_level, "info");
        assert!(!cli.log_json);
    }

    #[test]
    fn run_subcommand() {
        let cli = Cli::try_parse_from(["opencode", "run"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Run)));
    }

    #[test]
    fn server_subcommand_default_port() {
        let cli = Cli::try_parse_from(["opencode", "server"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Server { port: 4141 })));
    }

    #[test]
    fn server_subcommand_custom_port() {
        let cli = Cli::try_parse_from(["opencode", "server", "--port", "8080"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Server { port: 8080 })));
    }

    #[test]
    fn prompt_subcommand() {
        let cli = Cli::try_parse_from(["opencode", "prompt", "hello world"]).unwrap();
        if let Some(Command::Prompt { text, output }) = cli.command {
            assert_eq!(text, "hello world");
            assert_eq!(output, "text");
        } else {
            panic!("expected Prompt subcommand");
        }
    }

    #[test]
    fn version_subcommand() {
        let cli = Cli::try_parse_from(["opencode", "version"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Version)));
    }

    #[test]
    fn config_show_flag() {
        let cli = Cli::try_parse_from(["opencode", "config", "--show"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Config { show: true })));
    }

    #[test]
    fn log_json_flag() {
        let cli = Cli::try_parse_from(["opencode", "--log-json"]).unwrap();
        assert!(cli.log_json);
    }

    #[test]
    fn log_level_override() {
        let cli = Cli::try_parse_from(["opencode", "--log-level", "debug"]).unwrap();
        assert_eq!(cli.log_level, "debug");
    }

    // ── B.1 RED: tool subcommand parsing tests ────────────────────────────────

    #[test]
    fn tool_subcommand_parses_name() {
        let cli = Cli::try_parse_from(["opencode", "tool", "read"]).unwrap();
        if let Some(Command::Tool { name, .. }) = cli.command {
            assert_eq!(name, "read");
        } else {
            panic!("expected Tool subcommand");
        }
    }

    #[test]
    fn tool_subcommand_default_output_text() {
        let cli = Cli::try_parse_from(["opencode", "tool", "bash"]).unwrap();
        if let Some(Command::Tool { output, .. }) = cli.command {
            assert_eq!(output, "text");
        } else {
            panic!("expected Tool subcommand");
        }
    }

    #[test]
    fn tool_subcommand_json_output_flag() {
        let cli = Cli::try_parse_from(["opencode", "tool", "read", "--output", "json"]).unwrap();
        if let Some(Command::Tool { output, .. }) = cli.command {
            assert_eq!(output, "json");
        } else {
            panic!("expected Tool subcommand");
        }
    }

    #[test]
    fn tool_subcommand_args_json_flag() {
        let cli = Cli::try_parse_from([
            "opencode",
            "tool",
            "read",
            "--args-json",
            r#"{"filePath":"Cargo.toml"}"#,
        ])
        .unwrap();
        if let Some(Command::Tool { args_json, .. }) = cli.command {
            assert_eq!(args_json.unwrap(), r#"{"filePath":"Cargo.toml"}"#);
        } else {
            panic!("expected Tool subcommand");
        }
    }

    #[test]
    fn tool_subcommand_missing_name_errors() {
        let result = Cli::try_parse_from(["opencode", "tool"]);
        assert!(result.is_err(), "tool subcommand requires a name argument");
    }
}
