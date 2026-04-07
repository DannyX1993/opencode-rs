# opencode-core

Foundation types for `opencode-rs`: configuration loading, tracing initialisation,
shared DTOs, error types, and ID generation.

> **Status**: ✅ Complete — Config, tracing init, and core types implemented.

---

## Purpose

`opencode-core` is the dependency-free foundation that every other crate builds on.
It has no circular dependencies and no optional heavy features.

---

## Key Components

| Module    | Description                                                                  |
| --------- | ---------------------------------------------------------------------------- |
| `config`  | `Config` struct — loads from `~/.opencode/config.jsonc` and project dir      |
| `tracing` | `init(cfg)` — sets up `tracing-subscriber` with env-filter and optional JSON |
| `dto`     | Shared data transfer objects                                                 |
| `error`   | `CoreError` — base error type                                                |
| `id`      | UUID-based ID generation helpers                                             |
| `context` | Request context threading                                                    |

---

## Config Fields (key ones)

```rust
pub struct Config {
    pub log_level: String,      // default: "info"
    pub log_json: bool,         // default: false
    pub model: Option<String>,  // AI model to use
    pub server: ServerConfig,   // port: 4141
    pub providers: ProvidersConfig,
    // ...
}
```

Config is loaded with `Config::load(project_dir)`. It merges:

1. Built-in defaults
2. `~/.opencode/config.jsonc` (global)
3. `<project_dir>/.opencode/config.jsonc` (project-local)

---

## Tracing Init

```rust
use opencode_core::{config::Config, tracing};

let cfg = Config::load(dir).await?;
tracing::init(&cfg);  // sets up subscriber based on log_level + log_json
```

---

## Module Map

```
src/
├── lib.rs       — pub mod declarations
├── config.rs    — Config struct + loading logic
├── tracing.rs   — tracing subscriber initialisation
├── dto.rs       — shared DTOs
├── error.rs     — CoreError
├── id.rs        — ID generation
└── context.rs   — request context
```
