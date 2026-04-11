# opencode-lsp

Placeholder crate for future LSP integration.

## Status

Stub. The crate currently contains only a minimal `src/lib.rs` and package metadata so the workspace can reserve the boundary cleanly.

## Current Reality

- no public API beyond the crate existing
- no LSP client implementation yet
- no runtime integration with the binary or server yet

## Why The Crate Exists

It establishes a dedicated package for future language-server integration work without mixing that work into unrelated crates.

## Test

```sh
cargo test -p opencode-lsp
```

There may be zero tests today; that matches the current stub status.
