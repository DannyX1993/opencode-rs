# docs/

Project documentation for the Rust workspace.

## Contents

| File | Purpose |
| --- | --- |
| [`MANUAL_TESTING.md`](MANUAL_TESTING.md) | Manual validation of the provider streaming harness |

## What Belongs Here

- Testing guides that describe how to exercise behavior outside unit tests
- Workspace-level notes that should stay accurate even as implementation changes
- Documentation that helps contributors understand how the Rust workspace fits together

## Current Focus

The most concrete operational guide today is the provider harness manual test flow. That reflects the current codebase: the HTTP server, provider adapters, and storage layer exist, while the full interactive session loop is still incomplete.

## How To Verify Docs Against Code

Useful commands from the workspace root:

```sh
cargo test -p opencode-server -p opencode-provider
cargo run -p opencode -- server --port 4141
```

If documentation here mentions HTTP routes or runtime behavior, it should match the Rust code under `crates/opencode-server/`, not the TypeScript implementation.
