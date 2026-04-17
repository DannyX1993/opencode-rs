# opencode/src

Implementation modules for the `opencode` binary crate.

## Modules

- `main.rs` — thin process entrypoint
- `lib.rs` — command dispatch + server bootstrap + startup/runtime tests

## Version note

- `version` command reflects workspace release **`0.12.0`**

## Startup/testing notes

- Startup tests use readiness polling (`wait_for_server_ready`) instead of fixed sleeps.
- Startup/test behavior is expected to remain stable even when project foundation metadata is present/partial/unknown.
- `lib.rs` composes state from `opencode-server`, `opencode-storage`, `opencode-session`, and provider services.
