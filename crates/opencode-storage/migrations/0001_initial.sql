-- Migration 0001: Initial schema (compatible with TypeScript SQLite schema)
-- All table/column names must exactly match the TS Drizzle schema.

PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS project (
    id               TEXT PRIMARY KEY,
    worktree         TEXT NOT NULL,
    vcs              TEXT,
    name             TEXT,
    icon_url         TEXT,
    icon_color       TEXT,
    time_created     INTEGER NOT NULL,
    time_updated     INTEGER NOT NULL,
    time_initialized INTEGER,
    sandboxes        TEXT NOT NULL DEFAULT '[]',
    commands         TEXT
);

CREATE TABLE IF NOT EXISTS workspace (
    id         TEXT PRIMARY KEY,
    type       TEXT NOT NULL,
    branch     TEXT,
    name       TEXT,
    directory  TEXT,
    extra      TEXT,
    project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS session (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    workspace_id        TEXT,
    parent_id           TEXT,
    slug                TEXT NOT NULL,
    directory           TEXT NOT NULL,
    title               TEXT NOT NULL,
    version             TEXT NOT NULL,
    share_url           TEXT,
    summary_additions   INTEGER,
    summary_deletions   INTEGER,
    summary_files       INTEGER,
    summary_diffs       TEXT,
    revert              TEXT,
    permission          TEXT,
    time_created        INTEGER NOT NULL,
    time_updated        INTEGER NOT NULL,
    time_compacting     INTEGER,
    time_archived       INTEGER
);

CREATE INDEX IF NOT EXISTS session_project_idx   ON session(project_id);
CREATE INDEX IF NOT EXISTS session_workspace_idx ON session(workspace_id);
CREATE INDEX IF NOT EXISTS session_parent_idx    ON session(parent_id);

CREATE TABLE IF NOT EXISTS message (
    id           TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS message_session_time_created_id_idx
    ON message(session_id, time_created, id);

CREATE TABLE IF NOT EXISTS part (
    id           TEXT PRIMARY KEY,
    message_id   TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    session_id   TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS part_message_id_id_idx ON part(message_id, id);
CREATE INDEX IF NOT EXISTS part_session_idx       ON part(session_id);

CREATE TABLE IF NOT EXISTS todo (
    session_id   TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    content      TEXT NOT NULL,
    status       TEXT NOT NULL,
    priority     TEXT NOT NULL,
    position     INTEGER NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    PRIMARY KEY (session_id, position)
);

CREATE TABLE IF NOT EXISTS permission (
    project_id   TEXT PRIMARY KEY REFERENCES project(id) ON DELETE CASCADE,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session_share (
    session_id   TEXT PRIMARY KEY REFERENCES session(id) ON DELETE CASCADE,
    id           TEXT NOT NULL,
    secret       TEXT NOT NULL,
    url          TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS event_sequence (
    aggregate_id TEXT PRIMARY KEY,
    seq          INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS event (
    id           TEXT PRIMARY KEY,
    aggregate_id TEXT NOT NULL REFERENCES event_sequence(aggregate_id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    type         TEXT NOT NULL,
    data         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS account (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL,
    url           TEXT NOT NULL,
    access_token  TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    token_expiry  INTEGER,
    time_created  INTEGER NOT NULL,
    time_updated  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS account_state (
    id                INTEGER PRIMARY KEY,
    active_account_id TEXT REFERENCES account(id) ON DELETE SET NULL,
    active_org_id     TEXT
);

CREATE TABLE IF NOT EXISTS control_account (
    email         TEXT NOT NULL,
    url           TEXT NOT NULL,
    access_token  TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    token_expiry  INTEGER,
    active        INTEGER NOT NULL DEFAULT 0,
    time_created  INTEGER NOT NULL,
    time_updated  INTEGER NOT NULL,
    PRIMARY KEY (email, url)
);
