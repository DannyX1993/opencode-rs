//! Shared data-transfer objects mirroring the TypeScript SQLite schema.
//!
//! All structs are `serde`-annotated with `snake_case` field names so that
//! they round-trip cleanly with the existing database columns without any
//! explicit `#[serde(rename)]` noise.

use crate::id::{AccountId, MessageId, PartId, ProjectId, SessionId, WorkspaceId};
use crate::project::{RepositoryState, SyncBasis, WorktreeState};
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

/// Durable companion row for canonical repository/worktree foundation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFoundationRow {
    /// Foreign key to `project`.
    pub project_id: ProjectId,
    /// Canonical worktree root, when known.
    #[serde(default)]
    pub canonical_worktree: Option<String>,
    /// Repository root, when known.
    #[serde(default)]
    pub repository_root: Option<String>,
    /// Version-control system kind, when known.
    #[serde(default)]
    pub vcs_kind: Option<String>,
    /// Worktree-local state facts.
    #[serde(default)]
    pub worktree_state: WorktreeState,
    /// Repository-wide durable state facts.
    #[serde(default)]
    pub repository_state: RepositoryState,
    /// Optional sync anchor payload.
    #[serde(default)]
    pub sync_basis: Option<SyncBasis>,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
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
    /// Optional workspace this session belongs to.
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
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
    /// Summary line additions.
    #[serde(default)]
    pub summary_additions: Option<i64>,
    /// Summary line deletions.
    #[serde(default)]
    pub summary_deletions: Option<i64>,
    /// Summary changed file count.
    #[serde(default)]
    pub summary_files: Option<i64>,
    /// Summary diff payload.
    #[serde(default)]
    pub summary_diffs: Option<serde_json::Value>,
    /// Revert metadata payload.
    #[serde(default)]
    pub revert: Option<serde_json::Value>,
    /// Permission mode override.
    #[serde(default)]
    pub permission: Option<serde_json::Value>,
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

/// Request payload for `/api/v1/sessions/:sid/prompt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptRequestDto {
    /// Prompt text to execute.
    pub text: String,
    /// Optional model override.
    #[serde(default)]
    pub model: Option<String>,
    /// Run in plan-only mode.
    #[serde(default)]
    pub plan_mode: bool,
    /// Execute detached and return acceptance metadata immediately.
    #[serde(default)]
    pub detached: bool,
}

/// Detached prompt acceptance payload shared by server and CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetachedPromptDto {
    /// Session id that accepted the detached prompt.
    pub session_id: SessionId,
    /// Assistant message id reserved for this turn, when available.
    #[serde(default)]
    pub assistant_message_id: Option<MessageId>,
    /// Resolved model chosen by runtime, when available.
    #[serde(default)]
    pub resolved_model: Option<String>,
}

/// Prompt submission handle payload for non-detached prompt requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHandleDto {
    /// Session id that accepted the prompt.
    pub session_id: SessionId,
    /// Assistant message id reserved for this turn, when available.
    #[serde(default)]
    pub assistant_message_id: Option<MessageId>,
    /// Resolved model chosen by runtime, when available.
    #[serde(default)]
    pub resolved_model: Option<String>,
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

/// Singleton row from the `account_state` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStateRow {
    /// Singleton row id. Storage currently normalizes this to `1`.
    pub id: i64,
    /// Active account selection.
    #[serde(default)]
    pub active_account_id: Option<AccountId>,
    /// Active organization selection for the active account.
    #[serde(default)]
    pub active_org_id: Option<String>,
}

/// Legacy row from the `control_account` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlAccountRow {
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
    /// Legacy active flag.
    pub active: bool,
    /// Unix timestamp (ms) of creation.
    pub time_created: i64,
    /// Unix timestamp (ms) of last update.
    pub time_updated: i64,
}

/// Response-neutral account descriptor shared across services.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfoDto {
    /// Account identifier.
    pub id: AccountId,
    /// Account email address.
    pub email: String,
    /// Provider portal URL.
    pub url: String,
}

/// Response-neutral active account selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAccountDto {
    /// Active account identifier.
    pub account_id: AccountId,
    /// Active organization identifier, when selected.
    #[serde(default)]
    pub active_org_id: Option<String>,
}

/// Response-neutral organization descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationDto {
    /// Organization identifier.
    pub id: String,
    /// Human-readable organization name.
    pub name: String,
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

// ─── MessageWithParts ─────────────────────────────────────────────────────────

/// Composite DTO: a message row paired with all of its parts.
///
/// This is the honest return shape for history queries — callers always need
/// the parts alongside the message to reconstruct the full conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWithParts {
    /// The message header row.
    pub info: MessageRow,
    /// All parts belonging to this message, in `id` order.
    pub parts: Vec<PartRow>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        id::AccountId,
        project::{RepositoryState, WorktreeState},
    };

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

    #[test]
    fn account_state_row_round_trips_with_active_selection() {
        let row = AccountStateRow {
            id: 1,
            active_account_id: Some(AccountId::new()),
            active_org_id: Some("org-primary".to_string()),
        };
        let json = serde_json::to_value(&row).unwrap();
        let back: AccountStateRow = serde_json::from_value(json).unwrap();
        assert_eq!(row.id, back.id);
        assert_eq!(row.active_account_id, back.active_account_id);
        assert_eq!(row.active_org_id, back.active_org_id);
    }

    #[test]
    fn account_state_row_round_trips_without_active_selection() {
        let row = AccountStateRow {
            id: 1,
            active_account_id: None,
            active_org_id: None,
        };
        let json = serde_json::to_value(&row).unwrap();
        let back: AccountStateRow = serde_json::from_value(json).unwrap();
        assert_eq!(back.active_account_id, None);
        assert_eq!(back.active_org_id, None);
    }

    #[test]
    fn control_account_row_round_trips() {
        let row = ControlAccountRow {
            email: "legacy@example.com".to_string(),
            url: "https://legacy.example.com".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            token_expiry: Some(1_700_000_000_000),
            active: true,
            time_created: 1_700_000_000_001,
            time_updated: 1_700_000_000_002,
        };
        let json = serde_json::to_value(&row).unwrap();
        let back: ControlAccountRow = serde_json::from_value(json).unwrap();
        assert_eq!(row.email, back.email);
        assert!(back.active);
    }

    #[test]
    fn account_info_dto_round_trips() {
        let dto = AccountInfoDto {
            id: AccountId::new(),
            email: "user@example.com".to_string(),
            url: "https://provider.example.com".to_string(),
        };
        let json = serde_json::to_value(&dto).unwrap();
        let back: AccountInfoDto = serde_json::from_value(json).unwrap();
        assert_eq!(dto.id, back.id);
        assert_eq!(dto.email, back.email);
    }

    #[test]
    fn project_foundation_row_round_trips_partial_unknown_fields() {
        let row = ProjectFoundationRow {
            project_id: ProjectId::new(),
            canonical_worktree: Some("/tmp/worktree".into()),
            repository_root: None,
            vcs_kind: None,
            worktree_state: WorktreeState {
                branch: Some("main".into()),
                head_oid: None,
                is_dirty: Some(false),
            },
            repository_state: RepositoryState {
                default_branch: None,
                head_oid: None,
            },
            sync_basis: None,
            time_created: 100,
            time_updated: 200,
        };

        let json = serde_json::to_value(&row).unwrap();
        let back: ProjectFoundationRow = serde_json::from_value(json).unwrap();

        assert_eq!(back.project_id, row.project_id);
        assert_eq!(back.canonical_worktree.as_deref(), Some("/tmp/worktree"));
        assert_eq!(back.vcs_kind, None);
        assert_eq!(back.worktree_state.branch.as_deref(), Some("main"));
        assert_eq!(back.worktree_state.head_oid, None);
        assert_eq!(back.sync_basis, None);
    }

    #[test]
    fn project_foundation_row_serialization_avoids_snapshot_key_names() {
        let row = ProjectFoundationRow {
            project_id: ProjectId::new(),
            canonical_worktree: Some("/tmp/worktree".into()),
            repository_root: Some("/tmp".into()),
            vcs_kind: Some("git".into()),
            worktree_state: WorktreeState::default(),
            repository_state: RepositoryState::default(),
            sync_basis: None,
            time_created: 1,
            time_updated: 2,
        };

        let value = serde_json::to_value(&row).unwrap();
        let obj = value.as_object().unwrap();
        assert!(obj.contains_key("worktree_state"));
        assert!(obj.contains_key("repository_state"));
        assert!(obj.contains_key("sync_basis"));
        assert!(!obj.contains_key("snapshot"));
    }

    #[test]
    fn active_account_and_org_dtos_round_trip() {
        let active = ActiveAccountDto {
            account_id: AccountId::new(),
            active_org_id: Some("org-123".to_string()),
        };
        let org = OrganizationDto {
            id: "org-123".to_string(),
            name: "Primary Org".to_string(),
        };

        let active_back: ActiveAccountDto =
            serde_json::from_value(serde_json::to_value(&active).unwrap()).unwrap();
        let org_back: OrganizationDto =
            serde_json::from_value(serde_json::to_value(&org).unwrap()).unwrap();

        assert_eq!(active.account_id, active_back.account_id);
        assert_eq!(active.active_org_id, active_back.active_org_id);
        assert_eq!(org.name, org_back.name);
    }

    // ── Task 3.1: MessageWithParts composite DTO ─────────────────────────────

    #[test]
    fn message_with_parts_holds_info_and_parts() {
        use crate::id::{MessageId, PartId};
        let sid = SessionId::new();
        let mid = MessageId::new();
        let pid = PartId::new();

        let msg = MessageRow {
            id: mid,
            session_id: sid,
            time_created: 1_000,
            time_updated: 1_001,
            data: serde_json::json!({"role": "user"}),
        };
        let part = PartRow {
            id: pid,
            message_id: mid,
            session_id: sid,
            time_created: 1_000,
            time_updated: 1_001,
            data: serde_json::json!({"type": "text", "text": "hi"}),
        };
        let mwp = MessageWithParts {
            info: msg.clone(),
            parts: vec![part.clone()],
        };

        assert_eq!(mwp.info.id, mid);
        assert_eq!(mwp.parts.len(), 1);
        assert_eq!(mwp.parts[0].id, pid);
        assert_eq!(mwp.parts[0].data["text"], "hi");
    }

    #[test]
    fn message_with_parts_round_trips_json() {
        use crate::id::{MessageId, PartId};
        let sid = SessionId::new();
        let mid = MessageId::new();

        let mwp = MessageWithParts {
            info: MessageRow {
                id: mid,
                session_id: sid,
                time_created: 2_000,
                time_updated: 2_001,
                data: serde_json::json!({"role": "assistant"}),
            },
            parts: vec![PartRow {
                id: PartId::new(),
                message_id: mid,
                session_id: sid,
                time_created: 2_000,
                time_updated: 2_001,
                data: serde_json::json!({"type": "text", "text": "pong"}),
            }],
        };
        let json = serde_json::to_value(&mwp).unwrap();
        let back: MessageWithParts = serde_json::from_value(json).unwrap();
        assert_eq!(back.info.id, mid);
        assert_eq!(back.parts[0].data["text"], "pong");
    }

    #[test]
    fn session_prompt_request_dto_round_trips() {
        let dto = SessionPromptRequestDto {
            text: "hello".to_string(),
            model: Some("gpt-4.1".to_string()),
            plan_mode: true,
            detached: true,
        };
        let json = serde_json::to_value(&dto).unwrap();
        let back: SessionPromptRequestDto = serde_json::from_value(json).unwrap();
        assert_eq!(dto.text, back.text);
        assert_eq!(dto.model, back.model);
        assert_eq!(dto.plan_mode, back.plan_mode);
        assert_eq!(dto.detached, back.detached);
    }

    #[test]
    fn session_detached_prompt_dto_round_trips() {
        let dto = SessionDetachedPromptDto {
            session_id: SessionId::new(),
            assistant_message_id: Some(MessageId::new()),
            resolved_model: Some("claude-sonnet".to_string()),
        };
        let json = serde_json::to_value(&dto).unwrap();
        let back: SessionDetachedPromptDto = serde_json::from_value(json).unwrap();
        assert_eq!(dto.session_id, back.session_id);
        assert_eq!(dto.assistant_message_id, back.assistant_message_id);
        assert_eq!(dto.resolved_model, back.resolved_model);
    }

    #[test]
    fn session_handle_dto_round_trips() {
        let dto = SessionHandleDto {
            session_id: SessionId::new(),
            assistant_message_id: None,
            resolved_model: Some("gpt-4.1-mini".to_string()),
        };
        let json = serde_json::to_value(&dto).unwrap();
        let back: SessionHandleDto = serde_json::from_value(json).unwrap();
        assert_eq!(dto.session_id, back.session_id);
        assert_eq!(dto.assistant_message_id, back.assistant_message_id);
        assert_eq!(dto.resolved_model, back.resolved_model);
    }
}
