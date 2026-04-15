# opencode/src

Binary implementation modules for the `opencode` executable.

## Modules

- `main.rs` — thin process entrypoint.
- `lib.rs` — command dispatch and server bootstrap used by tests.

## Current notes

- `lib.rs` wires `SessionEngine`, provider services, storage, the event bus, and the default SSE heartbeat driver into `opencode-server::AppState`.
- The `version` command reflects the workspace release version (`0.10.0` in this batch).
