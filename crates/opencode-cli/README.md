# opencode-cli

Thin CLI layer for `opencode-rs`: clap argument parsing, bootstrap sequence,
and the `tool` subcommand dispatcher.

> **Status**: ✅ Complete — 21/21 tests, ≥95% line coverage.

---

## Purpose

`opencode-cli` sits between the binary entrypoint (`opencode/src/main.rs`) and
the rest of the crate graph. It does three things:

1. **Parse** CLI args via clap (`cli.rs`)
2. **Bootstrap** the runtime — config load + tracing init (`bootstrap.rs`)
3. **Dispatch** `tool` subcommand to `opencode-tool` via `tool_cmd::run` (`tool_cmd.rs`)

---

## Available Subcommands

| Subcommand          | Status         | Description                                  |
| ------------------- | -------------- | -------------------------------------------- |
| _(none)_            | 🔲 Stub        | Defaults to `run` (TUI, not yet implemented) |
| `run`               | 🔲 Stub        | Start interactive TUI                        |
| `server [--port N]` | ✅ Implemented | Start HTTP API server (default port 4141)    |
| `prompt <text>`     | 🔲 Stub        | One-shot prompt                              |
| `version`           | ✅ Implemented | Print version string                         |
| `config [--show]`   | ✅ Implemented | Show merged config as JSON                   |
| `tool <name>`       | ✅ Implemented | Invoke a built-in tool                       |

---

## `tool` Subcommand

```sh
opencode tool <NAME> [--args-json '<JSON>'] [--output text|json]
```

### Flags

| Flag          | Default | Description                                                               |
| ------------- | ------- | ------------------------------------------------------------------------- |
| `<NAME>`      | —       | Tool name: `read`, `list`, `glob`, `grep`, `write`, `bash`                |
| `--args-json` | `{}`    | JSON-encoded tool arguments                                               |
| `--output`    | `text`  | Output format: `text` (just the output field) or `json` (full ToolResult) |

### Examples

```sh
# Read a file
opencode tool read --args-json '{"filePath":"README.md","limit":10}'

# Run a bash command
opencode tool bash --args-json '{"command":"pwd","description":"print cwd"}'

# JSON envelope output
opencode tool bash --args-json '{"command":"date","description":"current date"}' --output json
```

---

## Exit Codes

| Code | Meaning                                                    |
| ---- | ---------------------------------------------------------- |
| 0    | Success                                                    |
| 1    | Tool invocation failed (NotFound, InvalidArgs, Exec, etc.) |
| 2+   | Clap parse error (arg validation failure)                  |

---

## Error Behaviour

- Invalid JSON in `--args-json` → error message + exit 1
- Unknown tool name → `not found` error + exit 1
- Tool logic failure (e.g. file not found, binary file) → error message + exit 1

---

## Module Map

```
src/
├── lib.rs         — module declarations (bootstrap, cli, tool_cmd)
├── cli.rs         — Cli struct + Command enum (clap derive)
├── bootstrap.rs   — bootstrap(project_dir) → Config + tracing init
└── tool_cmd.rs    — run(name, args_json, output, cwd) → Result<String>
```
