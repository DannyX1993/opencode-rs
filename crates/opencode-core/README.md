# opencode-core

Lowest-level shared contracts for the Rust workspace.

## Role

`opencode-core` defines types and service-facing interfaces that higher crates depend on, without pulling in storage/server/runtime implementation details.

## Project foundation contracts (new in this stream)

This crate now includes canonical repository/worktree foundation contracts in `src/project.rs`:

- `RepositoryProbe` (backend-agnostic inspection seam)
- `ProjectProbeError`
- `ProjectFoundationRecord`
- `WorktreeState`
- `RepositoryState`
- `SyncBasis`

These contracts are intentionally separate from session runtime naming. `RunSnapshot` remains session-runtime language; repository/worktree persistence uses foundation-specific terms.

## Shared DTO boundary

`src/dto.rs` exports persistence/transport-neutral rows used across storage and server layers, including `ProjectFoundationRow` for additive repository/worktree state.

The `v0.14.0` CLI/backend alignment also depends on these DTOs for route-level command contracts (project/session listing and detached prompt acceptance), keeping CLI adapters and server handlers on one shared shape.

Design constraints enforced here:

- unknown fields are representable (`Option<_>`)
- partial state remains serializable and round-trippable
- no bare persisted `snapshot` terminology in project foundation domain

## Why other crates depend on this

- `opencode-storage`: maps SQLite rows to/from shared DTOs
- `opencode-server`: route adapters and foundation probe orchestration
- `opencode-session`: runtime errors/IDs and strict naming boundary tests
- `opencode`: startup/bootstrap composition

## Testing expectations

From workspace root:

```sh
cargo test -p opencode-core
```

Core tests should continue proving:

- serialization of partial/unknown project foundation fields
- naming boundary invariants between project-foundation state and run-state snapshot terms

## Notes

Keep this crate implementation-light and dependency-light. Domain workflows belong in higher layers.
