# opencode-server

Axum HTTP server surface for the Rust workspace.

## Role

`opencode-server` exposes project/session/provider/config routes while keeping handlers thin and delegating stateful logic to domain/storage crates.

## Project foundation behavior (this change)

`PUT /api/v1/projects/:id` now performs two writes:

1. Legacy project upsert (`project` row)
2. Additive foundation upsert (`project_repository_state` row)

Foundation capture uses a `RepositoryProbe` implementation (`GitCliRepositoryProbe`) and preserves compatibility guarantees:

- existing project route response shape remains unchanged
- non-git worktrees still persist successfully
- probe failures (git missing/command error) fall back to unknown repository fields
- known metadata is persisted when available; unknown fields remain `None`

## Route surface

See [`src/routes/README.md`](src/routes/README.md) for module-level route responsibilities.

## Boundaries

- No control-plane/workspace-orchestration behavior is introduced here.
- Foundation metadata is internal persistence for later orchestration readiness, not a new public payload contract.
- Session runtime root/cwd semantics remain owned by `opencode-session` and validated in integration tests.

## Test expectations

From workspace root:

```sh
cargo test -p opencode-server
```

Server tests should continue validating:

- project upsert/list/get behavior remains stable
- foundation persistence for git and non-git/unknown scenarios
- session and runtime route contracts are not changed by enriched project foundation metadata

Full required workspace gates:

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```
