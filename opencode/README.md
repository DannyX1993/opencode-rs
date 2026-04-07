# opencode (binary)

The `opencode` binary crate — thin entrypoint and command dispatcher.

> **Status**: ✅ Active — builds and runs. Dispatches all CLI subcommands.
> TUI and full agent loop are planned for later phases.

---

## Structure

| File          | Role                                                                   |
| ------------- | ---------------------------------------------------------------------- |
| `src/main.rs` | Binary entry point — parses CLI args, runs bootstrap, calls `dispatch` |
| `src/lib.rs`  | Testable dispatch logic + `start_server` helper                        |

`main.rs` is intentionally minimal (≤15 lines). All branching lives in `lib.rs`
so every command path can be exercised by unit tests without spawning a process.

---

## Subcommands

| Subcommand       | Status     | Description                                |
| ---------------- | ---------- | ------------------------------------------ |
| _(none)_ / `run` | 🔲 Stub    | Interactive TUI — logs a stub message      |
| `server`         | ✅ Working | Start Axum HTTP API on the configured port |
| `prompt <text>`  | 🔲 Stub    | One-shot prompt — logs a stub message      |
| `version`        | ✅ Working | Print binary version                       |
| `config --show`  | ✅ Working | Print merged config as JSON                |
| `tool <NAME>`    | ✅ Working | Invoke a built-in tool (see root README)   |

---

## Exit Codes

| Code | Meaning                                          |
| ---- | ------------------------------------------------ |
| 0    | Successful                                       |
| 1    | Runtime error (tool failure, config error, etc.) |
| 2    | CLI parse error (missing arg, unknown flag)      |

---

## Development

Build and run from the workspace root:

```sh
cargo build -p opencode
cargo run -p opencode -- tool bash --args-json '{"command":"echo hi","description":"test"}'
```

Tests live in `src/lib.rs` and cover all dispatch branches:

```sh
cargo test -p opencode --lib
```
