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
| `POST` | `/api/v1/sessions/{sid}/prompt` | starts a runtime prompt turn via `opencode-session`, including bounded Anthropic/Google tool loops |
| `POST` | `/api/v1/sessions/{sid}/cancel` | cancels active prompt turn for that session |
| `GET` | `/api/v1/provider` | returns visible provider catalog + defaults + connected providers |
| `GET` | `/api/v1/provider/auth` | returns supported auth methods per built-in provider |
| `POST` | `/api/v1/provider/{provider}/oauth/authorize` | starts OAuth/device-style handoff for supported methods |
| `POST` | `/api/v1/provider/{provider}/oauth/callback` | completes callback and persists account state on success |
| `GET` | `/api/v1/provider/account` | returns persisted accounts + active account/org state |
| `POST` | `/api/v1/provider/account/use` | sets active account and optional active org |
| `DELETE` | `/api/v1/provider/account/{account_id}` | removes persisted account and clears invalid active state |
| `GET` | `/api/v1/config/providers` | returns connected provider subset + defaults |
| `POST` | `/api/v1/provider/stream` | manual SSE harness, only when enabled |

## Important Limitations

- The provider stream route returns `403` unless `OPENCODE_MANUAL_HARNESS=1` was set at startup.
- The session engine now covers a bounded runtime tool loop for Anthropic/Google, but full runtime parity is not complete.
- `/api/v1/provider/stream` is a raw provider harness; it does not create sessions, persist history, or exercise the session replay loop by itself.
- OpenAI remains available through the provider layer and harness, but not as a tool-capable session-runtime provider in this MVP.
- OAuth pending authorization state is in-process (not durable across server restarts).
- This crate does not currently expose a complete public API contract for all future agent features.

## Public vs manual provider routes

- Public parity routes: `/api/v1/provider*` and `/api/v1/config/providers`.
- Manual harness route: `/api/v1/provider/stream` (env-gated, diagnostics only).

## Manual validation expectations for parity routes

1. Call `GET /api/v1/provider/auth` to discover available methods.
2. Start with `POST /api/v1/provider/{provider}/oauth/authorize`.
3. Complete with `POST /api/v1/provider/{provider}/oauth/callback`.
4. Validate persistence using `GET /api/v1/provider/account`.
5. Validate state mutation with `/api/v1/provider/account/use` and account deletion.

## Manual session-runtime path

To exercise the persisted runtime loop through HTTP today:

1. Create or upsert a project.
2. Create a session for that project.
3. Call `POST /api/v1/sessions/{sid}/prompt` with text and an Anthropic/Google model.
4. Inspect `GET /api/v1/sessions/{sid}/messages` to confirm assistant text, `tool_use`, and `tool_result` artifacts.

Use `/api/v1/provider/stream` only when you want a lower-level provider streaming check without session persistence.

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
