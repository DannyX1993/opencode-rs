//! Permission runtime trait and in-memory service implementation.

use crate::types::{PermissionReply, PermissionRequest};
use async_trait::async_trait;
use opencode_bus::{BroadcastBus, BusEvent, EventBus};
use opencode_core::dto::PermissionRow;
use opencode_core::error::SessionError;
use opencode_storage::Storage;
use opencode_storage::repo::permission::{PermissionRule, merge_allow_rules, normalize_rules};
use std::{collections::HashMap, sync::Arc, time::SystemTime};
use tokio::sync::{Mutex, oneshot};

const PERMISSION_REJECTED_MESSAGE: &str = "permission request rejected";

#[derive(Debug)]
struct PendingPermission {
    request: PermissionRequest,
    responder: oneshot::Sender<Result<(), SessionError>>,
}

/// Runtime service responsible for permission ask/reply flows.
#[async_trait]
pub trait PermissionRuntime: Send + Sync {
    /// Register a pending permission request and block until answered.
    async fn ask(&self, req: PermissionRequest) -> Result<(), SessionError>;

    /// Reply to a pending permission request.
    ///
    /// Returns `true` when a pending request was resolved, `false` for unknown ids.
    async fn reply(&self, req: PermissionReply) -> Result<bool, SessionError>;

    /// List all pending permission requests.
    async fn list(&self) -> Result<Vec<PermissionRequest>, SessionError>;
}

/// In-memory permission runtime backed by pending waiters and project storage.
pub struct InMemoryPermissionRuntime {
    /// Durable permission rule storage.
    pub storage: Arc<dyn Storage>,
    /// Bus publisher for permission lifecycle events.
    pub bus: Arc<BroadcastBus>,
    pending: Mutex<HashMap<String, PendingPermission>>,
}

impl InMemoryPermissionRuntime {
    /// Build a new permission runtime service.
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>, bus: Arc<BroadcastBus>) -> Self {
        Self {
            storage,
            bus,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl PermissionRuntime for InMemoryPermissionRuntime {
    async fn ask(&self, req: PermissionRequest) -> Result<(), SessionError> {
        if self.request_is_already_allowed(&req).await? {
            return Ok(());
        }

        let request_id = req.id.clone();
        let session_id = req.session_id;
        let permission = req.permission.clone();
        let patterns = req.patterns.clone();
        let metadata = req.metadata.clone();
        let always = req.always.clone();
        let tool = req.tool.clone();

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(
            request_id.clone(),
            PendingPermission {
                request: req,
                responder: tx,
            },
        );

        // Event publication is best-effort; no receivers is non-fatal.
        let _ = self.bus.publish(BusEvent::PermissionAsked {
            session_id,
            request_id,
            permission,
            patterns,
            metadata,
            always,
            tool: tool.map(to_bus_tool_call_ref),
        });

        match rx.await {
            Ok(result) => result,
            Err(_closed) => Err(SessionError::RuntimeInternal(
                "permission waiter dropped before reply".into(),
            )),
        }
    }

    async fn reply(&self, req: PermissionReply) -> Result<bool, SessionError> {
        let reply_kind = req.reply;
        let pending = {
            let mut pending_map = self.pending.lock().await;
            pending_map.remove(&req.request_id)
        };

        let Some(pending) = pending else {
            return Ok(false);
        };

        if pending.request.session_id != req.session_id {
            let mut pending_map = self.pending.lock().await;
            pending_map.insert(req.request_id.clone(), pending);
            return Ok(false);
        }

        match reply_kind {
            crate::types::PermissionReplyKind::Once => {
                let _ = pending.responder.send(Ok(()));
            }
            crate::types::PermissionReplyKind::Always => {
                self.persist_always_rules(&pending.request).await?;
                let allow_rules = self
                    .load_project_allow_rules(pending.request.session_id)
                    .await?;

                let mut responders = vec![pending.responder];
                let mut pending_map = self.pending.lock().await;
                let covered_ids: Vec<String> = pending_map
                    .iter()
                    .filter_map(|(id, item)| {
                        if item.request.session_id == pending.request.session_id
                            && request_is_fully_allowed(&item.request, &allow_rules)
                        {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for id in covered_ids {
                    if let Some(other) = pending_map.remove(&id) {
                        responders.push(other.responder);
                    }
                }
                drop(pending_map);

                for responder in responders {
                    let _ = responder.send(Ok(()));
                }
            }
            crate::types::PermissionReplyKind::Reject => {
                let mut responders = vec![pending.responder];
                let mut pending_map = self.pending.lock().await;
                let same_session_ids: Vec<String> = pending_map
                    .iter()
                    .filter(|(_, item)| item.request.session_id == pending.request.session_id)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in same_session_ids {
                    if let Some(other) = pending_map.remove(&id) {
                        responders.push(other.responder);
                    }
                }
                drop(pending_map);

                for responder in responders {
                    let _ = responder.send(Err(SessionError::RuntimeInternal(
                        PERMISSION_REJECTED_MESSAGE.into(),
                    )));
                }
            }
        }

        let _ = self.bus.publish(BusEvent::PermissionReplied {
            session_id: req.session_id,
            request_id: req.request_id,
            reply: to_bus_permission_reply_kind(reply_kind),
        });

        Ok(true)
    }

    async fn list(&self) -> Result<Vec<PermissionRequest>, SessionError> {
        let pending = self.pending.lock().await;
        Ok(pending.values().map(|item| item.request.clone()).collect())
    }
}

impl InMemoryPermissionRuntime {
    async fn request_is_already_allowed(
        &self,
        request: &PermissionRequest,
    ) -> Result<bool, SessionError> {
        let allow_rules = self.load_project_allow_rules(request.session_id).await?;
        Ok(request_is_fully_allowed(request, &allow_rules))
    }

    async fn load_project_allow_rules(
        &self,
        session_id: opencode_core::id::SessionId,
    ) -> Result<Vec<PermissionRule>, SessionError> {
        let Some(session) = self
            .storage
            .get_session(session_id)
            .await
            .map_err(storage_to_session_error)?
        else {
            return Err(SessionError::NotFound(session_id.to_string()));
        };

        let row = self
            .storage
            .get_permission(session.project_id)
            .await
            .map_err(storage_to_session_error)?;

        let Some(row) = row else {
            return Ok(Vec::new());
        };

        Ok(normalize_rules(&row.data)
            .into_iter()
            .filter(|rule| rule.action == "allow")
            .collect())
    }

    async fn persist_always_rules(&self, request: &PermissionRequest) -> Result<(), SessionError> {
        let Some(session) = self
            .storage
            .get_session(request.session_id)
            .await
            .map_err(storage_to_session_error)?
        else {
            return Err(SessionError::NotFound(request.session_id.to_string()));
        };

        let existing = self
            .storage
            .get_permission(session.project_id)
            .await
            .map_err(storage_to_session_error)?;
        let now = now_millis();

        let patterns: Vec<String> = if request.always.is_empty() {
            request.patterns.clone()
        } else {
            request.always.clone()
        };

        let (time_created, existing_data) = existing
            .map(|row| (row.time_created, row.data))
            .unwrap_or((now, serde_json::json!([])));
        let merged = merge_allow_rules(&existing_data, &request.permission, &patterns);

        self.storage
            .set_permission(PermissionRow {
                project_id: session.project_id,
                time_created,
                time_updated: now,
                data: merged,
            })
            .await
            .map_err(storage_to_session_error)
    }
}

fn storage_to_session_error(error: opencode_core::error::StorageError) -> SessionError {
    SessionError::RuntimeInternal(format!("permission storage failure: {error}"))
}

fn now_millis() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
}

fn to_bus_permission_reply_kind(
    reply: crate::types::PermissionReplyKind,
) -> opencode_bus::PermissionReplyKind {
    match reply {
        crate::types::PermissionReplyKind::Once => opencode_bus::PermissionReplyKind::Once,
        crate::types::PermissionReplyKind::Always => opencode_bus::PermissionReplyKind::Always,
        crate::types::PermissionReplyKind::Reject => opencode_bus::PermissionReplyKind::Reject,
    }
}

fn to_bus_tool_call_ref(
    tool: crate::types::RuntimeToolCallRef,
) -> opencode_bus::RuntimeToolCallRef {
    opencode_bus::RuntimeToolCallRef {
        message_id: tool.message_id,
        call_id: tool.call_id,
    }
}

fn request_is_fully_allowed(request: &PermissionRequest, allow_rules: &[PermissionRule]) -> bool {
    request.patterns.iter().all(|pattern| {
        allow_rules.iter().any(|rule| {
            rule.permission == request.permission && wildcard_matches(&rule.pattern, pattern)
        })
    })
}

fn wildcard_matches(rule_pattern: &str, requested_pattern: &str) -> bool {
    if rule_pattern == requested_pattern || rule_pattern == "*" {
        return true;
    }

    if !rule_pattern.contains('*') {
        return false;
    }

    let mut remainder = requested_pattern;
    let mut first = true;
    for segment in rule_pattern
        .split('*')
        .filter(|segment| !segment.is_empty())
    {
        if first {
            if !remainder.starts_with(segment) {
                return false;
            }
            remainder = &remainder[segment.len()..];
            first = false;
            continue;
        }

        let Some(position) = remainder.find(segment) else {
            return false;
        };
        remainder = &remainder[position + segment.len()..];
    }

    if !rule_pattern.ends_with('*')
        && let Some(last_segment) = rule_pattern.split('*').rfind(|segment| !segment.is_empty())
    {
        return requested_pattern.ends_with(last_segment);
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PermissionReplyKind;
    use opencode_bus::BusEvent;
    use opencode_core::{
        dto::{ProjectRow, SessionRow},
        id::{MessageId, ProjectId, SessionId},
    };
    use opencode_storage::{StorageImpl, connect};
    use tempfile::NamedTempFile;
    use tokio::time::{Duration, sleep, timeout};

    async fn make_storage() -> (Arc<dyn Storage>, NamedTempFile) {
        let file = NamedTempFile::new().expect("tempfile");
        let pool = connect(file.path()).await.expect("connect sqlite");
        let storage: Arc<dyn Storage> = Arc::new(StorageImpl::new(pool));
        (storage, file)
    }

    fn project_row(id: ProjectId) -> ProjectRow {
        ProjectRow {
            id,
            worktree: "/tmp".into(),
            vcs: None,
            name: None,
            icon_url: None,
            icon_color: None,
            time_created: 0,
            time_updated: 0,
            time_initialized: None,
            sandboxes: serde_json::json!([]),
            commands: None,
        }
    }

    fn session_row(session_id: SessionId, project_id: ProjectId) -> SessionRow {
        SessionRow {
            id: session_id,
            project_id,
            workspace_id: None,
            parent_id: None,
            slug: "s".into(),
            directory: "/tmp".into(),
            title: "t".into(),
            version: "1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
        }
    }

    fn permission_request(session_id: SessionId, id: &str, patterns: &[&str]) -> PermissionRequest {
        PermissionRequest {
            id: id.into(),
            session_id,
            permission: "bash".into(),
            patterns: patterns.iter().map(|p| (*p).to_string()).collect(),
            metadata: serde_json::json!({"source": "test"}),
            always: patterns.iter().map(|p| (*p).to_string()).collect(),
            tool: None,
        }
    }

    async fn wait_for_pending_count(runtime: &InMemoryPermissionRuntime, expected: usize) {
        for _ in 0..50 {
            if runtime.list().await.expect("list pending").len() == expected {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for pending count {expected}");
    }

    #[tokio::test]
    async fn ask_blocks_and_once_reply_resumes_and_clears_pending() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .expect("insert project");
        let session_id = SessionId::new();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .expect("insert session");

        let runtime = Arc::new(InMemoryPermissionRuntime::new(
            storage,
            Arc::new(BroadcastBus::default_capacity()),
        ));
        let request = permission_request(session_id, "req-once", &["git:status"]);

        let ask_runtime = Arc::clone(&runtime);
        let ask_handle = tokio::spawn(async move { ask_runtime.ask(request).await });

        wait_for_pending_count(&runtime, 1).await;
        assert_eq!(runtime.list().await.expect("list").len(), 1);

        let resolved = runtime
            .reply(PermissionReply {
                session_id,
                request_id: "req-once".into(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .expect("reply once");
        assert!(resolved);

        timeout(Duration::from_secs(1), ask_handle)
            .await
            .expect("ask should complete")
            .expect("join")
            .expect("ask should succeed");
        assert!(runtime.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn reply_unknown_request_is_noop() {
        let (storage, _file) = make_storage().await;
        let runtime =
            InMemoryPermissionRuntime::new(storage, Arc::new(BroadcastBus::default_capacity()));
        let session_id = SessionId::new();

        let resolved = runtime
            .reply(PermissionReply {
                session_id,
                request_id: "missing".into(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .expect("unknown reply should not fail");

        assert!(!resolved);
        assert!(runtime.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn always_reply_persists_rules_and_auto_unblocks_same_session() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .expect("insert project");
        let session_id = SessionId::new();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .expect("insert session");

        let runtime = Arc::new(InMemoryPermissionRuntime::new(
            Arc::clone(&storage),
            Arc::new(BroadcastBus::default_capacity()),
        ));

        let ask_a = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move {
                runtime
                    .ask(permission_request(
                        session_id,
                        "req-always-a",
                        &["git:status"],
                    ))
                    .await
            })
        };
        let ask_b = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move {
                runtime
                    .ask(permission_request(
                        session_id,
                        "req-always-b",
                        &["git:status"],
                    ))
                    .await
            })
        };

        wait_for_pending_count(&runtime, 2).await;

        let resolved = runtime
            .reply(PermissionReply {
                session_id,
                request_id: "req-always-a".into(),
                reply: PermissionReplyKind::Always,
            })
            .await
            .expect("always reply");
        assert!(resolved);

        timeout(Duration::from_secs(1), ask_a)
            .await
            .expect("ask A should complete")
            .expect("join A")
            .expect("ask A success");
        timeout(Duration::from_secs(1), ask_b)
            .await
            .expect("ask B should complete")
            .expect("join B")
            .expect("ask B success");

        let stored = storage
            .get_permission(project_id)
            .await
            .expect("load permission row")
            .expect("permission row should exist");
        assert!(
            opencode_storage::repo::permission::normalize_rules(&stored.data)
                .iter()
                .any(|rule| {
                    rule.permission == "bash"
                        && rule.pattern == "git:status"
                        && rule.action == "allow"
                }),
            "expected durable allow rule for git:status"
        );

        let later_session = SessionId::new();
        storage
            .create_session(session_row(later_session, project_id))
            .await
            .expect("insert later session");

        runtime
            .ask(permission_request(
                later_session,
                "req-covered-by-durable",
                &["git:status"],
            ))
            .await
            .expect("durable allow should skip pending ask");
        assert!(runtime.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn reject_reply_rejects_all_pending_requests_in_same_session() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .expect("insert project");
        let session_id = SessionId::new();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .expect("insert session");

        let runtime = Arc::new(InMemoryPermissionRuntime::new(
            storage,
            Arc::new(BroadcastBus::default_capacity()),
        ));

        let ask_a = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move {
                runtime
                    .ask(permission_request(session_id, "req-reject-a", &["rm:*"]))
                    .await
            })
        };
        let ask_b = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move {
                runtime
                    .ask(permission_request(
                        session_id,
                        "req-reject-b",
                        &["git:push"],
                    ))
                    .await
            })
        };

        wait_for_pending_count(&runtime, 2).await;

        let resolved = runtime
            .reply(PermissionReply {
                session_id,
                request_id: "req-reject-a".into(),
                reply: PermissionReplyKind::Reject,
            })
            .await
            .expect("reject reply");
        assert!(resolved);

        let err_a = timeout(Duration::from_secs(1), ask_a)
            .await
            .expect("ask A should finish")
            .expect("join A")
            .expect_err("ask A should be rejected");
        let err_b = timeout(Duration::from_secs(1), ask_b)
            .await
            .expect("ask B should finish")
            .expect("join B")
            .expect_err("ask B should be rejected");

        assert!(err_a.to_string().contains("rejected"));
        assert!(err_b.to_string().contains("rejected"));
        assert!(runtime.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn ask_and_reply_publish_events_with_tool_linkage() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .expect("insert project");
        let session_id = SessionId::new();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .expect("insert session");

        let bus = Arc::new(BroadcastBus::default_capacity());
        let mut rx = bus.subscribe();
        let runtime = Arc::new(InMemoryPermissionRuntime::new(storage, Arc::clone(&bus)));

        let request = PermissionRequest {
            id: "req-events".into(),
            session_id,
            permission: "bash".into(),
            patterns: vec!["git:status".into()],
            metadata: serde_json::json!({"source": "tool-loop"}),
            always: vec!["git:status".into()],
            tool: Some(crate::types::RuntimeToolCallRef {
                message_id: MessageId::new(),
                call_id: "call-events-1".into(),
            }),
        };

        let ask_handle = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move { runtime.ask(request).await })
        };

        wait_for_pending_count(&runtime, 1).await;
        let asked_event = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("asked event should arrive")
            .expect("bus should decode asked event");
        assert!(matches!(
            asked_event,
            BusEvent::PermissionAsked {
                session_id: sid,
                request_id,
                tool: Some(_),
                ..
            } if sid == session_id && request_id == "req-events"
        ));

        runtime
            .reply(PermissionReply {
                session_id,
                request_id: "req-events".into(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .expect("reply should succeed");

        let replied_event = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("replied event should arrive")
            .expect("bus should decode replied event");
        assert!(matches!(
            replied_event,
            BusEvent::PermissionReplied {
                session_id: sid,
                request_id,
                reply: opencode_bus::PermissionReplyKind::Once,
            } if sid == session_id && request_id == "req-events"
        ));

        timeout(Duration::from_secs(1), ask_handle)
            .await
            .expect("ask should complete")
            .expect("join ask task")
            .expect("ask should succeed");
    }
}
