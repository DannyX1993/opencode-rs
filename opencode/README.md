# opencode

Binary crate for the Rust `opencode` executable.

## Release

- Current binary/workspace version: **`0.12.0`**

## Purpose

`src/main.rs` stays minimal while `src/lib.rs` contains testable command dispatch and server startup composition.

## Commands

| Command | Status | Behavior |
| --- | --- | --- |
| default / `run` | stub | logs non-implemented TUI mode |
| `server [--host <h>] [--port <n>]` | active | starts HTTP server + SQLite (`./opencode.db`) |
| `prompt <text>` | stub | parsed but not implemented as one-shot CLI flow |
| `version` | active | prints `opencode <version>` |
| `config --show` | active | prints merged runtime config JSON |
| `config` | stub | edit flow not implemented |
| `tool <name>` | active | runs built-in tool execution path |

## Startup composition details

Server startup wires:

- shared `ConfigService`
- SQLite storage (`opencode-storage`)
- runtime bus + SSE heartbeat support
- `SessionEngine` + permission/question runtimes
- provider registry/auth/account services

Bind precedence remains: **CLI args > resolved config > defaults**.

## Foundation/worktree compatibility expectations

Although project repository foundation persistence is initiated in server routes, startup/runtime behavior here remains compatible:

- no new control-plane dependency required
- no startup failure when project foundation metadata is partial/unknown
- runtime prompt/session behavior remains unchanged by additive foundation state

## Testing

From workspace root:

```sh
cargo test -p opencode --lib
```

Required release gates (workspace-wide):

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```
