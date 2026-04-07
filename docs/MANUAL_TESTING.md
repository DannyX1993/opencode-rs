# Manual Testing Guide — Provider Harness

This guide explains how to manually test the real LLM provider implementations
(Anthropic, OpenAI) by starting the HTTP server and using `curl`.

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
  cargo run -p opencode -- server
```

> The server will print: `opencode server listening on 0.0.0.0:4141` (or
> whichever port is configured via `--port`).

To use a custom port:

```bash
OPENCODE_MANUAL_HARNESS=1 OPENAI_API_KEY=sk-... \
  cargo run -p opencode -- server --port 4000
```

## Endpoints

### `POST /api/v1/provider/stream`

Streams model events as Server-Sent Events (one JSON object per `data:` line).

**Request body** (JSON):

| Field        | Type   | Required | Description                                    |
| ------------ | ------ | -------- | ---------------------------------------------- |
| `provider`   | string | ✅       | Provider id: `"anthropic"` or `"openai"`       |
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
