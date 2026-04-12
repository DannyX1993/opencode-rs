//! Runtime stream processing primitives for session prompt execution.

use futures::StreamExt;
use opencode_bus::{BroadcastBus, BusEvent, EventBus};
use opencode_core::{
    context::CancellationToken,
    dto::PartRow,
    error::SessionError,
    id::{MessageId, PartId, SessionId},
};
use opencode_provider::{ModelEvent, ProviderError};
use opencode_storage::Storage;
use std::{sync::Arc, time::SystemTime};

/// Lightweight sink placeholder for lifecycle stream events.
///
/// Full provider-event to storage projection is implemented in later tasks.
#[derive(Debug, Default)]
pub struct RuntimeEventSink;

/// Runtime collaborators required to project stream events into persistence and
/// bus lifecycle updates.
pub struct RuntimeStreamContext {
    /// Session under execution.
    pub session_id: SessionId,
    /// Assistant message shell created by `SessionEngine::prompt`.
    pub assistant_message_id: MessageId,
    /// Storage writer used for incremental assistant part persistence.
    pub storage: Arc<dyn Storage>,
    /// Bus publisher used for lifecycle and progress events.
    pub bus: Arc<BroadcastBus>,
    /// Provider id selected for the active turn.
    pub provider_id: String,
    /// Model id selected for the active turn.
    pub model_id: String,
}

/// Consume provider stream until completion or cancellation.
///
/// This phase only drives control-flow lifecycle. Event-to-storage projection is
/// implemented in the next runtime batch.
pub async fn run_prompt_stream(
    ctx: RuntimeStreamContext,
    mut stream: opencode_core::context::BoxStream<Result<ModelEvent, ProviderError>>,
    cancel: CancellationToken,
) -> Result<(), SessionError> {
    let _ = ctx.bus.publish(BusEvent::SessionUpdated {
        session_id: ctx.session_id,
    });

    let mut part_sequence: i64 = 0;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = ctx.bus.publish(BusEvent::SessionCancelled { session_id: ctx.session_id });
                return Err(SessionError::Cancelled);
            }
            maybe_event = stream.next() => {
                match maybe_event {
                    Some(Ok(ModelEvent::TextDelta { delta })) => {
                        part_sequence += 1;
                        let time = now_millis() + part_sequence;
                        let part = PartRow {
                            id: PartId::new(),
                            message_id: ctx.assistant_message_id,
                            session_id: ctx.session_id,
                            time_created: time,
                            time_updated: time,
                            data: serde_json::json!({"type": "text", "text": delta, "seq": part_sequence}),
                        };
                        let part_id = part.id;
                        ctx.storage
                            .append_part(part)
                            .await
                            .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?;
                        let _ = ctx.bus.publish(BusEvent::PartAdded {
                            session_id: ctx.session_id,
                            message_id: ctx.assistant_message_id,
                            part_id,
                        });
                    }
                    Some(Ok(ModelEvent::Usage { input, output, .. })) => {
                        let _ = ctx.bus.publish(BusEvent::ProviderTokensUsed {
                            session_id: ctx.session_id,
                            provider: ctx.provider_id.clone(),
                            model: ctx.model_id.clone(),
                            input,
                            output,
                        });
                    }
                    Some(Ok(ModelEvent::Done { .. })) => {
                        let _ = ctx.bus.publish(BusEvent::SessionCompleted { session_id: ctx.session_id });
                        return Ok(());
                    }
                    Some(Ok(
                        ModelEvent::ToolUseStart { .. }
                        | ModelEvent::ToolUseInputDelta { .. }
                        | ModelEvent::ToolUseEnd { .. },
                    )) => {
                        // Deferred by runtime-core scope: tool-use event execution will be
                        // implemented in a follow-up slice with opencode-tool integration.
                        return Err(SessionError::RuntimeInternal(
                            "tool-use stream events are unsupported in runtime-core slice".into(),
                        ));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(SessionError::Provider(err.to_string())),
                    None => {
                        let _ = ctx.bus.publish(BusEvent::SessionCompleted { session_id: ctx.session_id });
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use opencode_bus::{BroadcastBus, BusEvent, EventBus};
    use opencode_core::{
        dto::{MessageRow, ProjectRow, SessionRow},
        id::{MessageId, ProjectId, SessionId},
    };
    use opencode_storage::{Storage, StorageImpl, connect};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    async fn make_storage() -> (Arc<dyn Storage>, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let pool = connect(file.path()).await.unwrap();
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

    fn assistant_shell(session_id: SessionId, message_id: MessageId, now: i64) -> MessageRow {
        MessageRow {
            id: message_id,
            session_id,
            time_created: now,
            time_updated: now,
            data: serde_json::json!({"role": "assistant"}),
        }
    }

    #[tokio::test]
    async fn stream_persists_text_deltas_in_arrival_order_and_publishes_lifecycle() {
        let (storage, _file) = make_storage().await;
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();
        storage.upsert_project(project_row(pid)).await.unwrap();
        storage.create_session(session_row(sid, pid)).await.unwrap();
        storage
            .append_message(assistant_shell(sid, mid, 1), Vec::new())
            .await
            .unwrap();

        let bus = Arc::new(BroadcastBus::default_capacity());
        let mut rx = bus.subscribe();

        let stream = Box::pin(stream::iter([
            Ok(ModelEvent::TextDelta {
                delta: "hello".into(),
            }),
            Ok(ModelEvent::TextDelta {
                delta: " world".into(),
            }),
            Ok(ModelEvent::Done {
                reason: "stop".into(),
            }),
        ]));

        run_prompt_stream(
            RuntimeStreamContext {
                session_id: sid,
                assistant_message_id: mid,
                storage: Arc::clone(&storage),
                bus: Arc::clone(&bus),
                provider_id: "stub".into(),
                model_id: "test-model".into(),
            },
            stream,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        let history = storage.list_history_with_parts(sid).await.unwrap();
        let assistant = &history[0];
        assert_eq!(assistant.parts.len(), 2);
        assert_eq!(assistant.parts[0].data["text"], "hello");
        assert_eq!(assistant.parts[1].data["text"], " world");

        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        let third = rx.recv().await.unwrap();
        let fourth = rx.recv().await.unwrap();
        assert!(matches!(first, BusEvent::SessionUpdated { session_id } if session_id == sid));
        assert!(
            matches!(second, BusEvent::PartAdded { session_id, message_id, .. } if session_id == sid && message_id == mid)
        );
        assert!(
            matches!(third, BusEvent::PartAdded { session_id, message_id, .. } if session_id == sid && message_id == mid)
        );
        assert!(matches!(fourth, BusEvent::SessionCompleted { session_id } if session_id == sid));
    }

    #[tokio::test]
    async fn cancellation_keeps_already_persisted_output() {
        let (storage, _file) = make_storage().await;
        let storage_for_assertion = Arc::clone(&storage);
        let pid = ProjectId::new();
        let sid = SessionId::new();
        let mid = MessageId::new();
        storage.upsert_project(project_row(pid)).await.unwrap();
        storage.create_session(session_row(sid, pid)).await.unwrap();
        storage
            .append_message(assistant_shell(sid, mid, 1), Vec::new())
            .await
            .unwrap();

        let bus = Arc::new(BroadcastBus::default_capacity());
        let mut rx = bus.subscribe();
        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();

        let stream = Box::pin(
            stream::iter([Ok(ModelEvent::TextDelta {
                delta: "partial".into(),
            })])
            .chain(stream::pending()),
        );

        let run = tokio::spawn(async move {
            run_prompt_stream(
                RuntimeStreamContext {
                    session_id: sid,
                    assistant_message_id: mid,
                    storage,
                    bus,
                    provider_id: "stub".into(),
                    model_id: "test-model".into(),
                },
                stream,
                cancel_for_task,
            )
            .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        cancel.cancel();

        let result = run.await.unwrap();
        assert!(matches!(result, Err(SessionError::Cancelled)));

        let events = [
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
        ];
        assert!(events
            .iter()
            .any(|ev| matches!(ev, BusEvent::PartAdded { session_id, message_id, .. } if *session_id == sid && *message_id == mid)));
        assert!(events.iter().any(
            |ev| matches!(ev, BusEvent::SessionCancelled { session_id } if *session_id == sid)
        ));

        let history = storage_for_assertion
            .list_history_with_parts(sid)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].parts.len(), 1);
        assert_eq!(history[0].parts[0].data["text"], "partial");
    }
}
