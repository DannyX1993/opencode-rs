# opencode-rs

Rust workspace for the `opencode` runtime, server surface, and core libraries.

> Scope note: this README documents the Rust workspace in this directory only.
> `opencode-ts/` is intentionally out of scope.

## Project Status

The workspace is **partially production-shaped**: core crates are functional (CLI, storage, provider adapters, HTTP routes, and session runtime core), while some user-facing surfaces remain intentionally stubbed.

Recent milestone reflected in this repo state: **`port-session-runtime-core` (in progress, substantial runtime landed)**.

## Workspace Architecture (high level)

```text
opencode (binary)
  └─ opencode-cli (command parsing/bootstrap)
      ├─ opencode-server (HTTP routes)
      │   ├─ opencode-session (prompt/cancel runtime core)
      │   ├─ opencode-storage (SQLite repositories + migrations)
      │   ├─ opencode-provider (LLM registry + adapters)
      │   └─ opencode-bus (in-process event fan-out)
      └─ opencode-tool (tool runtime + built-ins)

opencode-core provides shared DTOs/config/errors/IDs across all crates.
```

## Workspace Layout

| Path | Type | Status | Responsibility |
| --- | --- | --- | --- |
| `opencode/` | binary crate | active | Runtime entrypoint and command dispatch |
| `crates/opencode-cli/` | library crate | active | Clap definitions, bootstrap, `tool` command wiring |
| `crates/opencode-core/` | library crate | active | Shared config, DTOs, IDs, errors, tracing/context helpers |
| `crates/opencode-provider/` | library crate | active | Provider trait, registry, OpenAI/Anthropic/Google adapters |
| `crates/opencode-server/` | library crate | active | Axum router + project/session/message/prompt/cancel endpoints |
| `crates/opencode-storage/` | library crate | active | SQLite persistence, migrations, repositories, event storage |
| `crates/opencode-tool/` | library crate | active | Tool trait, registry, built-in read/list/glob/grep/write/bash tools |
| `crates/opencode-bus/` | library crate | partial | Typed in-process broadcast bus with published event types |
| `crates/opencode-session/` | library crate | partial | Session runtime core: prompt lifecycle, run-state exclusivity, cancellation |
| `crates/opencode-lsp/` | library crate | stub | Placeholder for future LSP integration |
| `crates/opencode-mcp/` | library crate | stub | Placeholder for future MCP integration |
| `crates/opencode-plugin/` | library crate | stub | Placeholder for future plugin host |
| `crates/opencode-tui/` | library crate | stub | Placeholder for future terminal UI |
| `docs/` | docs | active | Manual testing + architecture/runtime notes |
| `scripts/` | scripts | active | Helper scripts (coverage, etc.) |

## Runtime/session work completed in this change stream

The `port-session-runtime-core` slice is now visible in code and tests:

- `SessionEngine::prompt` validates session existence, resolves model/provider, persists user + assistant shell messages, and streams provider output.
- Per-session run exclusivity is enforced via `RunState` (`Busy` on concurrent prompt for same session).
- `cancel(session_id)` is wired and returns `NoActiveRun` when appropriate.
- Provider `TextDelta` events are persisted incrementally as assistant parts.
- Lifecycle events are published on `opencode-bus` (`SessionUpdated`, `PartAdded`, `SessionCompleted`, `SessionCancelled`, token usage).
- HTTP endpoints `POST /api/v1/sessions/:sid/prompt` and `POST /api/v1/sessions/:sid/cancel` are wired through `opencode-server`.

Detailed runtime notes: [`docs/SESSION_RUNTIME.md`](docs/SESSION_RUNTIME.md).

## What works today

- `cargo run -p opencode -- version`
- `cargo run -p opencode -- config --show`
- `cargo run -p opencode -- tool ...`
- `cargo run -p opencode -- server --port 4141`
- Session REST routes + prompt/cancel runtime endpoints via `opencode-server`
- SQLite bootstrap from current working directory (`./opencode.db`)

## Deferred scope / known caveats

- `run` command still logs a stub message (TUI not implemented).
- `prompt <text>` CLI command is still a stub (server/session runtime is where prompt flow currently lives).
- `config` without `--show` is still a stub.
- Tool-use model events in session streaming are intentionally deferred and currently return runtime error for unsupported tool-use stream events.
- `opencode-lsp`, `opencode-mcp`, `opencode-plugin`, and `opencode-tui` remain scaffolding crates.
- `/api/v1/provider/stream` is a **manual harness**, not a stable public contract; it is disabled unless `OPENCODE_MANUAL_HARNESS=1`.

## Quick start

Prerequisites:

- Rust `1.85`+
- SQLite runtime support for `sqlx`
- Optional: `cargo-llvm-cov`

Build:

```sh
cargo build -p opencode
```

Run server:

```sh
cargo run -p opencode -- server --port 4141
```

Health check:

```sh
curl http://127.0.0.1:4141/health
```

## HTTP surface (currently wired)

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
- `POST /api/v1/sessions/:sid/prompt`
- `POST /api/v1/sessions/:sid/cancel`
- `POST /api/v1/provider/stream` (manual harness only)

## Development and testing

Workspace test pass:

```sh
cargo test --workspace
```

Focused pass for runtime/server iteration:

```sh
cargo test -p opencode-session -p opencode-server -p opencode --lib
```

Coverage helper:

```sh
./scripts/coverage.sh
./scripts/coverage.sh --check
```

Manual provider harness test plan: [`docs/MANUAL_TESTING.md`](docs/MANUAL_TESTING.md)

## Versioning

- Workspace version is currently `0.6.0` (`[workspace.package]`).
- Crates use `version.workspace = true`, so crate versions are kept in lockstep.
- Until `1.0`, API and behavior can change between minor releases as runtime parity work continues.

## More documentation

- [`crates/README.md`](crates/README.md) — crate index and status
- [`opencode/README.md`](opencode/README.md) — binary-specific runtime notes
- [`docs/README.md`](docs/README.md) — docs index
- [`scripts/README.md`](scripts/README.md) — helper scripts
