# opencode-bus

Typed in-process event bus for the Rust workspace.

## Status

Partial. The crate has a real `tokio::broadcast`-backed bus and a concrete `BusEvent` enum, including detached-runtime failure signaling, but higher-level routing and richer consumers are still to come.

## What Exists Today

- `EventBus` trait
- `BroadcastBus` implementation
- `BusEvent` variants for session, message, tool, provider, permission, todo, and config events
- `SessionError` events that let detached runtime failures reach the server/SSE layer
- `EventKind` for coarse-grained filtering

## What Does Not Exist Yet

- a separate filtered channel implementation per event kind
- durable replay/history for bus consumers
- finer-grained filtered channels beyond the current coarse event-kind split

## Usage

```rust
use opencode_bus::{BroadcastBus, EventBus};

let bus = BroadcastBus::new(64);
let mut rx = bus.subscribe();
```

`publish` returns an error when no receivers are subscribed. The current implementation treats that as a non-fatal condition.

## Test

```sh
cargo test -p opencode-bus
```

## Workspace Role

`opencode-server` and future session orchestration code depend on this crate for event fan-out inside the process; the current server SSE endpoint translates a supported subset into stable wire events.
