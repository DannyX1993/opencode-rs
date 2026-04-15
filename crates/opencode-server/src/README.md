# opencode-server/src

Internal module layout for the Axum server crate.

## Key modules

- `router.rs` — registers `/health`, `/api/v1/event`, legacy plural routes, singular `/api/v1/session/*` parity aliases, and `/api/v1/permission|question` runtime routes.
- `routes/` — HTTP handlers split by domain (`project`, `session`, `permission`, `question`, `provider`, `config`, `event`).
- `state.rs` — shared `AppState`, including the narrow `EventHeartbeat` hook used for deterministic SSE tests.
- `error.rs` — HTTP/server error mapping.

## Current notes

- `event.rs` in `routes/` owns live SSE translation and heartbeat behavior.
- Session parity handlers intentionally wrap only runtime/storage behavior already backed elsewhere in the workspace.
- Permission/question routes expose runtime pending queues and `ok: bool` reply contracts without introducing additional persistence tables.
