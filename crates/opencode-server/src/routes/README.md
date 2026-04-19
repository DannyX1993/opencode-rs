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
- `workspace.rs`
  - workspace CRUD endpoints
  - remote-target metadata validation (`instance`, `base_url`) for control-plane routing

## Workspace + control-plane route notes

- `workspace.rs` validates `remote` workspace payloads strictly before persistence.
- `remote` workspaces require:
  - `extra.instance` (non-empty string)
  - `extra.base_url` (absolute `http(s)` URL)
- Invalid metadata maps to explicit `400` responses instead of late proxy failures.

These guarantees are consumed by control-plane middleware when resolving local-vs-forward routing.

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
