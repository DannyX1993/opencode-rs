# crates/

Workspace crates for `opencode-rs`.

## Crate Index

| Crate                                  | Status      | Description                                             |
| -------------------------------------- | ----------- | ------------------------------------------------------- |
| [opencode-tool](opencode-tool)         | ✅ Complete | Tool trait, `ToolRegistry`, 6 built-in tools            |
| [opencode-cli](opencode-cli)           | ✅ Complete | CLI parsing (`clap`), bootstrap, `tool` subcommand      |
| [opencode-core](opencode-core)         | ✅ Complete | Config loader (JSONC), tracing init, shared types       |
| [opencode-provider](opencode-provider) | ✅ Complete | OpenAI, Anthropic, Google LLM provider adapters         |
| [opencode-server](opencode-server)     | ✅ Partial  | Axum HTTP API — health, session, and provider endpoints |
| [opencode-storage](opencode-storage)   | ✅ Complete | SQLite async storage via `sqlx`                         |
| [opencode-session](opencode-session)   | 🔲 Stub     | Session engine placeholder                              |
| [opencode-bus](opencode-bus)           | 🔲 Stub     | Broadcast event bus placeholder                         |
| [opencode-lsp](opencode-lsp)           | 🔲 Planned  | Language Server Protocol integration                    |
| [opencode-mcp](opencode-mcp)           | 🔲 Planned  | Model Context Protocol support                          |
| [opencode-plugin](opencode-plugin)     | 🔲 Planned  | Plugin system                                           |
| [opencode-tui](opencode-tui)           | 🔲 Planned  | Terminal user interface                                 |

## Status Legend

| Symbol      | Meaning                                              |
| ----------- | ---------------------------------------------------- |
| ✅ Complete | Implemented, tested, and ≥85% covered                |
| ✅ Partial  | Core functionality works; some features pending      |
| 🔲 Stub     | Crate exists with minimal scaffolding; logic pending |
| 🔲 Planned  | Not yet started                                      |

See the [workspace README](../README.md) for build and usage instructions.
