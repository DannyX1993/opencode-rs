# opencode-storage

SQLite-backed persistence layer for the Rust workspace.

## Status

Active. This crate contains real repositories, migration bootstrap, and the `Storage` trait used by the server.

## Purpose

`opencode-storage` owns the persistent data boundary for the Rust workspace. It exposes a trait-based facade and a concrete SQLite implementation.

## What Exists Today

- `connect(path)` to open a SQLite pool and run migrations
- `Storage` trait used by higher layers
- `StorageImpl` backed by `sqlx`
- repositories for projects, sessions, messages, todos, permissions, and accounts
- append-only sync event storage

## Data Surface

The `Storage` trait currently supports:

- projects
- sessions
- messages and parts
- todos
- permissions
- accounts
- raw sync events

`list_history_with_parts` is the richer message-history API used by the server routes.

## Usage

```rust
use opencode_storage::{connect, StorageImpl};

let pool = connect(std::path::Path::new("opencode.db")).await?;
let storage = StorageImpl::new(pool);
```

## Test

```sh
cargo test -p opencode-storage
```

## Workspace Role

This crate is the persistence backend for the current Rust server. The binary uses it when starting `opencode server`, creating or reusing `./opencode.db` in the current working directory.
