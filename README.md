# opencode-rs

Rust workspace for the `opencode` runtime, HTTP server, domain crates, and SQLite persistence.

> Scope: this README documents the Rust workspace only. `opencode-ts/` is out of scope.

## Release

- Current workspace release: **`0.13.0`**
- Tag convention for published git releases in this repo: **`<semver>`** (for this cycle: `0.13.0`)
- Crates use `version.workspace = true`, so all Rust packages stay lockstep with `[workspace.package]`.

## What landed in this change stream

`port-project-workspace-control-plane` adds a first control-plane slice for workspace-aware routing with safe rollback controls.

### Design intent

1. Keep existing local API/runtime behavior unchanged when no workspace selector is provided.
2. Enable selector-based local-vs-remote routing for eligible session endpoints.
3. Preserve operational safety with explicit rollback and bounded forwarding policy.
4. Keep WebSocket parity explicit: forwarding is intentionally deferred in this release.

### Control-plane behavior (v0.13.0)

- New selector resolution precedence for eligible routes:
  - `?workspace=<id>` query first
  - `x-opencode-workspace` header second
- Route policy enforces which paths are eligible for forwarding. Non-eligible routes stay local.
- Workspace metadata (`type=remote`, `extra.instance`, `extra.base_url`) drives remote target resolution.
- Requests targeting a remote instance are proxied preserving method/path/query/body, with hop-by-hop header stripping.
- Forwarding uses bounded timeout/retry/backoff policy for resilient cloud-native operation.
- WebSocket upgrade forwarding returns explicit deferral (`501 Not Implemented`) in this slice.

### Rollback / rollout notes

- Emergency rollback switch: `force_local_only=true` keeps all traffic local without changing route handlers.
- Safe rollout strategy:
  1. deploy with local-only enabled,
  2. verify observability/log signals,
  3. disable local-only progressively per environment.

### Manual usage hints

- Local handling (default parity):

```http
GET /api/v1/sessions/<sid>
```

- Explicit remote targeting by query:

```http
POST /api/v1/sessions/<sid>/prompt?workspace=<workspace-id>
```

- Explicit remote targeting by header:

```http
x-opencode-workspace: <workspace-id>
```

- Remote workspace metadata must include:
  - `instance`: destination control-plane instance id
  - `base_url`: absolute `http(s)` base URL

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

- `opencode-core`: shared IDs/DTOs used by server + storage layers.
- `opencode-storage`: workspace persistence (`workspace` rows, metadata blobs) consumed by control-plane resolution.
- `opencode-server`: control-plane middleware, route policy, selector resolver, and HTTP proxy forwarding.
- `opencode-session`: session runtime semantics remain unchanged; control-plane only decides where requests execute.

## Compatibility policy

- No selector provided on eligible route → request stays local.
- Selector present but route is local-only by policy → request stays local.
- Selector present + remote target resolved + same instance id → request stays local.
- Selector present + remote target resolved + different instance id → request is forwarded.
- WebSocket upgrade + forward decision → explicit deferral (`501`) rather than silent downgrade.

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
- [`crates/opencode-server/src/control_plane/README.md`](crates/opencode-server/src/control_plane/README.md)
- [`crates/opencode-server/src/routes/README.md`](crates/opencode-server/src/routes/README.md)
- [`crates/opencode-session/README.md`](crates/opencode-session/README.md)
- [`opencode/README.md`](opencode/README.md)
- [`opencode/src/README.md`](opencode/src/README.md)
