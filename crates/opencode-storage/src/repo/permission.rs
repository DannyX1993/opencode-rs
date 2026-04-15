//! Permission repository — one row per project.

use opencode_core::{dto::PermissionRow, error::StorageError, id::ProjectId};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

/// Normalized permission rule used by runtime persistence helpers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionRule {
    /// Permission category.
    pub permission: String,
    /// Wildcard pattern associated with the permission.
    pub pattern: String,
    /// Rule action (`allow`, `deny`, or `ask`).
    pub action: String,
}

fn map(e: sqlx::Error) -> StorageError {
    StorageError::Db(e.to_string())
}

fn from_row(r: sqlx::sqlite::SqliteRow) -> Result<PermissionRow, StorageError> {
    let pid: String = r.try_get("project_id").map_err(map)?;
    let data_str: String = r.try_get("data").map_err(map)?;
    Ok(PermissionRow {
        project_id: pid
            .parse()
            .map_err(|e: uuid::Error| StorageError::Db(e.to_string()))?,
        time_created: r.try_get("time_created").map_err(map)?,
        time_updated: r.try_get("time_updated").map_err(map)?,
        data: serde_json::from_str(&data_str).map_err(|e| StorageError::Db(e.to_string()))?,
    })
}

/// Fetch the permission row for `project_id`.
pub async fn get(
    pool: &SqlitePool,
    project_id: ProjectId,
) -> Result<Option<PermissionRow>, StorageError> {
    sqlx::query("SELECT * FROM permission WHERE project_id = ?")
        .bind(project_id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(map)?
        .map(from_row)
        .transpose()
}

/// Insert or update the permission row for a project.
pub async fn set(pool: &SqlitePool, row: &PermissionRow) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO permission (project_id, time_created, time_updated, data) VALUES (?, ?, ?, ?)
         ON CONFLICT(project_id) DO UPDATE SET time_updated = excluded.time_updated, data = excluded.data",
    )
    .bind(row.project_id.to_string())
    .bind(row.time_created)
    .bind(row.time_updated)
    .bind(row.data.to_string())
    .execute(pool)
    .await
    .map_err(map)?;
    Ok(())
}

/// Normalize arbitrary JSON permission data into deduplicated valid rules.
#[must_use]
pub fn normalize_rules(data: &serde_json::Value) -> Vec<PermissionRule> {
    let Some(items) = data.as_array() else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let Some(permission) = item.get("permission").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(pattern) = item.get("pattern").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(action) = item.get("action").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !matches!(action, "allow" | "deny" | "ask") {
            continue;
        }

        let rule = PermissionRule {
            permission: permission.to_string(),
            pattern: pattern.to_string(),
            action: action.to_string(),
        };
        if seen.insert(rule.clone()) {
            out.push(rule);
        }
    }

    out
}

/// Merge durable `allow` rules for `permission` and dedupe resulting rows.
#[must_use]
pub fn merge_allow_rules(
    existing: &serde_json::Value,
    permission: &str,
    patterns: &[String],
) -> serde_json::Value {
    let mut rules = normalize_rules(existing);
    let mut seen: HashSet<PermissionRule> = rules.iter().cloned().collect();

    for pattern in patterns {
        let next = PermissionRule {
            permission: permission.to_string(),
            pattern: pattern.clone(),
            action: "allow".to_string(),
        };
        if seen.insert(next.clone()) {
            rules.push(next);
        }
    }

    serde_json::Value::Array(
        rules
            .into_iter()
            .map(|item| {
                serde_json::json!({
                    "permission": item.permission,
                    "pattern": item.pattern,
                    "action": item.action,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pool::connect, repo::project};
    use opencode_core::{dto::ProjectRow, id::ProjectId};
    use tempfile::NamedTempFile;

    async fn setup() -> (SqlitePool, NamedTempFile, ProjectId) {
        let f = NamedTempFile::new().unwrap();
        let pool = connect(f.path()).await.unwrap();
        let pid = ProjectId::new();
        project::upsert(
            &pool,
            &ProjectRow {
                id: pid,
                worktree: "/tmp".into(),
                vcs: None,
                name: None,
                icon_url: None,
                icon_color: None,
                time_created: 1,
                time_updated: 1,
                time_initialized: None,
                sandboxes: serde_json::json!([]),
                commands: None,
            },
        )
        .await
        .unwrap();
        (pool, f, pid)
    }

    #[tokio::test]
    async fn set_and_get() {
        let (pool, _f, pid) = setup().await;
        let row = PermissionRow {
            project_id: pid,
            time_created: 1,
            time_updated: 1,
            data: serde_json::json!({"allow": ["read"]}),
        };
        set(&pool, &row).await.unwrap();
        let result = get(&pool, pid).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().data["allow"][0], "read");
    }

    #[tokio::test]
    async fn set_upserts_on_conflict() {
        let (pool, _f, pid) = setup().await;
        set(
            &pool,
            &PermissionRow {
                project_id: pid,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({"allow": ["read"]}),
            },
        )
        .await
        .unwrap();
        set(
            &pool,
            &PermissionRow {
                project_id: pid,
                time_created: 1,
                time_updated: 2,
                data: serde_json::json!({"allow": ["read", "write"]}),
            },
        )
        .await
        .unwrap();
        let result = get(&pool, pid).await.unwrap().unwrap();
        assert_eq!(result.data["allow"].as_array().unwrap().len(), 2);
        assert_eq!(result.time_updated, 2);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (pool, _f, _pid) = setup().await;
        let missing = ProjectId::new();
        let result = get(&pool, missing).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn normalize_rules_filters_invalid_entries() {
        let normalized = normalize_rules(&serde_json::json!([
            {"permission": "bash", "pattern": "git:*", "action": "allow"},
            {"permission": "bash", "pattern": "git:*", "action": "allow"},
            {"permission": "bash", "pattern": 12, "action": "allow"},
            {"permission": "read", "pattern": "secret:*", "action": "deny"},
            {"permission": "bash", "pattern": "rm:*", "action": "maybe"}
        ]));

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].permission, "bash");
        assert_eq!(normalized[0].pattern, "git:*");
        assert_eq!(normalized[0].action, "allow");
        assert_eq!(normalized[1].action, "deny");
    }

    #[test]
    fn merge_allow_rules_adds_unique_allow_entries() {
        let merged = merge_allow_rules(
            &serde_json::json!([
                {"permission": "bash", "pattern": "git:status", "action": "allow"},
                {"permission": "bash", "pattern": "rm:*", "action": "deny"}
            ]),
            "bash",
            &["git:status".into(), "git:commit".into()],
        );

        assert_eq!(
            merged,
            serde_json::json!([
                {"permission": "bash", "pattern": "git:status", "action": "allow"},
                {"permission": "bash", "pattern": "rm:*", "action": "deny"},
                {"permission": "bash", "pattern": "git:commit", "action": "allow"}
            ])
        );
    }
}
