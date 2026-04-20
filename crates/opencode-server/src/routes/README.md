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

## CLI/backend parity contracts

`opencode-cli` core command slices in `v0.14.0` intentionally consume these route contracts for deterministic behavior:

- `providers list` → `GET /api/v1/provider`
- `session list` and one-shot `run`/`prompt` project resolution → `GET /api/v1/projects`
- `session list` and one-shot `run`/`prompt` session lookup → `GET /api/v1/projects/:project_id/sessions`
- one-shot `run`/`prompt` session creation (when missing) → `POST /api/v1/projects/:project_id/sessions`
- one-shot `run`/`prompt` detached acceptance → `POST /api/v1/sessions/:session_id/prompt` (`detached=true`)

This route-level reuse keeps command semantics backend-aligned instead of introducing separate CLI-only behavior.

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
