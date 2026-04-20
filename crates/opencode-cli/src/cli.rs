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
    /// Start the interactive TUI (default) or run a one-shot prompt when text is provided.
    Run {
        /// One-shot prompt text (non-interactive mode) as whitespace-joined tokens.
        text: Vec<String>,
        /// Output format for non-interactive mode: text | json.
        #[arg(long, default_value = "text")]
        output: String,
        /// Timeout budget for backend request acceptance in milliseconds.
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },

    /// Start the HTTP API server in headless mode.
    /// Bind precedence: CLI > resolved config > defaults.
    #[command(visible_alias = "server")]
    Serve {
        /// Bind host override (CLI > resolved config > defaults).
        #[arg(long)]
        host: Option<String>,
        /// Bind port override (CLI > resolved config > defaults).
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Run a single one-shot prompt and exit.
    Prompt {
        /// The prompt text.
        text: String,
        /// Output format: text | json.
        #[arg(long, default_value = "text")]
        output: String,
        /// Timeout budget for backend request acceptance in milliseconds.
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
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

    /// Inspect configured and available providers.
    Providers {
        /// Providers subcommand.
        #[command(subcommand)]
        command: ProvidersCommand,
    },

    /// Inspect project sessions for the current working directory.
    Session {
        /// Session subcommand.
        #[command(subcommand)]
        command: SessionCommand,
    },
}

/// Provider inspection subcommands.
#[derive(Subcommand, Debug)]
pub enum ProvidersCommand {
    /// List providers in table or JSON mode.
    List {
        /// Output format: text | json.
        #[arg(long, default_value = "text")]
        output: String,
    },
}

/// Session inspection subcommands.
#[derive(Subcommand, Debug)]
pub enum SessionCommand {
    /// List sessions for the cwd-resolved project.
    List,
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
    use clap::{CommandFactory, Parser};

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
        assert!(matches!(
            cli.command,
            Some(Command::Run {
                text,
                output,
                timeout_ms
            }) if text.is_empty() && output == "text" && timeout_ms == 30_000
        ));
    }

    #[test]
    fn run_subcommand_accepts_message_and_output_flags_for_noninteractive_mode() {
        let cli = Cli::try_parse_from([
            "opencode",
            "run",
            "explain",
            "rust",
            "lifetimes",
            "--output",
            "json",
            "--timeout-ms",
            "1500",
        ])
        .unwrap();

        assert!(matches!(cli.command, Some(Command::Run { .. })));
    }

    #[test]
    fn server_subcommand_default_port() {
        let cli = Cli::try_parse_from(["opencode", "server"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Serve {
                host: None,
                port: None
            })
        ));
    }

    #[test]
    fn server_subcommand_custom_port() {
        let cli = Cli::try_parse_from(["opencode", "server", "--port", "8080"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Serve {
                host: None,
                port: Some(8080)
            })
        ));
    }

    #[test]
    fn server_subcommand_custom_host() {
        let cli = Cli::try_parse_from(["opencode", "server", "--host", "127.0.0.9"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Serve {
                host: Some(ref host),
                port: None
            }) if host == "127.0.0.9"
        ));
    }

    #[test]
    fn serve_subcommand_parses_bind_flags() {
        let cli =
            Cli::try_parse_from(["opencode", "serve", "--host", "127.0.0.9", "--port", "8088"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Serve {
                host: Some(ref host),
                port: Some(8088)
            }) if host == "127.0.0.9"
        ));
    }

    #[test]
    fn server_help_mentions_bind_precedence() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(
            help.contains("CLI > resolved config > defaults"),
            "expected precedence hint in help text, got: {help}"
        );
    }

    #[test]
    fn prompt_subcommand() {
        let cli = Cli::try_parse_from(["opencode", "prompt", "hello world"]).unwrap();
        if let Some(Command::Prompt {
            text,
            output,
            timeout_ms,
        }) = cli.command
        {
            assert_eq!(text, "hello world");
            assert_eq!(output, "text");
            assert_eq!(timeout_ms, 30_000);
        } else {
            panic!("expected Prompt subcommand");
        }
    }

    #[test]
    fn prompt_subcommand_supports_timeout_for_deterministic_noninteractive_execution() {
        let cli =
            Cli::try_parse_from(["opencode", "prompt", "hello world", "--timeout-ms", "2500"])
                .unwrap();

        if let Some(Command::Prompt {
            output, timeout_ms, ..
        }) = cli.command
        {
            assert_eq!(output, "text");
            assert_eq!(timeout_ms, 2500);
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

    #[test]
    fn providers_list_subcommand_default_output_text() {
        let cli = Cli::try_parse_from(["opencode", "providers", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Providers {
                command: ProvidersCommand::List { ref output }
            }) if output == "text"
        ));
    }

    #[test]
    fn providers_list_subcommand_json_output() {
        let cli =
            Cli::try_parse_from(["opencode", "providers", "list", "--output", "json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Providers {
                command: ProvidersCommand::List { ref output }
            }) if output == "json"
        ));
    }

    #[test]
    fn session_list_subcommand_parses() {
        let cli = Cli::try_parse_from(["opencode", "session", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Session {
                command: SessionCommand::List
            })
        ));
    }
}
