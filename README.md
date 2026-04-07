# opencode-rs

Rust port of [opencode](https://github.com/opencode-ai/opencode) — an open source AI coding agent.

> **Status**: Active development — Phase 3 complete. Core tool infrastructure, HTTP API server,
> and manual CLI tool invocation are functional. TUI and full agent loop are planned for
> later phases.

---

## Workspace Layout

| Crate                                           | Status      | Description                                    |
| ----------------------------------------------- | ----------- | ---------------------------------------------- |
| [`opencode-tool`](crates/opencode-tool)         | ✅ Complete | Tool trait, registry, 6 built-in tools         |
| [`opencode-cli`](crates/opencode-cli)           | ✅ Complete | CLI parsing, bootstrap, `tool` subcommand      |
| [`opencode-core`](crates/opencode-core)         | ✅ Complete | Config, tracing init                           |
| [`opencode-provider`](crates/opencode-provider) | ✅ Complete | OpenAI, Anthropic, Google provider adapters    |
| [`opencode-server`](crates/opencode-server)     | ✅ Partial  | REST API + SSE streaming                       |
| [`opencode-storage`](crates/opencode-storage)   | ✅ Complete | SQLite storage layer (sqlx)                    |
| [`opencode-session`](crates/opencode-session)   | 🔲 Stub     | Session engine (wiring present, logic pending) |
| [`opencode-bus`](crates/opencode-bus)           | 🔲 Stub     | Broadcast event bus                            |
| [`opencode-lsp`](crates/opencode-lsp)           | 🔲 Planned  | Language Server Protocol integration           |
| [`opencode-mcp`](crates/opencode-mcp)           | 🔲 Planned  | Model Context Protocol support                 |
| [`opencode-plugin`](crates/opencode-plugin)     | 🔲 Planned  | Plugin system                                  |
| [`opencode-tui`](crates/opencode-tui)           | 🔲 Planned  | Terminal user interface                        |
| [`opencode`](opencode)                          | ✅ Active   | Binary entrypoint + dispatch                   |

---

## Quick Start

### Prerequisites

- Rust 1.85+ (see `rust-toolchain.toml` or `Cargo.toml`)
- `cargo-llvm-cov` for coverage reports (optional)

```sh
# Clone
git clone https://github.com/opencode-ai/opencode
cd opencode/opencode-rs

# Build
cargo build -p opencode

# Run (interactive TUI — not yet implemented, logs a stub message)
cargo run -p opencode

# Run the HTTP API server on port 4141
cargo run -p opencode -- server --port 4141

# Print current config as JSON
cargo run -p opencode -- config --show
```

---

## Manual Tool Invocation

The `tool` subcommand lets you invoke any built-in tool directly from the shell.
This is useful for scripting, debugging, and verifying tool behaviour.

Bootstrap logs are always written to **stderr**; stdout carries only tool output,
making JSON mode safe for piping and scripting.

### Syntax

```sh
opencode tool <TOOL_NAME> [--args-json '<JSON>'] [--output text|json]
```

The `--output` flag accepts exactly `text` (default) or `json`. Any other value
is rejected with exit code 1.

### Examples

```sh
# Read a file (first 5 lines)
cargo run -p opencode -- tool read \
  --args-json '{"filePath":"Cargo.toml","limit":5}'

# List directory tree
cargo run -p opencode -- tool list \
  --args-json '{"path":"crates"}'

# Glob for Rust source files
cargo run -p opencode -- tool glob \
  --args-json '{"pattern":"**/*.rs","path":"crates/opencode-tool"}'

# Grep for a pattern
cargo run -p opencode -- tool grep \
  --args-json '{"pattern":"pub fn","path":"crates/opencode-tool","include":"*.rs"}'

# Write a file
cargo run -p opencode -- tool write \
  --args-json '{"filePath":"/tmp/hello.txt","content":"hello world\n"}'

# Run a shell command
cargo run -p opencode -- tool bash \
  --args-json '{"command":"echo hello from opencode","description":"greet"}'

# JSON output — stdout is clean JSON, bootstrap logs go to stderr
cargo run -p opencode -- tool bash \
  --args-json '{"command":"pwd","description":"print cwd"}' \
  --output json
```

### Available Tools

| Tool name | Description                        | TS equivalent |
| --------- | ---------------------------------- | ------------- |
| `read`    | Read a file or directory listing   | `ReadTool`    |
| `list`    | Directory tree view                | `ListTool`    |
| `glob`    | Find files matching a glob pattern | `GlobTool`    |
| `grep`    | Search file contents with regex    | `GrepTool`    |
| `write`   | Write content to a file            | `WriteTool`   |
| `bash`    | Execute a shell command            | `BashTool`    |

### Exit Codes

| Code | Meaning                                                                      |
| ---- | ---------------------------------------------------------------------------- |
| 0    | Tool ran successfully                                                        |
| 1    | Tool error (not found, invalid args, exec failure, invalid `--output` value) |
| 2    | CLI parse error (missing required arg, unknown flag — clap default)          |

---

## Running Tests

```sh
# All tests for the core crates
cargo test -p opencode-tool -p opencode-cli -p opencode --lib

# Full workspace
cargo test --workspace --lib

# Coverage report (requires cargo-llvm-cov)
cargo llvm-cov -p opencode-tool --summary-only
cargo llvm-cov -p opencode-cli --summary-only
```

Current coverage (Phase 3):

| Crate           | Line | Region |
| --------------- | ---- | ------ |
| `opencode-tool` | ≥96% | ≥94%   |
| `opencode-cli`  | ≥95% | ≥94%   |

---

## Architecture Notes

- `opencode-tool` is the heart: it defines the `Tool` trait and all built-in implementations.
- `opencode-cli` is deliberately thin — it only parses flags and delegates to `tool_cmd::run`.
- The `Ctx` struct carries project root, CWD, shell path, and timeout — it threads through every tool invocation.
- GlobTool and GrepTool use native Rust crates (`globset`, `regex`, `walkdir`) instead of spawning `rg`, giving deterministic, subprocess-free behaviour.
- All tracing output goes to **stderr** — stdout is reserved for tool output only.

---

## Contributing

See [`docs/`](docs/) for design notes. All changes must follow the Spec-Driven Development
flow (proposal → spec → design → tasks → apply → verify).

Rust style guidelines:

- Single-word identifiers by default
- No `try/catch` equivalents (use `?` and `anyhow`)
- `cargo clippy -- -D warnings` must be clean
- Coverage gate: ≥85% line and region per crate

---

## License

MIT — see [LICENSE](../LICENSE) or `Cargo.toml` workspace metadata.
