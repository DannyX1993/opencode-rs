# opencode-session

Session runtime core for the Rust agent loop.

## Status

Partial but functional for bounded runtime flows (`prompt`, `prompt_detached`, `cancel`, `status`, permission/question blocking).

## Runtime contract boundary

This crate preserves the runtime execution contract that foundation work must not break:

- runtime/tool `root` is the project worktree
- runtime `cwd` is the session directory

The project foundation stream introduced in this release is additive and external to this crate’s ownership. Session runtime behavior must remain unchanged when foundation metadata exists.

## Naming boundary

`RunSnapshot` is reserved for session runtime occupancy semantics and must not be reused for repository/worktree state naming.

## Existing capabilities

- run exclusivity and cancellation-safe run ownership
- permission/question ask/reply/reject runtime integration
- bounded provider-driven tool loop (Anthropic/Google)
- persisted tool-use/tool-result artifacts for replay
- status snapshots (`idle`, `busy`, `blocked`)

## Out of scope (still)

- full OpenAI runtime tool-loop parity
- broader orchestration/sub-agent workflow
- durable status history replay

## Testing expectations

From workspace root:

```sh
cargo test -p opencode-session
```

Keep tests proving:

- root/cwd runtime contract continuity
- run-state semantics and cancellation invariants
- naming boundary between `RunSnapshot` and repository foundation terms
