# opencode-server

Axum HTTP server for the Rust workspace.

## Status

Active. The crate has real routes, router tests, and is used by `cargo run -p opencode -- server`.

## Routes Wired Today

| Method | Path | Current behavior |
| --- | --- | --- |
| `GET` | `/health` | returns `{ "status": "ok" }` |
| `GET` | `/api/v1/projects` | lists projects from storage |
| `PUT` | `/api/v1/projects/{id}` | upserts a project |
| `GET` | `/api/v1/projects/{id}` | fetches one project |
| `POST` | `/api/v1/projects/{pid}/sessions` | creates a session row |
| `GET` | `/api/v1/projects/{pid}/sessions` | lists sessions for a project |
| `GET` | `/api/v1/sessions/{sid}` | fetches one session |
| `PATCH` | `/api/v1/sessions/{sid}` | updates mutable session fields |
| `GET` | `/api/v1/sessions/{sid}/messages` | lists messages with parts |
| `POST` | `/api/v1/sessions/{sid}/messages` | appends a message and parts |
| `POST` | `/api/v1/sessions/{sid}/prompt` | starts a runtime prompt turn via `opencode-session` |
| `POST` | `/api/v1/sessions/{sid}/cancel` | cancels active prompt turn for that session |
| `POST` | `/api/v1/provider/stream` | manual SSE harness, only when enabled |

## Important Limitations

- The provider stream route returns `403` unless `OPENCODE_MANUAL_HARNESS=1` was set at startup.
- The session engine now covers runtime core behavior (prompt/cancel + stream persistence), but full runtime parity is not complete yet.
- Tool-use stream event handling is deferred in runtime-core and currently returns an explicit unsupported error path.
- This crate does not currently expose a complete public API contract for all future agent features.

## Run

From the workspace root:

```sh
cargo run -p opencode -- server --port 4141
```

Enable the manual provider harness:

```sh
OPENCODE_MANUAL_HARNESS=1 cargo run -p opencode -- server --port 4141
```

## Test

```sh
cargo test -p opencode-server
```

## Workspace Role

`opencode-server` sits on top of `opencode-storage`, `opencode-provider`, `opencode-session`, and `opencode-bus` to expose the Rust runtime over HTTP.
