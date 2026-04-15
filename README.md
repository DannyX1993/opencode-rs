# opencode-rs

Rust workspace for the `opencode` runtime, server surface, and core libraries.

> Scope note: this README documents the Rust workspace in this directory only.
> `opencode-ts/` is intentionally out of scope.

## Project Status

The workspace is **partially production-shaped**: core crates are functional (CLI, storage, provider adapters, HTTP routes, provider/account domain services, and session runtime core), while some user-facing surfaces remain intentionally stubbed.

Current milestone reflected in this repo state: **`port-permission-and-question-runtime` (implemented + docs updated)**.

## Workspace Architecture (high level)

```text
opencode (binary)
  └─ opencode-cli (command parsing/bootstrap)
      ├─ opencode-server (HTTP routes)
      │   ├─ opencode-session (prompt/cancel runtime core)
      │   ├─ opencode-storage (SQLite repositories + migrations)
      │   ├─ opencode-provider (runtime adapters + catalog/auth/account services)
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
| `crates/opencode-provider/` | library crate | active | Runtime model adapters plus provider catalog/auth/account services |
| `crates/opencode-server/` | library crate | active | Axum router + project/session routes, permission/question runtime routes, SSE event stream, and provider/config/account contracts |
| `crates/opencode-storage/` | library crate | active | SQLite persistence, migrations, repositories, account state, event storage |
| `crates/opencode-tool/` | library crate | active | Tool trait, registry, built-in read/list/glob/grep/write/bash tools |
| `crates/opencode-bus/` | library crate | partial | Typed in-process broadcast bus with lifecycle/runtime events, including permission/question ask/reply/reject |
| `crates/opencode-session/` | library crate | partial | Session runtime core: prompt lifecycle, permission/question interactive runtimes, blocked status, detached execution, cancellation |
| `crates/opencode-lsp/` | library crate | stub | Placeholder for future LSP integration |
| `crates/opencode-mcp/` | library crate | stub | Placeholder for future MCP integration |
| `crates/opencode-plugin/` | library crate | stub | Placeholder for future plugin host |
| `crates/opencode-tui/` | library crate | stub | Placeholder for future terminal UI |
| `docs/` | docs | active | Manual testing + architecture/runtime notes |
| `scripts/` | scripts | active | Helper scripts (coverage, etc.) |

## Permission/question runtime parity completed in this change stream

The `port-permission-and-question-runtime` slice is now landed in code, docs, and tests:

- Session runtime now includes permission and question runtimes with explicit ask/reply/reject lifecycle and in-memory pending queues.
- Public HTTP routes now include `/api/v1/permission*` and `/api/v1/question*` for listing and resolving pending runtime prompts.
- Runtime status now supports blocked shapes: `{ "type": "blocked", "kind": "permission|question", "requestID": "..." }`.
- Public SSE route `GET /api/v1/event` now translates `permission.asked`, `permission.replied`, `question.asked`, `question.replied`, and `question.rejected` in addition to existing lifecycle/tool events.
- Durable allow-always semantics are implemented by merging normalized `allow` rules into project permission storage.

## Provider/auth/account parity retained from the prior stream

The `port-provider-auth-and-account-parity` slice is now landed in code and tests:

- Public provider surface now includes catalog, auth methods, OAuth authorize/callback, account-state reads, active-account switching, account removal, and `/api/v1/config/providers`.
- `ProviderCatalogService`, `ProviderAuthService`, and `AccountService` are explicit domain services; Axum handlers remain thin adapters.
- Account persistence and active state reuse existing SQLite tables (`account`, `account_state`, `control_account`) with no schema migration.
- Startup overlays provider catalog models from `.opencode/models.json` cache when present; built-in provider defaults remain fallback.

## Runtime/session scope retained and extended

- `SessionEngine::prompt` rebuilds provider requests from persisted session history.
- Anthropic and Google session turns can complete provider-driven built-in tool loops (`provider -> tool -> provider -> done`).
- Assistant `tool_use` parts and `tool_result` messages are persisted for replay.
- Tool lifecycle events are published on `opencode-bus` (`ToolStarted`, `ToolFinished { ok }`).
- Session runtime status is queryable through singular parity routes and now includes blocked states for permission/question waits.
- Detached prompt requests return acceptance metadata immediately while background execution continues.

Detailed runtime notes: [`docs/SESSION_RUNTIME.md`](docs/SESSION_RUNTIME.md).

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
- `GET /api/v1/session/status`
- `GET /api/v1/session/:sid/status`
- `POST /api/v1/session/:sid/abort`
- `GET /api/v1/session/:sid/message`
- `POST /api/v1/session/:sid/prompt`
- `GET /api/v1/permission`
- `POST /api/v1/permission/reply`
- `GET /api/v1/question`
- `POST /api/v1/question/reply`
- `POST /api/v1/question/reject`
- `GET /api/v1/event`
- `GET /api/v1/provider`
- `GET /api/v1/provider/auth`
- `POST /api/v1/provider/:provider/oauth/authorize`
- `POST /api/v1/provider/:provider/oauth/callback`
- `GET /api/v1/provider/account`
- `POST /api/v1/provider/account/use`
- `DELETE /api/v1/provider/account/:account_id`
- `GET /api/v1/config/providers`
- `POST /api/v1/provider/stream` (manual harness only)

## Manual validation expectations

- Use provider parity routes without harness flag for catalog/auth/account checks.
- Use `GET /api/v1/event` for runtime SSE checks; expect a `server.connected` frame first and `server.heartbeat` while idle.
- Use singular `/api/v1/session/*` aliases for upstream-style clients; status can return `idle`, `busy`, or blocked runtime objects.
- Validate permission/question flows through `/api/v1/permission*` and `/api/v1/question*` and confirm `ok: true|false` reply contracts.
- Validate durable allow-always behavior by approving with `always`, then re-triggering the same permission pattern and confirming it no longer appears in pending lists.
- OAuth flow for manual checks is two-step: authorize endpoint first, callback endpoint second.
- Verify persistence behavior by checking `GET /api/v1/provider/account` before and after `use`/`delete` calls.
- Use `/api/v1/provider/stream` only for raw SSE adapter validation with `OPENCODE_MANUAL_HARNESS=1`.

## Deferred scope / known caveats

- `run`, `prompt <text>`, and `config` (without `--show`) CLI commands remain stubs.
- Runtime tool loop is intentionally bounded: Anthropic/Google supported for tool-capable session turns; OpenAI is still text-only there.
- `/api/v1/event` is live-only SSE in this release; it does not replay persisted history.
- Singular session parity stays intentionally narrow: unsupported write parity like `POST /api/v1/session/:sid/message` is still not exposed.
- OAuth pending authorization state is in-process only; server restart during auth requires re-authorize.
- `opencode-lsp`, `opencode-mcp`, `opencode-plugin`, and `opencode-tui` remain scaffolding crates.
- `/api/v1/provider/stream` is a **manual harness**, not a stable public API contract.

## Development and testing

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```

Manual endpoint guide: [`docs/MANUAL_TESTING.md`](docs/MANUAL_TESTING.md)

## Versioning

- Workspace version is currently `0.10.0` (`[workspace.package]`).
- Crates use `version.workspace = true`, so crate versions are kept in lockstep.
- Git tag style remains `v<semver>` (this release target: `v0.10.0`).
- Until `1.0`, API and behavior may change between minor releases.

## More documentation

- [`crates/README.md`](crates/README.md) — crate index and status
- [`opencode/README.md`](opencode/README.md) — binary-specific runtime notes
- [`docs/README.md`](docs/README.md) — docs index
- [`scripts/README.md`](scripts/README.md) — helper scripts
