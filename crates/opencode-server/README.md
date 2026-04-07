# opencode-server

Axum HTTP API server for `opencode-rs`.

> **Status**: ✅ Implemented — health endpoint, provider streaming, session routes (partial).

---

## Purpose

`opencode-server` builds the Axum router that exposes the opencode REST API.
It is used in headless mode (`opencode server`) and by integration tests.

---

## Endpoints

| Method | Path                      | Description                              |
| ------ | ------------------------- | ---------------------------------------- |
| `GET`  | `/health`                 | Health check — returns `{"status":"ok"}` |
| `POST` | `/api/v1/provider/stream` | Stream completions from a provider       |
| `GET`  | `/api/v1/session`         | List sessions (stub)                     |
| `POST` | `/api/v1/session`         | Create session (stub)                    |

---

## Usage

```rust
use opencode_server::{AppState, build, serve};
use std::net::SocketAddr;

let router = build(state);
let addr: SocketAddr = "0.0.0.0:4141".parse()?;
serve(router, addr).await?;
```

`AppState` bundles config, bus, storage, session engine, and model registry.
All are `Arc`-wrapped for cheap cloning across handlers.
