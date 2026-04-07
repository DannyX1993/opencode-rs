# opencode-session

Session engine for `opencode-rs` — manages conversation state, turn lifecycle,
and agent loop orchestration.

> **Status**: 🔲 Stub — `SessionEngine` struct defined; conversation logic planned for Phase 5.

---

## Purpose

`opencode-session` will own the full agent loop: receiving user messages,
dispatching tool calls via `opencode-tool`, streaming partial results back
through `opencode-bus`, and persisting session data via `opencode-storage`.

---

## Current State

The `SessionEngine` struct is wired into `AppState` and the HTTP server but
its methods are not yet implemented. A placeholder is returned for all session
operations.

Full implementation arrives in Phase 5 (agent loop phase).
