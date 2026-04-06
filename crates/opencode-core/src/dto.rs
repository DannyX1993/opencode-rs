//! Shared data-transfer objects mirroring the TypeScript SQLite schema.
//!
//! All structs are `serde`-annotated with `snake_case` field names so that
//! they round-trip cleanly with the existing database columns without any
//! explicit `#[serde(rename)]` noise.

use crate::id::{AccountId, MessageId, PartId, ProjectId, SessionId, WorkspaceId};
use serde::{Deserialize, Serialize};

// ─── Project ─────────────────────────────────────────────────────────────────

/// Row from the `project` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRow {
    /// Primary key.
    pub id: ProjectId,
    /// Absolute path to the project worktree.
    pub worktree: String,
    /// VCS type (e.g. "git").
    #[serde(default)]
    pub vcs: Option<String>,
    /// Human-readable project name.
    #[serde(default)]
    pub name: Option<String>,
    /// Icon URL for display.
    #[serde(default)]
    pub icon_url: Option<String>,
    /// Icon accent colour (hex).
    #[serde(default)]
    pub icon_color: Option<String>,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
    /// Unix timestamp (ms) when project was first initialized.
    #[serde(default)]
    pub time_initialized: Option<i64>,
    /// JSON blob: sandbox configuration.
    #[serde(default)]
    pub sandboxes: serde_json::Value,
    /// JSON blob: custom commands.
    #[serde(default)]
    pub commands: Option<serde_json::Value>,
}

// ─── Workspace ───────────────────────────────────────────────────────────────

/// Row from the `workspace` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRow {
    /// Primary key.
    pub id: WorkspaceId,
    /// Type identifier (e.g. "local", "remote").
    pub r#type: String,
    /// Git branch name (if applicable).
    #[serde(default)]
    pub branch: Option<String>,
    /// Human-readable name.
    #[serde(default)]
    pub name: Option<String>,
    /// Working directory.
    #[serde(default)]
    pub directory: Option<String>,
    /// Extra JSON metadata.
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
    /// Foreign key to `project`.
    pub project_id: ProjectId,
}

// ─── Session ─────────────────────────────────────────────────────────────────

/// Row from the `session` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    /// Primary key.
    pub id: SessionId,
    /// Foreign key to `project`.
    pub project_id: ProjectId,
    /// Optional parent session (sub-agent chains).
    #[serde(default)]
    pub parent_id: Option<SessionId>,
    /// URL-friendly slug.
    pub slug: String,
    /// Working directory for this session.
    pub directory: String,
    /// Session title (user-visible).
    pub title: String,
    /// Schema version of this session.
    pub version: String,
    /// Shareable URL (if enabled).
    #[serde(default)]
    pub share_url: Option<String>,
    /// Permission mode override.
    #[serde(default)]
    pub permission: Option<String>,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
    /// Unix timestamp (ms) when compaction started.
    #[serde(default)]
    pub time_compacting: Option<i64>,
    /// Unix timestamp (ms) when session was archived.
    #[serde(default)]
    pub time_archived: Option<i64>,
}

// ─── Message ─────────────────────────────────────────────────────────────────

/// Row from the `message` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    /// Primary key.
    pub id: MessageId,
    /// Foreign key to `session`.
    pub session_id: SessionId,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
    /// JSON-encoded message payload.
    pub data: serde_json::Value,
}

// ─── Part ────────────────────────────────────────────────────────────────────

/// Row from the `part` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartRow {
    /// Primary key.
    pub id: PartId,
    /// Foreign key to `message`.
    pub message_id: MessageId,
    /// Denormalized session reference (for efficient queries).
    pub session_id: SessionId,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
    /// JSON-encoded part payload.
    pub data: serde_json::Value,
}

// ─── Todo ────────────────────────────────────────────────────────────────────

/// Row from the `todo` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoRow {
    /// Foreign key to `session`.
    pub session_id: SessionId,
    /// Todo item text content.
    pub content: String,
    /// Status: "pending" | "in-progress" | "done".
    pub status: String,
    /// Priority: "low" | "medium" | "high".
    pub priority: String,
    /// Ordering position (0-based).
    pub position: i64,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
}

// ─── Account ─────────────────────────────────────────────────────────────────

/// Row from the `account` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRow {
    /// Primary key.
    pub id: AccountId,
    /// Account email address.
    pub email: String,
    /// Provider portal URL.
    pub url: String,
    /// OAuth access token.
    pub access_token: String,
    /// OAuth refresh token.
    pub refresh_token: String,
    /// Token expiry unix timestamp (ms).
    #[serde(default)]
    pub token_expiry: Option<i64>,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
}

// ─── Permission ──────────────────────────────────────────────────────────────

/// Row from the `permission` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRow {
    /// Foreign key to `project`.
    pub project_id: ProjectId,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
    /// JSON blob containing the permission ruleset.
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_row_round_trips() {
        let row = ProjectRow {
            id: ProjectId::new(),
            worktree: "/home/user/project".to_string(),
            vcs: Some("git".to_string()),
            name: Some("My Project".to_string()),
            icon_url: None,
            icon_color: None,
            time_created: 1_700_000_000_000,
            time_updated: 1_700_000_000_001,
            time_initialized: None,
            sandboxes: serde_json::json!([]),
            commands: None,
        };
        let json = serde_json::to_value(&row).unwrap();
        let back: ProjectRow = serde_json::from_value(json).unwrap();
        assert_eq!(row.id, back.id);
        assert_eq!(row.worktree, back.worktree);
    }

    #[test]
    fn todo_row_round_trips() {
        let row = TodoRow {
            session_id: SessionId::new(),
            content: "implement the feature".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
            position: 0,
            time_created: 1_700_000_000_000,
            time_updated: 1_700_000_000_001,
        };
        let json = serde_json::to_value(&row).unwrap();
        let back: TodoRow = serde_json::from_value(json).unwrap();
        assert_eq!(row.content, back.content);
    }
}
