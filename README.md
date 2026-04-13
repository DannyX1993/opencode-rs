# opencode-rs

Rust workspace for the `opencode` runtime, server surface, and core libraries.

> Scope note: this README documents the Rust workspace in this directory only.
> `opencode-ts/` is intentionally out of scope.

## Project Status

The workspace is **partially production-shaped**: core crates are functional (CLI, storage, provider adapters, HTTP routes, provider/account domain services, and session runtime core), while some user-facing surfaces remain intentionally stubbed.

Current milestone reflected in this repo state: **`port-provider-auth-and-account-parity` (verified, release-prep)**.

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
| `crates/opencode-server/` | library crate | active | Axum router + project/session routes and provider/config/account contracts |
| `crates/opencode-storage/` | library crate | active | SQLite persistence, migrations, repositories, account state, event storage |
| `crates/opencode-tool/` | library crate | active | Tool trait, registry, built-in read/list/glob/grep/write/bash tools |
| `crates/opencode-bus/` | library crate | partial | Typed in-process broadcast bus with published event types |
| `crates/opencode-session/` | library crate | partial | Session runtime core: prompt lifecycle, run-state exclusivity, cancellation |
| `crates/opencode-lsp/` | library crate | stub | Placeholder for future LSP integration |
| `crates/opencode-mcp/` | library crate | stub | Placeholder for future MCP integration |
| `crates/opencode-plugin/` | library crate | stub | Placeholder for future plugin host |
| `crates/opencode-tui/` | library crate | stub | Placeholder for future terminal UI |
| `docs/` | docs | active | Manual testing + architecture/runtime notes |
| `scripts/` | scripts | active | Helper scripts (coverage, etc.) |

## Provider/auth/account parity completed in this change stream

The `port-provider-auth-and-account-parity` slice is now landed in code and tests:

- Public provider surface now includes catalog, auth methods, OAuth authorize/callback, account-state reads, active-account switching, account removal, and `/api/v1/config/providers`.
- `ProviderCatalogService`, `ProviderAuthService`, and `AccountService` are explicit domain services; Axum handlers remain thin adapters.
- Account persistence and active state reuse existing SQLite tables (`account`, `account_state`, `control_account`) with no schema migration.
- Startup overlays provider catalog models from `.opencode/models.json` cache when present; built-in provider defaults remain fallback.

## Runtime/session scope retained from prior stream

- `SessionEngine::prompt` rebuilds provider requests from persisted session history.
- Anthropic and Google session turns can complete provider-driven built-in tool loops (`provider -> tool -> provider -> done`).
- Assistant `tool_use` parts and `tool_result` messages are persisted for replay.
- Tool lifecycle events are published on `opencode-bus` (`ToolStarted`, `ToolFinished { ok }`).

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
- OAuth flow for manual checks is two-step: authorize endpoint first, callback endpoint second.
- Verify persistence behavior by checking `GET /api/v1/provider/account` before and after `use`/`delete` calls.
- Use `/api/v1/provider/stream` only for raw SSE adapter validation with `OPENCODE_MANUAL_HARNESS=1`.

## Deferred scope / known caveats

- `run`, `prompt <text>`, and `config` (without `--show`) CLI commands remain stubs.
- Runtime tool loop is intentionally bounded: Anthropic/Google supported for tool-capable session turns; OpenAI is still text-only there.
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

- Workspace version is currently `0.8.0` (`[workspace.package]`).
- Crates use `version.workspace = true`, so crate versions are kept in lockstep.
- Git tag style remains `v<semver>` (this release: `v0.8.0`).
- Until `1.0`, API and behavior may change between minor releases.

## More documentation

- [`crates/README.md`](crates/README.md) — crate index and status
- [`opencode/README.md`](opencode/README.md) — binary-specific runtime notes
- [`docs/README.md`](docs/README.md) — docs index
- [`scripts/README.md`](scripts/README.md) — helper scripts
