# crates/

Rust library crates used by the `opencode-rs` workspace.

## Scope

This directory contains reusable Rust crates only. The runnable binary lives in `../opencode/`.

## Crate Index

| Crate | Status | Purpose |
| --- | --- | --- |
| [`opencode-bus`](opencode-bus) | partial | Typed broadcast bus and shared event enums, including permission/question runtime events and session failures |
| [`opencode-cli`](opencode-cli) | active | Clap CLI types, bootstrap flow, `tool` command dispatch |
| [`opencode-core`](opencode-core) | active | Shared DTOs, config, IDs, errors, tracing |
| [`opencode-lsp`](opencode-lsp) | stub | Placeholder for future LSP integration |
| [`opencode-mcp`](opencode-mcp) | stub | Placeholder for future MCP integration |
| [`opencode-plugin`](opencode-plugin) | stub | Placeholder for future plugin hosting |
| [`opencode-provider`](opencode-provider) | active | Runtime adapters plus provider catalog/auth/account domain services |
| [`opencode-server`](opencode-server) | active | Axum router and HTTP endpoints, including SSE, session/provider parity routes, and permission/question runtime APIs |
| [`opencode-session`](opencode-session) | partial | Bounded session runtime loop (`prompt`, `prompt_detached`, `status`, `cancel`) plus permission/question runtimes and blocked status tracking |
| [`opencode-storage`](opencode-storage) | active | SQLite persistence and repositories including account active-state support |
| [`opencode-tool`](opencode-tool) | active | Tool runtime, built-in file/shell tools, and provider-facing tool metadata |
| [`opencode-tui`](opencode-tui) | stub | Placeholder for future terminal UI |

## Status Notes

- `active` means the crate has real code used by the current binary or tests.
- `partial` means part of the intended surface exists, but not all planned behavior is implemented yet.
- `stub` means the crate mostly exists to reserve the package boundary and public API direction.

Current notable `partial` crate details:

- `opencode-session` now includes bounded Anthropic/Google tool-loop execution, detached prompt acceptance, permission/question ask-reply runtimes, and blocked runtime status snapshots.
- `opencode-bus` now includes `SessionError` plus permission/question ask/reply/reject events used by the SSE surface.

Current notable `active` crate updates in `v0.12.0`:

- `opencode-provider` now owns provider metadata catalog filtering, auth-method discovery, and account-domain composition.
- `opencode-server` now exposes public provider/account/config contracts (`/api/v1/provider*`, `/api/v1/config/providers`) alongside the manual stream harness.
- `opencode-server` also exposes `GET /api/v1/event` plus singular `/api/v1/session/*` aliases for status, abort, message reads, and detached prompt parity.
- `opencode-server` also exposes `/api/v1/permission*` and `/api/v1/question*` routes for interactive runtime gating.
- `opencode-storage` now exposes richer account/account_state helpers used by provider/account services.
- `opencode-core` / `opencode-storage` / `opencode-server` now include additive project repository foundation contracts and persistence seams for git/non-git worktree state.

## Build And Test

Build all crates from the workspace root:

```sh
cargo build --workspace
```

Test all crates from the workspace root:

```sh
cargo test --workspace
```

Target a single crate while iterating:

```sh
cargo test -p opencode-storage
```

## Relationship To The Rest Of The Workspace

- `../opencode/` turns these crates into the runnable `opencode` binary.
- `../docs/` documents testing and workspace behavior.
- `../scripts/` contains helper scripts for quality checks such as coverage.

Each immediate crate directory should have its own README describing its current state and limitations.
