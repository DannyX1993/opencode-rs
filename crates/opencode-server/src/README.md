# opencode-server/src

Internal module layout for the Axum server crate.

## Key modules

- `router.rs` — registers `/health`, `/api/v1/event`, legacy plural routes, and singular `/api/v1/session/*` parity aliases.
- `routes/` — HTTP handlers split by domain (`project`, `session`, `provider`, `config`, `event`).
- `state.rs` — shared `AppState`, including the narrow `EventHeartbeat` hook used for deterministic SSE tests.
- `error.rs` — HTTP/server error mapping.

## Current notes

- `event.rs` in `routes/` owns live SSE translation and heartbeat behavior.
- Session parity handlers intentionally wrap only runtime/storage behavior already backed elsewhere in the workspace.
