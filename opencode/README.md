# opencode

Binary crate for the Rust `opencode` executable.

## Purpose

This crate is the runnable entrypoint for the workspace. It keeps `src/main.rs` minimal and puts command dispatch in `src/lib.rs` so behavior can be tested without spawning a separate process.

## Current Commands

| Command | Status | Current behavior |
| --- | --- | --- |
| default / `run` | stub | Logs that TUI mode is not implemented yet |
| `server [--host <h>] [--port <n>]` | active | Starts the Axum HTTP server and initializes SQLite storage; bind precedence is `CLI > resolved config > defaults` |
| `prompt <text>` | stub | Parsed, but one-shot CLI prompt mode is still not implemented |
| `version` | active | Prints `opencode <version>` |
| `config --show` | active | Loads merged config and prints it as JSON |
| `config` | stub | Logs that config editing is not implemented yet |
| `tool <name>` | active | Executes one built-in tool directly via `opencode-cli::tool_cmd::run` |

## Runtime Behavior

- The current working directory is used as the project root.
- Starting the server creates or opens `./opencode.db`.
- Startup builds one shared `ConfigService` for both initial config resolution and request-time config/provider views.
- Config resolution order is `defaults < global config < local config < env`; CLI bind flags override host/port only.
- `OPENCODE_MANUAL_HARNESS=1` enables the manual provider streaming route.
- Standard providers are registered for the harness only when that environment variable is set.
- Server startup wires `SessionEngine` from `opencode-session`, so session prompt/cancel APIs are runtime-backed.
- Server startup also wires the default SSE heartbeat driver used by `GET /api/v1/event`.
- Anthropic and Google session turns can execute the bounded Rust built-in tool loop during `POST /api/v1/sessions/:sid/prompt`.
- Server startup also wires provider parity services (`ProviderCatalogService`, `ProviderAuthService`, `AccountService`) into `opencode-server` app state.
- Provider catalog startup overlays model metadata from `.opencode/models.json` when present.

## Session Runtime Surface

Even though the CLI `prompt` command is still stubbed, server mode exposes the real session runtime endpoints:

- `POST /api/v1/sessions/:sid/prompt`
- `POST /api/v1/sessions/:sid/cancel`
- `GET /api/v1/session/status`
- `GET /api/v1/session/:sid/status`
- `POST /api/v1/session/:sid/abort`
- `POST /api/v1/session/:sid/prompt`
- `GET /api/v1/event`

These routes are served by `opencode-server` and call into `opencode-session::engine::SessionEngine`.

Key notes:

- `GET /api/v1/event` is a live SSE stream with `server.connected`, heartbeats, and translated runtime events.
- Detached prompt alias requests return acceptance metadata immediately; background failures can surface as `session.error`.

## Provider/Auth/Account parity surface

`server` mode now exposes public provider/account/config routes:

- `GET /api/v1/provider`
- `GET /api/v1/provider/auth`
- `POST /api/v1/provider/:provider/oauth/authorize`
- `POST /api/v1/provider/:provider/oauth/callback`
- `GET /api/v1/provider/account`
- `POST /api/v1/provider/account/use`
- `DELETE /api/v1/provider/account/:account_id`
- `GET /api/v1/config`
- `PATCH /api/v1/config`
- `GET /api/v1/global/config`
- `PATCH /api/v1/global/config`
- `GET /api/v1/config/providers`

Manual-only route (diagnostics):

- `POST /api/v1/provider/stream` (requires `OPENCODE_MANUAL_HARNESS=1`)

Important distinction:

- `opencode tool ...` is standalone tool execution.
- Session prompt flow lives behind the HTTP API today.
- The manual `/api/v1/provider/stream` harness is only for raw provider streaming checks; it does not exercise the persisted session tool loop.
- OAuth pending authorize/callback state is currently process-local; restarts during login require re-authorization.

## Run

From the workspace root:

```sh
cargo run -p opencode -- version
cargo run -p opencode -- config --show
cargo run -p opencode -- tool bash --args-json '{"command":"pwd","description":"print cwd"}'
```

Start the server:

```sh
cargo run -p opencode -- server --port 4141
```

## Test

```sh
cargo test -p opencode --lib
```

The library tests cover dispatch branches and a basic server startup path.
