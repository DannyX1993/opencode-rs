# opencode

Binary crate for the Rust `opencode` executable.

## Release

- Current binary/workspace version: **`0.13.0`**

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

## Control-plane compatibility expectations

Although workspace control-plane routing is initiated in server middleware, startup/runtime behavior here remains compatible:

- no new control-plane dependency required
- no startup failure when workspace metadata is absent for local-only flows
- runtime prompt/session behavior remains unchanged by control-plane routing enablement
- rollback remains available via control-plane `force_local_only` config

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
