# opencode-tui

Placeholder crate for the future terminal UI.

## Status

Stub. The crate currently contains only minimal scaffolding and is not used to power the `run` command yet.

## Current Reality

- no ratatui application implementation yet
- no event loop or rendering surface yet
- the binary's `run` command still logs a stub message instead of using this crate

## Why The Crate Exists

It keeps the eventual TUI boundary explicit without forcing terminal concerns into the binary crate prematurely.

## Test

```sh
cargo test -p opencode-tui
```
