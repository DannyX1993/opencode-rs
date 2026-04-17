# opencode-server/src/routes

HTTP route modules for the Rust server surface.

## Modules

- `project.rs`
  - project CRUD handlers
  - additive project foundation persistence during upsert
  - git probe + unknown/partial fallback handling
- `session.rs`
  - legacy and singular session parity routes
  - prompt/cancel/status aliases and regression behavior guards
- `permission.rs`
  - pending permission list + reply
- `question.rs`
  - pending question list + reply/reject
- `event.rs`
  - live SSE endpoint with heartbeat + bus translation
- `provider.rs`
  - provider catalog/auth/account routes + manual stream harness
- `config.rs`
  - local/global config read/patch + provider config projection

## Project foundation route notes

`project.rs` now treats foundation capture as an additive companion write.

- Primary project row remains the compatibility anchor.
- Companion foundation row stores canonical worktree/repository state for later orchestration use.
- `RepositoryProbe` is backend-agnostic by contract.
- Current implementation uses CLI git probing and gracefully degrades to unknown fields.

Fallback matrix:

- git success: persist canonical + repository metadata
- non-git path: persist canonical (if resolvable) with unknown repository fields
- git unavailable/error: same fallback as non-git

## Boundaries

- Routes should stay adapter-thin.
- Storage schema evolution belongs in `opencode-storage`.
- Shared types belong in `opencode-core`.
- Session runtime occupancy semantics (`RunSnapshot`) belong in `opencode-session`.

## Testing expectations

Route tests should cover both compatibility and foundation behavior:

- existing response contracts unchanged
- foundation companion persistence for git/non-git/partial metadata
- no prompt/history/session behavior drift when foundation metadata exists
