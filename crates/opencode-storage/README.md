# opencode-storage

SQLite storage layer for `opencode-rs` using `sqlx`.

> **Status**: ✅ Implemented — schema migrations, session/message/todo/permission/account CRUD.

---

## Purpose

`opencode-storage` provides persistent storage for sessions, messages, todos,
permissions, and accounts. It uses SQLite via `sqlx` with async runtime support.

---

## Usage

```rust
use opencode_storage::{StorageImpl, connect};
use std::path::Path;

let pool = connect(Path::new("opencode.db")).await?;
let storage = StorageImpl::new(pool);
```

`connect()` runs migrations automatically on first connection.

---

## Storage Operations

The `Storage` trait provides:

- **Projects**: create, read, list, delete
- **Sessions**: create, read, update, list, delete
- **Messages**: append, list by session
- **Todos**: create, read, update, delete
- **Permissions**: create, read, list, delete
- **Accounts**: create, read, list, delete
- **Events**: emit, subscribe (via event bus integration)

All operations are async and return `Result<T, StorageError>`.

> **Note**: Artifact or blob storage is not yet implemented. The `Storage` trait
> covers structured relational records only.
