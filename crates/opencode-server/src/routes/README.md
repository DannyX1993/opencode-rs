# opencode-server/src/routes

Route handlers for the Rust HTTP surface.

## Modules

- `project.rs` — storage-backed project CRUD and project-scoped session creation/listing.
- `session.rs` — legacy session routes plus singular parity aliases for status, abort, message reads, and detached prompt acceptance.
- `event.rs` — `/api/v1/event` SSE endpoint, heartbeat handling, and bus-event translation.
- `provider.rs` — provider catalog/auth/account routes and the manual provider stream harness.
- `config.rs` — provider-config projection routes.

## Scope notes

- `event.rs` exposes only live translated events; unsupported/internal bus variants are filtered out.
- `session.rs` keeps unsupported write parity routes unregistered.
