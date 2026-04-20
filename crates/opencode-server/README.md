# opencode-server

Axum HTTP server surface for the Rust workspace.

## Role

`opencode-server` exposes project/session/provider/config routes while keeping handlers thin and delegating stateful logic to domain/storage crates.

## Workspace control-plane behavior (this change)

This crate now includes a control-plane middleware slice for workspace-aware routing.

### Request decision flow

1. Classify route via control-plane policy (`Eligible` vs `LocalOnly`).
2. Resolve selector precedence (`?workspace=` first, `x-opencode-workspace` second).
3. Load workspace metadata from storage.
4. Decide local vs forward using instance identity + remote target metadata.
5. If forwarding, proxy the request with bounded timeout/retry/backoff.

### Compatibility guarantees

- Existing handlers remain unchanged; middleware only gates execution path.
- Requests without selector preserve local behavior parity.
- Local-only policy routes always run in-process.
- Forwarded HTTP requests preserve method/path/query/body.
- WebSocket forward path is explicitly deferred (returns `501`).

## Route surface

See [`src/routes/README.md`](src/routes/README.md) for module-level route responsibilities.

See [`src/control_plane/README.md`](src/control_plane/README.md) for middleware/proxy internals.

In `v0.14.0`, `opencode-cli` core commands (`providers list`, `session list`, non-interactive `run`/`prompt`) are expected to consume these server route contracts directly for backend alignment.

## Boundaries

- This crate owns HTTP boundary concerns (routing, middleware, extractor validation, proxying).
- Storage schema and persistence mapping remain in `opencode-storage`.
- Session runtime behavior remains in `opencode-session` (control-plane only decides placement).
- Workspace metadata validation for remote routing is explicit and fails fast on malformed payloads.

## Test expectations

From workspace root:

```sh
cargo test -p opencode-server
```

Server tests should continue validating:

- local-only route enforcement remains stable
- selector precedence and validation errors are deterministic
- forwarding retries/timeouts map to expected HTTP errors
- websocket forwarding deferral is explicit and test-covered

Full required workspace gates:

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/coverage.sh --check
```
