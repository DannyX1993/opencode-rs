# opencode-storage

SQLite-backed persistence boundary for the Rust workspace.

## Role

`opencode-storage` owns migrations, repository mappers, and the `Storage` trait used by server/session/provider runtime layers.

## Project repository foundation seam

This change adds an additive companion persistence seam for canonical repository/worktree state.

### Schema

- New migration: `migrations/0002_project_repository_foundation.sql`
- New table: `project_repository_state`
- Keyed by `project_id`
- Keeps existing `project` table/API usage stable

### Storage trait additions

- `get_project_foundation(project_id)`
- `upsert_project_foundation(row)`

Both methods are additive and do not alter legacy project CRUD behavior.

### Repository behavior

`repo/project_repository_state.rs` maps nullable canonical/repository/vcs/sync fields and JSON state payloads.

Fallback semantics are deliberate:

- missing JSON columns deserialize to defaults when appropriate
- missing optional JSON payloads remain `None`
- partial state persists without fabrication

## Boundaries

- This crate persists data; it does not execute git probing logic.
- Probing happens at route/domain layer (`opencode-server`), then writes normalized rows through this seam.

## Testing expectations

From workspace root:

```sh
cargo test -p opencode-storage
```

Tests should keep covering:

- upgrade from `0001_initial.sql` to include foundation table
- round-trip CRUD for full and partial foundation rows
- `None`-heavy/non-git-compatible payload persistence

For full gate validation, run workspace checks from root:

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```
