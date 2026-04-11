# opencode-cli

CLI definitions and bootstrap helpers for `opencode`.

## Status

Active. This crate is used by the current binary and has real tests around command parsing and tool dispatch.

## Purpose

`opencode-cli` is the thin layer between the executable and the rest of the workspace. It owns:

- clap argument parsing
- bootstrap of config and tracing
- the `tool` subcommand adapter into `opencode-tool`

## Commands Defined Here

| Command | Implemented in runtime? | Notes |
| --- | --- | --- |
| `run` | no | default command, still stubbed in `opencode` |
| `server [--port N]` | yes | starts HTTP server |
| `prompt <text>` | no | parsed here, still stubbed in runtime |
| `version` | yes | printed by `opencode` |
| `config [--show]` | partial | `--show` works; edit mode is stubbed |
| `tool <name>` | yes | invokes built-in tool registry |

## Tool Command

Syntax:

```sh
opencode tool <NAME> [--args-json '<JSON>'] [--output text|json]
```

Supported output values are exactly `text` and `json`.

Examples:

```sh
opencode tool read --args-json '{"filePath":"README.md","limit":5}'
opencode tool bash --args-json '{"command":"pwd","description":"print cwd"}' --output json
```

## Test

```sh
cargo test -p opencode-cli
```

## Workspace Role

`opencode/src/main.rs` parses args through this crate, then dispatches to the runtime in `opencode/src/lib.rs`.
