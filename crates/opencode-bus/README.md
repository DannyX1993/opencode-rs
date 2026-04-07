# opencode-bus

Broadcast event bus for `opencode-rs`.

> **Status**: ✅ Stub — `BroadcastBus` struct implemented; event routing logic pending.

---

## Purpose

`opencode-bus` provides a tokio-broadcast-backed event bus used to fan out
agent events (tool calls, streaming chunks, session state changes) to multiple
consumers (SSE clients, TUI, logger).

```rust
use opencode_bus::BroadcastBus;

let bus = BroadcastBus::new(64); // channel capacity
```

The bus is wired into `AppState` and passed to the HTTP server.
Full publish/subscribe API arrives in Phase 5.
