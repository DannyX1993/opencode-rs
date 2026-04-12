# opencode

Binary crate for the Rust `opencode` executable.

## Purpose

This crate is the runnable entrypoint for the workspace. It keeps `src/main.rs` minimal and puts command dispatch in `src/lib.rs` so behavior can be tested without spawning a separate process.

## Current Commands

| Command | Status | Current behavior |
| --- | --- | --- |
| default / `run` | stub | Logs that TUI mode is not implemented yet |
| `server --port <n>` | active | Starts the Axum HTTP server and initializes SQLite storage |
| `prompt <text>` | stub | Logs that one-shot prompt mode is not implemented yet |
| `version` | active | Prints `opencode <version>` |
| `config --show` | active | Loads merged config and prints it as JSON |
| `config` | stub | Logs that config editing is not implemented yet |
| `tool <name>` | active | Delegates to `opencode-cli::tool_cmd::run` |

## Runtime Behavior

- The current working directory is used as the project root.
- Starting the server creates or opens `./opencode.db`.
- `OPENCODE_MANUAL_HARNESS=1` enables the manual provider streaming route.
- Standard providers are registered for the harness only when that environment variable is set.
- Server startup wires `SessionEngine` from `opencode-session`, so session prompt/cancel APIs are runtime-backed.

## Session Runtime Surface

Even though the CLI `prompt` command is still stubbed, server mode now exposes runtime-core endpoints:

- `POST /api/v1/sessions/:sid/prompt`
- `POST /api/v1/sessions/:sid/cancel`

These routes are served by `opencode-server` and call into `opencode-session::engine::SessionEngine`.

## Run

From the workspace root:

```sh
cargo run -p opencode -- version
cargo run -p opencode -- config --show
cargo run -p opencode -- tool bash --args-json '{"command":"pwd","description":"print cwd"}'
```

Start the server:

```sh
cargo run -p opencode -- server --port 4141
```

## Test

```sh
cargo test -p opencode --lib
```

The library tests cover dispatch branches and a basic server startup path.
