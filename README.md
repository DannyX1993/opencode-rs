# opencode-rs

Rust workspace for the `opencode` runtime, server surface, and core libraries.

> Scope note: this README documents the Rust workspace in this directory only.
> `opencode-ts/` is intentionally out of scope.

## Project Status

The workspace is **partially production-shaped**: core crates are functional (CLI, storage, provider adapters, HTTP routes, and session runtime core), while some user-facing surfaces remain intentionally stubbed.

Recent milestone reflected in this repo state: **`expand-tool-runtime-parity` (verified, archive-ready)**.

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

The bounded `expand-tool-runtime-parity` slice is now landed in code and tests:

- `SessionEngine::prompt` rebuilds provider requests from persisted session history instead of treating each pass as user-text-only.
- Anthropic and Google session turns can now complete provider-driven built-in tool loops in Rust (`provider -> tool -> provider -> done`).
- Assistant `tool_use` parts and `tool_result` messages are persisted so the next provider pass replays the same turn from storage.
- Tool lifecycle events are published on `opencode-bus` (`ToolStarted`, `ToolFinished { ok }`) alongside existing session/message events.
- Per-session run exclusivity is still enforced via `RunState`, and cancellation only releases the lease after the active task unwinds.
- HTTP endpoints `POST /api/v1/sessions/:sid/prompt` and `POST /api/v1/sessions/:sid/cancel` are wired through `opencode-server` for manual end-to-end validation.
- Google/Gemini compatibility fixes now included in this state: recursive removal of `additionalProperties` from tool parameter schemas, `thoughtSignature` replay on the enclosing `Part`, and `functionResponse` replay under Google-compatible `user` role.

Detailed runtime notes: [`docs/SESSION_RUNTIME.md`](docs/SESSION_RUNTIME.md).

## What works today

- `cargo run -p opencode -- version`
- `cargo run -p opencode -- config --show`
- `cargo run -p opencode -- tool ...`
- `cargo run -p opencode -- server --port 4141`
- Session REST routes + prompt/cancel runtime endpoints via `opencode-server`
- Provider-driven built-in tool execution for Anthropic/Google session prompts
- SQLite bootstrap from current working directory (`./opencode.db`)

## Deferred scope / known caveats

- `run` command still logs a stub message (TUI not implemented).
- `prompt <text>` CLI command is still a stub (server/session runtime is where prompt flow currently lives).
- `config` without `--show` is still a stub.
- The runtime tool loop is intentionally bounded: Anthropic and Google are supported; OpenAI remains text-only for session prompts and is rejected on tool-capable turns rather than pretending parity.
- Permission/approval UX, question flows, task/subagent orchestration, broader TypeScript tool-catalog parity, and new SSE/session-frame contracts are still deferred.
- `opencode-lsp`, `opencode-mcp`, `opencode-plugin`, and `opencode-tui` remain scaffolding crates.
- `/api/v1/provider/stream` is a **manual harness**, not a stable public contract; it is disabled unless `OPENCODE_MANUAL_HARNESS=1`.

## Tool execution modes: session flow vs standalone CLI

- `cargo run -p opencode -- tool ...` executes one built-in tool directly from the CLI. No model, no session history replay, no provider loop.
- `POST /api/v1/sessions/:sid/prompt` runs the session runtime. The model may request a supported built-in tool, the runtime executes it, persists the artifacts, and re-enters the same turn.
- The current Rust workspace does **not** claim full TypeScript runtime parity; this change only covers the bounded provider-driven session tool loop above.

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

- Workspace version is currently `0.7.0` (`[workspace.package]`).
- Crates use `version.workspace = true`, so crate versions are kept in lockstep.
- Until `1.0`, API and behavior can change between minor releases as runtime parity work continues.

## More documentation

- [`crates/README.md`](crates/README.md) — crate index and status
- [`opencode/README.md`](opencode/README.md) — binary-specific runtime notes
- [`docs/README.md`](docs/README.md) — docs index
- [`scripts/README.md`](scripts/README.md) — helper scripts
