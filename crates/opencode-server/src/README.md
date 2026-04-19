# opencode-server/src

Internal module layout for the Axum server crate.

## Key modules

- `router.rs` — registers `/health`, `/api/v1/event`, legacy plural routes, singular `/api/v1/session/*` parity aliases, and `/api/v1/permission|question` runtime routes.
- `control_plane/` — workspace selector resolution, route policy, local/forward decisioning, proxy transport, and observability counters.
- `routes/` — HTTP handlers split by domain (`project`, `session`, `permission`, `question`, `provider`, `config`, `event`, `workspace`).
- `state.rs` — shared `AppState`, including SSE heartbeat and control-plane runtime config/proxy policy.
- `error.rs` — HTTP/server error mapping.

## Current notes

- `event.rs` in `routes/` owns live SSE translation and heartbeat behavior.
- Session parity handlers intentionally wrap only runtime/storage behavior already backed elsewhere in the workspace.
- Permission/question routes expose runtime pending queues and `ok: bool` reply contracts without introducing additional persistence tables.
- Control-plane middleware runs in front of `/api/v1` and only forwards routes marked eligible by policy.
- Forwarding strips hop-by-hop headers and injects explicit `x-opencode-forwarded-*` context headers for traceability.
