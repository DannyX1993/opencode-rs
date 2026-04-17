-- Migration 0002: project repository/worktree companion state
-- Additive migration; leaves 0001 schema untouched.

CREATE TABLE IF NOT EXISTS project_repository_state (
    project_id          TEXT PRIMARY KEY REFERENCES project(id) ON DELETE CASCADE,
    canonical_worktree  TEXT,
    repository_root     TEXT,
    vcs_kind            TEXT,
    worktree_state      TEXT,
    repository_state    TEXT,
    sync_basis          TEXT,
    time_created        INTEGER NOT NULL,
    time_updated        INTEGER NOT NULL
);
