# opencode-rs

Rust workspace for the `opencode` runtime, HTTP server, domain crates, and SQLite persistence.

> Scope: this README documents the Rust workspace only. `opencode-ts/` is out of scope.

## Release

- Current workspace release: **`0.12.0`**
- Tag convention: **`v<semver>`** (for this cycle: `v0.12.0`)
- Crates use `version.workspace = true`, so all Rust packages stay lockstep with `[workspace.package]`.

## What landed in this change stream

`port-project-git-snapshot-worktree-foundations` adds a durable **project repository foundation** layer while preserving existing runtime behavior.

### Design intent

1. Keep runtime contract unchanged:
   - tool/runtime `root` continues to use `project.worktree`
   - runtime `cwd` continues to use `session.directory`
2. Persist canonical repository/worktree metadata in an additive seam.
3. Keep naming boundaries clear:
   - repository/worktree state uses `worktree_state`, `repository_state`, `sync_basis`
   - `RunSnapshot` remains session-runtime terminology only.

### New foundation behavior

- Project upsert flow (`PUT /api/v1/projects/:id`) now probes the worktree and persists companion foundation data.
- Persistence lives in additive table `project_repository_state` (migration `0002_project_repository_foundation.sql`).
- If repository metadata is not available (non-git path, git command unavailable, partial probe), persistence still succeeds with unknown fields (`None`) rather than invented values.
- Existing `/api/v1/projects` response shape remains unchanged (foundation fields are internal persistence/state data, not newly exposed API payloads).

## Workspace architecture (high level)

```text
opencode (binary)
  └─ opencode-cli
      ├─ opencode-server
      │   ├─ opencode-session
      │   ├─ opencode-storage
      │   ├─ opencode-provider
      │   └─ opencode-bus
      └─ opencode-tool

opencode-core provides shared DTOs/config/errors/IDs across all crates.
```

## Key boundaries

- `opencode-core`: shared domain contracts (including project-foundation structs/traits).
- `opencode-storage`: additive persistence seam (`project_repository_state`) + storage trait methods.
- `opencode-server`: thin route adapters; project route orchestrates probe + persistence.
- `opencode-session`: runtime engine and run-state semantics; unchanged root/cwd contract guarded by tests.

## Git/non-git fallback policy

Foundation persistence is intentionally additive and tolerant:

- **Git available + repo detected** → persist canonical worktree + repository metadata.
- **Git unavailable / command error / non-git path** → persist canonical worktree (when resolvable) with unknown repo fields.
- **Partial inspectability** → persist known fields, leave unknown fields as `None`.

This enables later enrichment without blocking current project/session workflows.

## Validation gates (required)

From workspace root:

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```

Expected quality stance:

- additive migrations only
- no behavior regression in existing HTTP/runtime contracts
- clear tests for git + non-git + unknown/partial repository states

## Workspace layout

See [`crates/README.md`](crates/README.md) for crate index and statuses.

## Additional docs

- [`crates/opencode-core/README.md`](crates/opencode-core/README.md)
- [`crates/opencode-storage/README.md`](crates/opencode-storage/README.md)
- [`crates/opencode-server/README.md`](crates/opencode-server/README.md)
- [`crates/opencode-server/src/routes/README.md`](crates/opencode-server/src/routes/README.md)
- [`crates/opencode-session/README.md`](crates/opencode-session/README.md)
- [`opencode/README.md`](opencode/README.md)
- [`opencode/src/README.md`](opencode/src/README.md)
