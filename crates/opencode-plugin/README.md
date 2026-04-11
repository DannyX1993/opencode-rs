# opencode-plugin

Placeholder crate for future plugin hosting.

## Status

Stub. Package metadata and a minimal library file exist, but there is no real plugin runtime yet.

## Current Reality

- no plugin loading
- no sandbox or ABI surface
- no runtime integration in the binary or server

## Why The Crate Exists

It preserves a separate package boundary for future plugin work instead of forcing that design into `opencode-core` or `opencode-tool` prematurely.

## Test

```sh
cargo test -p opencode-plugin
```
