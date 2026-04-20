# opencode

Binary crate for the Rust `opencode` executable.

## Release

- Current binary/workspace version: **`0.14.0`**

## Purpose

`src/main.rs` stays minimal while `src/lib.rs` contains testable command dispatch and server startup composition.

## Commands

| Command | Status | Behavior |
| --- | --- | --- |
| default / `run` | partial | no args: logs TUI stub; with `<text...>` executes one-shot detached prompt acceptance flow |
| `serve [--host <h>] [--port <n>]` (`server` alias) | active | starts HTTP server + SQLite (`./opencode.db`) with bind precedence `CLI > resolved config > defaults` |
| `prompt <text> [--output text|json] [--timeout-ms <n>]` | active | one-shot detached prompt acceptance flow with deterministic result payload |
| `version` | active | prints `opencode <version>` |
| `config --show` | active | prints merged runtime config JSON |
| `config` | stub | edit flow not implemented |
| `tool <name>` | active | runs built-in tool execution path |
| `providers list [--output text|json]` | active | lists provider catalog from backend-aligned route contracts |
| `session list` | active | resolves project from cwd and lists sessions deterministically |

## Deterministic CLI semantics

Core scriptable commands (`serve`, `providers list`, `session list`, non-interactive `run`, and `prompt`) follow a predictable contract:

- stdout contains only command payloads intended for piping/parsing
- stderr contains actionable diagnostics
- `0` indicates success, `1` indicates runtime/backend failure, and `2` indicates CLI input validation failure

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
