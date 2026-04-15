# opencode-bus/src

Internal modules for the in-process event bus.

## Modules

- `bus.rs` — `BroadcastBus` and `EventBus` trait implementation.
- `event.rs` — `BusEvent` and `EventKind` definitions, including detached runtime errors and permission/question ask-reply-reject payloads.
- `lib.rs` — crate exports.

## Current notes

- The server SSE layer translates only a supported subset of `BusEvent` variants.
- Permission and question runtime events are currently classified under `EventKind::Permission` for coarse subscription filtering.
- Delivery is in-process and broadcast-based; durable replay is still out of scope.
