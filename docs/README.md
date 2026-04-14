# docs/

Project documentation for the Rust workspace.

## Contents

| File | Purpose |
| --- | --- |
| [`MANUAL_TESTING.md`](MANUAL_TESTING.md) | Manual validation of session parity routes, SSE event stream, and the raw provider harness |
| [`SESSION_RUNTIME.md`](SESSION_RUNTIME.md) | Current bounded session runtime/tool-loop behavior |

## What Belongs Here

- Testing guides that describe how to exercise behavior outside unit tests
- Workspace-level notes that should stay accurate even as implementation changes
- Documentation that helps contributors understand how the Rust workspace fits together

## Current Focus

The most concrete operational guides today are:

- the session-runtime HTTP path (`projects -> sessions -> prompt -> messages`)
- the live SSE event path (`GET /api/v1/event` with `server.connected` + heartbeats)
- the lower-level provider harness (`POST /api/v1/provider/stream`)

The session runtime document is intentionally implementation-faithful: Anthropic/Google tool-loop support exists, while broader parity items remain deferred.

## How To Verify Docs Against Code

Useful commands from the workspace root:

```sh
cargo test -p opencode-server -p opencode-provider
cargo run -p opencode -- server --port 4141
```

If documentation here mentions HTTP routes or runtime behavior, it should match the Rust code under `crates/opencode-server/`, not the TypeScript implementation.
