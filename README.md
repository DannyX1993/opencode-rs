# opencode-rs

Rust workspace for the `opencode` agent runtime and supporting libraries.

## Status

`opencode-rs` builds a real CLI, a headless HTTP server, provider adapters, and a SQLite-backed storage layer. Several crates in the workspace are still stubs or placeholders, so this workspace is not yet a complete replacement for the TypeScript implementation.

This README documents only the Rust workspace in this directory. `opencode-ts/` is intentionally out of scope.

## Workspace Layout

| Path | Type | Status | Purpose |
| --- | --- | --- | --- |
| `opencode/` | binary crate | active | Binary entrypoint and command dispatch |
| `crates/opencode-cli/` | library crate | active | Clap CLI definitions, bootstrap, `tool` command wiring |
| `crates/opencode-core/` | library crate | active | Shared config, DTOs, IDs, errors, tracing helpers |
| `crates/opencode-provider/` | library crate | active | Provider trait, registry, OpenAI/Anthropic/Google adapters |
| `crates/opencode-server/` | library crate | active | Axum router, health route, project/session/message API, provider harness |
| `crates/opencode-storage/` | library crate | active | SQLite persistence, migrations, repositories, event store |
| `crates/opencode-tool/` | library crate | active | Tool trait, registry, built-in read/list/glob/grep/write/bash tools |
| `crates/opencode-bus/` | library crate | partial | Typed in-process broadcast bus with published event types |
| `crates/opencode-session/` | library crate | stub | Session trait surface plus stub engine returning `NotFound` |
| `crates/opencode-lsp/` | library crate | stub | Placeholder for future LSP integration |
| `crates/opencode-mcp/` | library crate | stub | Placeholder for future MCP integration |
| `crates/opencode-plugin/` | library crate | stub | Placeholder for future plugin host |
| `crates/opencode-tui/` | library crate | stub | Placeholder for future terminal UI |
| `docs/` | docs | active | Manual testing and workspace documentation |
| `scripts/` | scripts | active | Helper scripts such as coverage reporting |

## Version

Current Rust workspace version: `0.5.0`

The workspace uses `version.workspace = true`, so crate package versions inherit from the root `Cargo.toml`.

## What Works Today

- `cargo run -p opencode -- version` prints the binary version.
- `cargo run -p opencode -- config --show` prints merged configuration as JSON.
- `cargo run -p opencode -- tool ...` invokes built-in tools directly.
- `cargo run -p opencode -- server` starts an Axum server with `/health` plus project/session/message routes.
- `POST /api/v1/provider/stream` exists only as a manual harness and is disabled unless `OPENCODE_MANUAL_HARNESS=1` is set.
- SQLite storage is initialized from the current working directory using `opencode.db`.

## What Is Still Incomplete

- `run` currently logs a stub message instead of launching a TUI.
- `prompt <text>` currently logs a stub message instead of executing a full agent loop.
- `config` without `--show` currently logs a stub message.
- `opencode-session` does not yet implement the real session engine.
- `opencode-lsp`, `opencode-mcp`, `opencode-plugin`, and `opencode-tui` are scaffolding crates with minimal code.

## Quick Start

Prerequisites:

- Rust `1.85` or newer
- SQLite runtime support available for `sqlx` builds
- Optional: `cargo-llvm-cov` for coverage reports

Build the Rust binary:

```sh
cargo build -p opencode
```

Show the merged config for the current project:

```sh
cargo run -p opencode -- config --show
```

Run a built-in tool directly:

```sh
cargo run -p opencode -- tool read \
  --args-json '{"filePath":"Cargo.toml","limit":10}'
```

Start the HTTP server on port `4141`:

```sh
cargo run -p opencode -- server --port 4141
```

Call the health route:

```sh
curl http://127.0.0.1:4141/health
```

## HTTP Surface

Routes currently wired by `opencode-server`:

- `GET /health`
- `GET /api/v1/projects`
- `PUT /api/v1/projects/:id`
- `GET /api/v1/projects/:id`
- `POST /api/v1/projects/:pid/sessions`
- `GET /api/v1/projects/:pid/sessions`
- `GET /api/v1/sessions/:sid`
- `PATCH /api/v1/sessions/:sid`
- `GET /api/v1/sessions/:sid/messages`
- `POST /api/v1/sessions/:sid/messages`
- `POST /api/v1/provider/stream` only when `OPENCODE_MANUAL_HARNESS=1`

The provider stream route is a manual validation endpoint, not a stable public API.

## Configuration

`opencode-core::Config::load` merges configuration in this order:

1. `~/.config/opencode/config.jsonc`
2. `<project>/.opencode/config.jsonc`
3. Environment variables such as `OPENCODE_MODEL`, `OPENCODE_LOG_LEVEL`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, and `OPENCODE_SERVER_PORT`

The binary also creates or reuses `opencode.db` in the current working directory when the server starts.

## Testing

Run the full Rust workspace tests:

```sh
cargo test --workspace
```

Run a narrower validation pass while editing docs or CLI behavior:

```sh
cargo test -p opencode -p opencode-cli -p opencode-server
```

Generate coverage summaries:

```sh
./scripts/coverage.sh
./scripts/coverage.sh --check
```

Manual provider-harness testing steps live in `docs/MANUAL_TESTING.md`.

## Relationship Between Crates

- `opencode` is the runnable binary and depends on the library crates.
- `opencode-cli` parses commands and delegates `tool` execution to `opencode-tool`.
- `opencode-server` exposes HTTP routes over `opencode-storage`, `opencode-provider`, `opencode-session`, and `opencode-bus`.
- `opencode-storage` owns persistence and schema compatibility with the existing SQLite layout.
- `opencode-session` is intended to orchestrate the agent loop, but today it is still a stub.

## More Documentation

- `crates/README.md` for the crate index
- `docs/README.md` for available documentation
- `opencode/README.md` for binary-specific notes
- `scripts/README.md` for helper scripts
