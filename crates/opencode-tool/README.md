# opencode-tool

Tool runtime and built-in tools for the Rust workspace.

## Status

Active. This crate is used both by the standalone `opencode tool ...` command and by the bounded session runtime tool loop.

## What Exists Today

- `Tool` trait and associated types
- `ToolRegistry`
- runtime-facing `ToolDefinition` metadata (`name`, `description`, `input_schema`)
- execution context `Ctx`
- built-in tools: `read`, `list`, `glob`, `grep`, `write`, `bash`

## Current Behavior

- `ToolRegistry::with_builtins` registers the built-in tools used by the CLI.
- `ToolRegistry::definitions()` exposes provider-facing metadata for the same built-ins used during session prompt tool execution.
- `glob` and `grep` use native Rust crates instead of shelling out to `rg`.
- `bash` runs shell commands with the context timeout.
- `read` and `write` operate on files and directories through the Rust tool layer.
- `ToolResult::as_provider_tool_result_content()` formats persisted tool output for provider replay without leaking storage-specific structure into adapters.

## Runtime relationships

- Standalone CLI path: `opencode-cli::tool_cmd::run` parses JSON arguments and executes one tool immediately.
- Session path: `opencode-session` asks the registry for tool definitions, sends them to supported providers, then invokes the selected built-in and persists the result for replay.

These are related but different flows: the CLI command is direct execution, while session prompts are provider-driven and storage-backed.

## Example

```sh
cargo run -p opencode -- tool glob \
  --args-json '{"pattern":"**/*.rs","path":"crates/opencode-tool"}'
```

## Test

```sh
cargo test -p opencode-tool
```
