# opencode-tool

Core tool infrastructure for `opencode-rs`: the `Tool` trait, type system,
shared execution kernel, thread-safe registry, and all six built-in tools.

> **Status**: ✅ Complete — 72/72 tests, ≥96% line/region coverage.

---

## Overview

This crate is the engine that powers every tool invocation in opencode.
It mirrors the TS `packages/opencode/src/tool/` module.

---

## Core Types

| Type           | Description                                                                 |
| -------------- | --------------------------------------------------------------------------- |
| `Tool`         | Async trait — implement `name()` and `invoke(ToolCall)`                     |
| `ToolCall`     | Input: `{ id, name, args: serde_json::Value }`                              |
| `ToolResult`   | Output: `{ call_id, is_err, output, title, metadata, output_path }`         |
| `ToolError`    | Typed errors: `NotFound`, `InvalidArgs`, `Timeout`, `Exec`, `BinaryFile`, … |
| `ToolPolicy`   | Resource policy: reads, writes, net, exclusive                              |
| `ToolRegistry` | Thread-safe map of `name → Arc<dyn Tool>`                                   |
| `Ctx`          | Execution context: root, cwd, out_dir, shell, timeout                       |

---

## Built-in Tools

| Name      | `Tool::name()` | TS equivalent | Description                                                    |
| --------- | -------------- | ------------- | -------------------------------------------------------------- |
| ReadTool  | `read`         | `ReadTool`    | Read file contents with line offset/limit, reject binary files |
| LsTool    | `list`         | `ListTool`    | Directory tree with ignore patterns, capped at 100 entries     |
| GlobTool  | `glob`         | `GlobTool`    | Find files matching a glob (native `globset`+`walkdir`)        |
| GrepTool  | `grep`         | `GrepTool`    | Search file contents with regex (native `regex`+`walkdir`)     |
| WriteTool | `write`        | `WriteTool`   | Write or overwrite a file                                      |
| BashTool  | `bash`         | `BashTool`    | Execute a shell command with configurable timeout              |

> **Safe improvement**: GlobTool and GrepTool use native Rust crates instead of spawning `rg`.
> Same output contract, no subprocess overhead, fully deterministic in tests.

---

## Ctx Setup

```rust
use opencode_tool::Ctx;
use std::path::PathBuf;

// Minimal context
let ctx = Ctx::default_for(PathBuf::from("/my/project"));

// Full control
let ctx = Ctx::new(
    PathBuf::from("/my/project"),   // root
    PathBuf::from("/my/project"),   // cwd
    std::env::temp_dir(),           // out_dir (for truncated output)
    "/bin/bash".into(),             // shell
    30_000,                         // timeout_ms
);
```

---

## Example: Invoke a Tool in Rust

```rust
use opencode_tool::{Ctx, ToolCall, ToolRegistry};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let ctx = Ctx::default_for(PathBuf::from("."));
    let reg = ToolRegistry::with_builtins(ctx);

    let call = ToolCall {
        id: "my-call-1".into(),
        name: "bash".into(),
        args: serde_json::json!({
            "command": "echo hello from opencode",
            "description": "greet"
        }),
    };

    match reg.invoke(call).await {
        Ok(res) => println!("{}", res.output),
        Err(e) => eprintln!("error: {e}"),
    }
}
```

---

## Implement a Custom Tool

```rust
use async_trait::async_trait;
use opencode_tool::{Tool, ToolCall, ToolError, ToolResult};

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &'static str { "echo" }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let msg = call.args["message"]
            .as_str()
            .unwrap_or("(no message)")
            .to_string();
        Ok(ToolResult::ok(call.id, "echo".into(), msg))
    }
}
```

---

## Coverage

```sh
cargo llvm-cov -p opencode-tool --summary-only
```

Current: **≥96% line / ≥94% region** (gate: ≥85%).

---

## Module Map

```
src/
├── lib.rs           — re-exports: Ctx, Tool, ToolCall, ToolResult, ToolError, ToolRegistry
├── types.rs         — Tool trait + all associated types
├── registry.rs      — ToolRegistry (RwLock<HashMap<String, Arc<dyn Tool>>>)
├── common/
│   ├── ctx.rs       — Ctx struct + DEFAULT_TIMEOUT_MS
│   ├── fs.rs        — is_binary(), read_lines()
│   ├── shell.rs     — run_shell() async
│   └── truncate.rs  — truncate() helper
└── tools/
    ├── read.rs
    ├── ls.rs
    ├── glob.rs
    ├── grep.rs
    ├── write.rs
    └── bash.rs
```
