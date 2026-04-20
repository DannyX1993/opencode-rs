# opencode/src

Implementation modules for the `opencode` binary crate.

## Modules

- `main.rs` — thin process entrypoint
- `lib.rs` — command dispatch + server bootstrap + startup/runtime tests

## Version note

- `version` command reflects workspace release **`0.14.0`**

## Startup/testing notes

- Startup tests use readiness polling (`wait_for_server_ready`) instead of fixed sleeps.
- Startup/test behavior is expected to remain stable even when workspace control-plane routing is enabled.
- `lib.rs` composes state from `opencode-server`, `opencode-storage`, `opencode-session`, and provider services.
- `run_command` now keeps backend-aligned deterministic semantics for `serve`, `providers list`, `session list`, and non-interactive `run`/`prompt` paths.
