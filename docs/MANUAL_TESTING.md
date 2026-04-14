# Manual Testing Guide — Session Runtime + Provider Harness

This guide explains how to manually test the current Rust runtime by starting the HTTP server and using `curl`.

It covers two different paths:

1. the **session runtime** HTTP flow, which exercises persisted history, runtime tool execution, and replay
2. the lower-level **provider harness**, which exercises raw provider streaming only

## Prerequisites

- Rust toolchain installed (`cargo`)
- Valid API key for at least one provider
- `curl` available in your shell

## Starting the Server with the Harness Enabled

The manual harness route (`POST /api/v1/provider/stream`) is **disabled by
default**. Set the environment variable `OPENCODE_MANUAL_HARNESS=1` at server
startup to enable it.

```bash
cd opencode-rs
OPENCODE_MANUAL_HARNESS=1 \
  ANTHROPIC_API_KEY=sk-ant-... \
  OPENAI_API_KEY=sk-... \
  GOOGLE_API_KEY=... \
  cargo run -p opencode -- server
```

> The server will print: `opencode server listening on 0.0.0.0:4141` (or
> whichever port is configured via `--port`).

To use a custom port:

```bash
OPENCODE_MANUAL_HARNESS=1 OPENAI_API_KEY=sk-... \
  cargo run -p opencode -- server --port 4000
```

## Session runtime path (recommended for this change)

Use this path to validate the `expand-tool-runtime-parity` work.

### Supported providers for tool-capable turns

- `anthropic`
- `google`

OpenAI is still useful for raw provider checks, but not for the bounded session tool loop.

### 1) Create or upsert a project

Choose a project id first and reuse it below:

```bash
PID="11111111-1111-1111-1111-111111111111"
NOW=$(date +%s000)

curl -X PUT http://localhost:4141/api/v1/projects/$PID \
  -H "Content-Type: application/json" \
  -d '{
    "id": "'"$PID"'",
    "worktree": "/home/dannyx/projects/Rust/opencode-rs",
    "vcs": "git",
    "name": "opencode-rs",
    "icon_url": null,
    "icon_color": null,
    "time_created": '"$NOW"',
    "time_updated": '"$NOW"',
    "time_initialized": null,
    "sandboxes": null,
    "commands": null
  }'
```

This route returns `204 No Content` on success.

### 2) Create a session

Choose a session id up front. This route also returns no response body.

```bash
SID="22222222-2222-2222-2222-222222222222"

curl -X POST http://localhost:4141/api/v1/projects/$PID/sessions \
  -H "Content-Type: application/json" \
  -d '{
    "id": "'"$SID"'",
    "workspace_id": null,
    "parent_id": null,
    "slug": "manual-runtime-test",
    "directory": "/home/dannyx/projects/Rust/opencode-rs",
    "title": "Manual runtime test",
    "version": "0.9.0",
    "share_url": null,
    "summary_additions": null,
    "summary_deletions": null,
    "summary_files": null,
    "summary_diffs": null,
    "revert": null,
    "permission": null,
    "time_created": '"$NOW"',
    "time_updated": '"$NOW"',
    "time_compacting": null,
    "time_archived": null
  }'
```

This route returns `201 Created` on success.

### 3) Start a prompt turn

Anthropic example:

```bash
curl -X POST http://localhost:4141/api/v1/sessions/$SID/prompt \
  -H "Content-Type: application/json" \
  -d '{
    "text": "List the Rust crates in this workspace using available tools.",
    "model": "anthropic/claude-3-5-sonnet-20241022"
  }'
```

Google example:

```bash
curl -X POST http://localhost:4141/api/v1/sessions/$SID/prompt \
  -H "Content-Type: application/json" \
  -d '{
    "text": "Read the root README and summarize the current runtime tool support.",
    "model": "google/gemini-2.0-flash"
  }'
```

The route returns `202 Accepted` when the run is queued successfully.

### 4) Inspect persisted messages and tool replay artifacts

```bash
curl http://localhost:4141/api/v1/sessions/$SID/messages
```

Look for persisted parts/messages such as:

- user text message
- assistant text parts
- assistant `tool_use` part
- `tool` role message containing a `tool_result` part

That confirms the provider -> tool -> provider loop was replayed from storage.

### 5) Optional: cancel an in-flight run

```bash
curl -X POST http://localhost:4141/api/v1/sessions/$SID/cancel
```

## Raw provider harness path

Use this only when you want to validate the provider adapter itself without session persistence.

## Provider/auth/account parity path

These endpoints are public API surface (no harness flag required):

- `GET /api/v1/provider`
- `GET /api/v1/provider/auth`
- `POST /api/v1/provider/{provider}/oauth/authorize`
- `POST /api/v1/provider/{provider}/oauth/callback`
- `GET /api/v1/provider/account`
- `POST /api/v1/provider/account/use`
- `DELETE /api/v1/provider/account/{account_id}`
- `GET /api/v1/config/providers`

Expected manual flow:

1. Read methods from `/api/v1/provider/auth`.
2. Start auth via `/oauth/authorize`.
3. Complete with `/oauth/callback`.
4. Verify persisted account state and active selection via `/provider/account`.
5. Optionally switch active account/org and remove account to verify cleanup behavior.

## Endpoints

### `POST /api/v1/provider/stream`

Streams model events as Server-Sent Events (one JSON object per `data:` line).

**Request body** (JSON):

| Field        | Type   | Required | Description                                    |
| ------------ | ------ | -------- | ---------------------------------------------- |
| `provider`   | string | ✅       | Provider id: `"anthropic"`, `"google"`, or `"openai"` |
| `model`      | string | ✅       | Model id (e.g. `"claude-3-5-sonnet-20241022"`) |
| `prompt`     | string | ✅       | The user message text                          |
| `max_tokens` | number | optional | Token cap (default: 1024)                      |

**Error responses**:

| Status | Reason                                                        |
| ------ | ------------------------------------------------------------- |
| 403    | Harness disabled (`OPENCODE_MANUAL_HARNESS` not set to `"1"`) |
| 404    | Unknown provider id                                           |
| 502    | Provider returned an error (auth, rate limit, network)        |

## Example: Anthropic (Claude)

```bash
curl -N -X POST http://localhost:4141/api/v1/provider/stream \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "anthropic",
    "model": "claude-3-5-sonnet-20241022",
    "prompt": "What is 2 + 2?",
    "max_tokens": 64
  }'
```

Expected output (newline-delimited SSE):

```
data: {"type":"text_delta","delta":"2 + 2 equals 4."}

data: {"type":"usage","input":12,"output":7,"cache_read":0,"cache_write":0}

data: {"type":"done","reason":"end_turn"}
```

## Example: OpenAI (gpt-4.1-nano)

Native OpenAI requests are routed through the **Responses API** (`/v1/responses`).
Use any model available on that endpoint (e.g. `gpt-4.1-nano`, `gpt-4o`).

```bash
curl -N -X POST http://localhost:4141/api/v1/provider/stream \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "openai",
    "model": "gpt-4.1-nano",
    "prompt": "What is 2 + 2?",
    "max_tokens": 64
  }'
```

## Example: Google (Gemini)

```bash
curl -N -X POST http://localhost:4141/api/v1/provider/stream \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "google",
    "model": "gemini-2.0-flash",
    "prompt": "What is 2 + 2?",
    "max_tokens": 64
  }'
```

## Example: Missing / Wrong Key (expected 502)

```bash
curl -X POST http://localhost:4141/api/v1/provider/stream \
  -H "Content-Type: application/json" \
  -d '{"provider":"anthropic","model":"claude-3-5-sonnet-20241022","prompt":"hi"}'
```

Response when started without `ANTHROPIC_API_KEY`:

```json
{ "error": "auth error for anthropic: env var ANTHROPIC_API_KEY not set and no config key provided" }
```

## Example: Harness Disabled (expected 403)

```bash
# Start server WITHOUT OPENCODE_MANUAL_HARNESS=1
curl -X POST http://localhost:4141/api/v1/provider/stream \
  -H "Content-Type: application/json" \
  -d '{"provider":"openai","model":"gpt-4.1-nano","prompt":"hi"}'
```

```json
{ "error": "harness disabled" }
```

## Security Note

The harness endpoint is intentionally NOT part of the production API. It
requires a real API key and makes live network requests to external providers.

- **Never expose a harness-enabled server to the public internet.**
- The `OPENCODE_MANUAL_HARNESS` flag must be set explicitly at startup — it
  cannot be enabled via the API itself.
- All API keys are read from environment variables at startup; they are
  never stored or logged.

## Important scope notes

- `POST /api/v1/provider/stream` does **not** exercise session persistence or the runtime tool loop.
- `POST /api/v1/sessions/:sid/prompt` is the correct path for validating the bounded Anthropic/Google runtime parity work.
- The current workspace version remains `0.9.0`.
