# opencode-server/src/control_plane

Workspace control-plane slice for route decisioning and HTTP forwarding.

## Modules

- `mod.rs`
  - middleware entrypoint used by `router.rs`
  - `ControlPlaneService` for local vs forward decisions
- `policy.rs`
  - route eligibility classifier (`Eligible` / `LocalOnly`)
  - parity fixtures for known TS behavior
- `resolver.rs`
  - selector precedence resolver (`query` > `header`)
  - malformed selector validation
- `proxy.rs`
  - bounded retry/timeout forwarding transport
  - hop-by-hop header stripping
  - forwarded context headers
- `error.rs`
  - typed control-plane errors + HTTP mapping (`400/404/502/504/501`)
- `observability.rs`
  - lightweight in-process counters and structured logs

## Current behavior

- Eligible route + no selector => local pass-through.
- Eligible route + selector + same-instance workspace => local pass-through.
- Eligible route + selector + remote-instance workspace => HTTP forward.
- Forward + websocket-upgrade intent => explicit deferral (`501 Not Implemented`).

## Operational notes

- `force_local_only` in `ControlPlaneConfig` is the rollback switch.
- Retry policy (`timeout`, `max_retries`, `backoff`) is clamped to safe bounds in `ProxyPolicy::bounded`.
- Forwarded requests add:
  - `x-opencode-forwarded-workspace-selector`
  - `x-opencode-forwarded-workspace-source`
  - `x-opencode-forwarded-workspace-instance`

These headers are trace context, not end-user API contract.
