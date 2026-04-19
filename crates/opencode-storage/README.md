# opencode-storage

SQLite-backed persistence boundary for the Rust workspace.

## Role

`opencode-storage` owns migrations, repository mappers, and the `Storage` trait used by server/session/provider runtime layers.

## Workspace persistence used by control-plane

This change extends workspace persistence to support control-plane routing decisions.

### Workspace repository

- `repo/workspace.rs` now provides full CRUD over the `workspace` table.
- `extra` metadata is serialized/deserialized as JSON (`Option<serde_json::Value>`).
- `type=remote` payload validation is enforced in `opencode-server` before writes.

### Why this matters for routing

Control-plane resolution in `opencode-server` reads workspace rows and expects stable metadata:

- workspace id (`WorkspaceId`) is the selector target
- remote instance identity from `extra.instance`
- remote base URL from `extra.base_url`

Storage does not infer or fabricate these values; it only persists/retrieves canonical payloads.

## Boundaries

- This crate persists data; it does not run route policy, selector precedence, or proxy logic.
- Control-plane decisioning and HTTP forwarding live in `opencode-server`.
- This crate guarantees consistent row/JSON mapping so routing logic sees deterministic metadata.

## Testing expectations

From workspace root:

```sh
cargo test -p opencode-storage
```

Tests should keep covering:

- workspace CRUD round-trip (create/get/list/delete)
- upsert updates for mutable fields
- JSON `extra` encode/decode (including invalid JSON failure mapping)

For full gate validation, run workspace checks from root:

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```
