//! Runtime stream processing primitives for session prompt execution.

use futures::StreamExt;
use opencode_bus::{BroadcastBus, BusEvent, EventBus};
use opencode_core::{
    context::CancellationToken,
    dto::{MessageRow, PartRow},
    error::SessionError,
    id::{MessageId, PartId, SessionId},
};
use opencode_provider::{ModelEvent, ProviderError};
use opencode_storage::Storage;
use opencode_tool::ToolResult;
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

/// Outcome of one provider streaming pass for a turn.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptTurnOutcome {
    /// Provider finished the turn without requesting tool execution.
    Done,
    /// Provider emitted a complete tool request that runtime must execute.
    ToolCall {
        /// Provider correlation id.
        id: String,
        /// Tool name.
        name: String,
        /// Parsed JSON tool input.
        input: serde_json::Value,
    },
}

#[derive(Debug, Default)]
struct ToolUseAccumulator {
    id: Option<String>,
    name: Option<String>,
    thought_signature: Option<String>,
    input: String,
}

/// Consume provider stream until completion or cancellation.
///
/// This phase only drives control-flow lifecycle. Event-to-storage projection is
/// implemented in the next runtime batch.
pub async fn run_prompt_stream(
    ctx: RuntimeStreamContext,
    mut stream: opencode_core::context::BoxStream<Result<ModelEvent, ProviderError>>,
    cancel: CancellationToken,
) -> Result<PromptTurnOutcome, SessionError> {
    let _ = ctx.bus.publish(BusEvent::SessionUpdated {
        session_id: ctx.session_id,
    });

    let mut part_sequence: i64 = 0;
    let mut tool = ToolUseAccumulator::default();

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
                        return Ok(PromptTurnOutcome::Done);
                    }
                    Some(Ok(ModelEvent::ToolUseStart {
                        id,
                        name,
                        thought_signature,
                    })) => {
                        if tool.id.is_some() {
                            return Err(SessionError::RuntimeInternal(
                                "received nested tool-use start before prior call completed".into(),
                            ));
                        }
                        tool.id = Some(id);
                        tool.name = Some(name);
                        tool.thought_signature = thought_signature;
                    }
                    Some(Ok(ModelEvent::ToolUseInputDelta { id, delta })) => {
                        if tool.id.as_deref() != Some(id.as_str()) {
                            return Err(SessionError::RuntimeInternal(format!(
                                "tool input delta id mismatch: expected {:?}, got {id}",
                                tool.id
                            )));
                        }
                        tool.input.push_str(&delta);
                    }
                    Some(Ok(ModelEvent::ToolUseEnd { id })) => {
                        if tool.id.as_deref() != Some(id.as_str()) {
                            return Err(SessionError::RuntimeInternal(format!(
                                "tool end id mismatch: expected {:?}, got {id}",
                                tool.id
                            )));
                        }
                        let name = tool.name.clone().ok_or_else(|| {
                            SessionError::RuntimeInternal(
                                "tool-use end received without tool name".into(),
                            )
                        })?;
                        let input = if tool.input.trim().is_empty() {
                            serde_json::json!({})
                        } else {
                            serde_json::from_str::<serde_json::Value>(&tool.input).map_err(|err| {
                                SessionError::RuntimeInternal(format!(
                                    "invalid tool input json for {name}: {err}"
                                ))
                            })?
                        };

                        part_sequence += 1;
                        let time = now_millis() + part_sequence;
                        let part = PartRow {
                            id: PartId::new(),
                            message_id: ctx.assistant_message_id,
                            session_id: ctx.session_id,
                            time_created: time,
                            time_updated: time,
                            data: serde_json::json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input,
                                "thought_signature": tool.thought_signature,
                                "seq": part_sequence,
                            }),
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

                        return Ok(PromptTurnOutcome::ToolCall { id, name, input });
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(SessionError::Provider(err.to_string())),
                    None => {
                        let _ = ctx.bus.publish(BusEvent::SessionCompleted { session_id: ctx.session_id });
                        return Ok(PromptTurnOutcome::Done);
                    }
                }
            }
        }
    }
}

/// Persist a tool result as a dedicated `tool` role message.
pub async fn persist_tool_result(
    ctx: &RuntimeStreamContext,
    call_id: &str,
    result: &ToolResult,
) -> Result<(), SessionError> {
    let now = now_millis();
    let tool_message_id = MessageId::new();
    let part_id = PartId::new();

    // Persist provider-neutral runtime history here. Provider adapters can normalize replay
    // details later (for example, Google remaps this `tool` role to wire-level `user`).
    ctx.storage
        .append_message(
            MessageRow {
                id: tool_message_id,
                session_id: ctx.session_id,
                time_created: now,
                time_updated: now,
                data: serde_json::json!({"role": "tool"}),
            },
            vec![PartRow {
                id: part_id,
                message_id: tool_message_id,
                session_id: ctx.session_id,
                time_created: now,
                time_updated: now,
                data: serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": result.as_provider_tool_result_content(),
                    "is_error": result.is_err,
                }),
            }],
        )
        .await
        .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?;

    let _ = ctx.bus.publish(BusEvent::MessageAdded {
        session_id: ctx.session_id,
        message_id: tool_message_id,
    });
    let _ = ctx.bus.publish(BusEvent::PartAdded {
        session_id: ctx.session_id,
        message_id: tool_message_id,
        part_id,
    });

    Ok(())
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

    #[tokio::test]
    async fn tool_events_return_tool_call_outcome() {
        let (storage, _file) = make_storage().await;
        let storage_for_assertion = storage.clone();
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
        let stream = Box::pin(stream::iter([
            Ok(ModelEvent::ToolUseStart {
                id: "call_1".into(),
                name: "bash".into(),
                thought_signature: Some("sig-runtime".into()),
            }),
            Ok(ModelEvent::ToolUseInputDelta {
                id: "call_1".into(),
                delta: "{\"command\":\"ls\"}".into(),
            }),
            Ok(ModelEvent::ToolUseEnd {
                id: "call_1".into(),
            }),
        ]));

        let outcome = run_prompt_stream(
            RuntimeStreamContext {
                session_id: sid,
                assistant_message_id: mid,
                storage,
                bus,
                provider_id: "stub".into(),
                model_id: "test-model".into(),
            },
            stream,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        match outcome {
            PromptTurnOutcome::ToolCall { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "bash");
                assert_eq!(input["command"], "ls");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }

        let history = storage_for_assertion
            .list_history_with_parts(sid)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].parts.len(), 1);
        assert_eq!(history[0].parts[0].data["type"], "tool_use");
        assert_eq!(
            history[0].parts[0].data["thought_signature"],
            serde_json::json!("sig-runtime")
        );
    }

    #[tokio::test]
    async fn malformed_tool_input_returns_runtime_error() {
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
        let stream = Box::pin(stream::iter([
            Ok(ModelEvent::ToolUseStart {
                id: "call_1".into(),
                name: "bash".into(),
                thought_signature: None,
            }),
            Ok(ModelEvent::ToolUseInputDelta {
                id: "call_1".into(),
                delta: "{bad-json".into(),
            }),
            Ok(ModelEvent::ToolUseEnd {
                id: "call_1".into(),
            }),
        ]));

        let err = run_prompt_stream(
            RuntimeStreamContext {
                session_id: sid,
                assistant_message_id: mid,
                storage,
                bus,
                provider_id: "stub".into(),
                model_id: "test-model".into(),
            },
            stream,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, SessionError::RuntimeInternal(_)));
    }

    #[tokio::test]
    async fn tool_end_id_mismatch_returns_runtime_error() {
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
        let stream = Box::pin(stream::iter([
            Ok(ModelEvent::ToolUseStart {
                id: "call_1".into(),
                name: "bash".into(),
                thought_signature: None,
            }),
            Ok(ModelEvent::ToolUseEnd {
                id: "call_2".into(),
            }),
        ]));

        let err = run_prompt_stream(
            RuntimeStreamContext {
                session_id: sid,
                assistant_message_id: mid,
                storage,
                bus,
                provider_id: "stub".into(),
                model_id: "test-model".into(),
            },
            stream,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, SessionError::RuntimeInternal(_)));
    }
}
