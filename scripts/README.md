# scripts/

Helper scripts for the Rust workspace.

## Files

| File | Purpose |
| --- | --- |
| [`coverage.sh`](coverage.sh) | Runs `cargo llvm-cov` in a few common modes |

## `coverage.sh`

Requirements:

- `cargo llvm-cov` installed
- run from anywhere; the script resolves the workspace root itself

Examples:

```sh
./scripts/coverage.sh
./scripts/coverage.sh --html
./scripts/coverage.sh --check
```

Behavior:

- default mode generates `target/coverage/lcov.info` and prints a summary
- `--html` writes `target/coverage/html/`
- `--check` fails if line or region coverage drops below `85%`

These scripts are workspace helpers only. They do not replace `cargo test --workspace`.
