# opencode-bus/src

Internal modules for the in-process event bus.

## Modules

- `bus.rs` — `BroadcastBus` and `EventBus` trait implementation.
- `event.rs` — `BusEvent` and `EventKind` definitions, including `SessionError` for detached runtime failures.
- `lib.rs` — crate exports.

## Current notes

- The server SSE layer translates only a supported subset of `BusEvent` variants.
- Delivery is in-process and broadcast-based; durable replay is still out of scope.
