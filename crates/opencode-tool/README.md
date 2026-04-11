# opencode-tool

Tool runtime and built-in tools for the Rust workspace.

## Status

Active. This crate is already used by the binary through `opencode tool ...`.

## What Exists Today

- `Tool` trait and associated types
- `ToolRegistry`
- execution context `Ctx`
- built-in tools: `read`, `list`, `glob`, `grep`, `write`, `bash`

## Current Behavior

- `ToolRegistry::with_builtins` registers the built-in tools used by the CLI.
- `glob` and `grep` use native Rust crates instead of shelling out to `rg`.
- `bash` runs shell commands with the context timeout.
- `read` and `write` operate on files and directories through the Rust tool layer.

## CLI Relationship

The main binary reaches this crate through `opencode-cli::tool_cmd::run`, which parses JSON arguments and returns either plain text output or a JSON envelope.

## Example

```sh
cargo run -p opencode -- tool glob \
  --args-json '{"pattern":"**/*.rs","path":"crates/opencode-tool"}'
```

## Test

```sh
cargo test -p opencode-tool
```
