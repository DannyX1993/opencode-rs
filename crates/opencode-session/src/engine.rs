//! [`Session`] trait and core [`SessionEngine`] runtime implementation.

use async_trait::async_trait;
use opencode_bus::{BroadcastBus, BusEvent, EventBus};
use opencode_core::{
    context::SessionCtx,
    dto::{MessageRow, PartRow},
    error::SessionError,
    id::{MessageId, PartId, SessionId},
};
use opencode_provider::{
    ModelRegistry,
    types::{ContentPart, ModelMessage, ModelRequest},
};
use opencode_storage::Storage;
use std::{sync::Arc, time::SystemTime};

use crate::run_state::RunState;
use crate::runtime;
use crate::types::{SessionHandle, SessionPrompt};

/// The session engine abstraction.
///
/// Callers inject `Arc<dyn Session>` so implementations can be swapped for
/// testing.
#[async_trait]
pub trait Session: Send + Sync {
    /// Submit a prompt and return a handle for tracking the turn.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError`] if the session cannot be found or the
    /// provider fails to initialise.
    async fn prompt(&self, req: SessionPrompt) -> Result<SessionHandle, SessionError>;

    /// Cancel an in-progress prompt turn.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NoActiveRun`] if no active turn exists.
    async fn cancel(&self, session_id: SessionId) -> Result<(), SessionError>;
}

/// Constructor-backed runtime session engine.
pub struct SessionEngine {
    storage: Arc<dyn Storage>,
    bus: Arc<BroadcastBus>,
    registry: Arc<ModelRegistry>,
    default_model: Option<String>,
    runs: Arc<RunState>,
}

impl SessionEngine {
    /// Construct a new [`SessionEngine`] with runtime collaborators.
    #[must_use]
    pub fn new(
        storage: Arc<dyn Storage>,
        bus: Arc<BroadcastBus>,
        registry: Arc<ModelRegistry>,
        default_model: Option<String>,
    ) -> Self {
        Self {
            storage,
            bus,
            registry,
            default_model,
            runs: Arc::new(RunState::default()),
        }
    }
}

#[async_trait]
impl Session for SessionEngine {
    async fn prompt(&self, req: SessionPrompt) -> Result<SessionHandle, SessionError> {
        if self
            .storage
            .get_session(req.session_id)
            .await
            .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?
            .is_none()
        {
            return Err(SessionError::NotFound(req.session_id.to_string()));
        }

        let run = self.runs.acquire(req.session_id).await?;

        SessionCtx::scope(req.session_id, async {
            let resolved_model = req
                .model
                .clone()
                .or_else(|| self.default_model.clone())
                .ok_or_else(|| {
                    SessionError::RuntimeInternal("no model resolved for prompt".into())
                })?;

            let (provider_id, provider_model) =
                resolved_model.split_once('/').ok_or_else(|| {
                    SessionError::RuntimeInternal(format!("invalid model id: {resolved_model}"))
                })?;

            let provider = self.registry.get(provider_id).await.ok_or_else(|| {
                SessionError::Provider(format!("provider not registered: {provider_id}"))
            })?;

            let now = now_millis();

            let user_message_id = MessageId::new();
            self.storage
                .append_message(
                    MessageRow {
                        id: user_message_id,
                        session_id: req.session_id,
                        time_created: now,
                        time_updated: now,
                        data: serde_json::json!({"role": "user"}),
                    },
                    vec![PartRow {
                        id: PartId::new(),
                        message_id: user_message_id,
                        session_id: req.session_id,
                        time_created: now,
                        time_updated: now,
                        data: serde_json::json!({"type": "text", "text": req.text.clone()}),
                    }],
                )
                .await
                .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?;

            let assistant_message_id = MessageId::new();
            let assistant_time = now + 1;
            self.storage
                .append_message(
                    MessageRow {
                        id: assistant_message_id,
                        session_id: req.session_id,
                        time_created: assistant_time,
                        time_updated: assistant_time,
                        data: serde_json::json!({"role": "assistant"}),
                    },
                    Vec::new(),
                )
                .await
                .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?;

            let _ = self.bus.publish(BusEvent::MessageAdded {
                session_id: req.session_id,
                message_id: user_message_id,
            });
            let _ = self.bus.publish(BusEvent::MessageAdded {
                session_id: req.session_id,
                message_id: assistant_message_id,
            });

            let stream = provider
                .stream(ModelRequest {
                    model: provider_model.to_string(),
                    system: Vec::new(),
                    messages: vec![ModelMessage {
                        role: "user".into(),
                        content: vec![ContentPart::Text {
                            text: req.text.clone(),
                        }],
                    }],
                    tools: Default::default(),
                    max_tokens: None,
                    temperature: None,
                })
                .await
                .map_err(|err| SessionError::Provider(err.to_string()))?;

            runtime::run_prompt_stream(
                runtime::RuntimeStreamContext {
                    session_id: req.session_id,
                    assistant_message_id,
                    storage: Arc::clone(&self.storage),
                    bus: Arc::clone(&self.bus),
                    provider_id: provider_id.to_string(),
                    model_id: provider_model.to_string(),
                },
                stream,
                run.cancellation_token(),
            )
            .await?;

            Ok(SessionHandle::new(req.session_id)
                .with_assistant_message_id(assistant_message_id)
                .with_resolved_model(resolved_model))
        })
        .await
    }

    async fn cancel(&self, session_id: SessionId) -> Result<(), SessionError> {
        if self.runs.cancel(session_id).await {
            Ok(())
        } else {
            Err(SessionError::NoActiveRun(session_id.to_string()))
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
    use async_trait::async_trait;
    use futures::stream;
    use opencode_bus::BroadcastBus;
    use opencode_core::{
        context::BoxStream,
        dto::{MessageRow, PartRow, ProjectRow, SessionRow},
        id::{MessageId, PartId, ProjectId, SessionId},
    };
    use opencode_provider::{
        LanguageModel, ModelEvent, ModelInfo, ModelRegistry, ModelRequest, ProviderError,
        types::{ContentPart, ModelMessage},
    };
    use opencode_storage::{Storage, StorageImpl, connect};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    enum StubMode {
        Done,
        DeltaThenDone,
        Pending,
    }

    struct StubProvider {
        mode: StubMode,
        started: Option<Arc<tokio::sync::Notify>>,
    }

    impl StubProvider {
        fn done() -> Self {
            Self {
                mode: StubMode::Done,
                started: None,
            }
        }

        fn delta_then_done() -> Self {
            Self {
                mode: StubMode::DeltaThenDone,
                started: None,
            }
        }

        fn pending() -> Self {
            Self {
                mode: StubMode::Pending,
                started: None,
            }
        }

        fn pending_with_start_signal(started: Arc<tokio::sync::Notify>) -> Self {
            Self {
                mode: StubMode::Pending,
                started: Some(started),
            }
        }
    }

    #[async_trait]
    impl LanguageModel for StubProvider {
        fn provider(&self) -> &'static str {
            "stub"
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "stub/test-model".into(),
                name: "Test".into(),
                context_window: 8_000,
                max_output: 1_000,
                vision: false,
            }])
        }

        async fn stream(
            &self,
            req: ModelRequest,
        ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
            if let Some(started) = &self.started {
                started.notify_waiters();
            }
            assert_eq!(req.model, "test-model");
            assert_eq!(req.messages.len(), 1);
            let first: &ModelMessage = &req.messages[0];
            assert_eq!(first.role, "user");
            assert!(matches!(
                first.content.first(),
                Some(ContentPart::Text { .. })
            ));

            let stream: BoxStream<Result<ModelEvent, ProviderError>> = match self.mode {
                StubMode::Done => Box::pin(stream::iter([Ok(ModelEvent::Done {
                    reason: "stop".into(),
                })])),
                StubMode::DeltaThenDone => Box::pin(stream::iter([
                    Ok(ModelEvent::TextDelta {
                        delta: "hello".into(),
                    }),
                    Ok(ModelEvent::TextDelta {
                        delta: " world".into(),
                    }),
                    Ok(ModelEvent::Done {
                        reason: "stop".into(),
                    }),
                ])),
                StubMode::Pending => Box::pin(stream::pending()),
            };
            Ok(stream)
        }
    }

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

    async fn build_engine(
        storage: Arc<dyn Storage>,
        provider: Arc<dyn LanguageModel>,
        default_model: Option<String>,
    ) -> SessionEngine {
        let registry = Arc::new(ModelRegistry::new());
        registry.register("stub", provider).await;

        SessionEngine::new(
            storage,
            Arc::new(BroadcastBus::default_capacity()),
            registry,
            default_model,
        )
    }

    #[tokio::test]
    async fn prompt_rejects_unknown_session() {
        let (storage, _file) = make_storage().await;
        let engine = build_engine(
            storage,
            Arc::new(StubProvider::done()),
            Some("stub/test-model".into()),
        )
        .await;

        let err = engine
            .prompt(SessionPrompt {
                session_id: SessionId::new(),
                text: "hello".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn prompt_unknown_session_rejected_without_creating_run_state() {
        let (storage, _file) = make_storage().await;
        let engine = build_engine(
            Arc::clone(&storage),
            Arc::new(StubProvider::done()),
            Some("stub/test-model".into()),
        )
        .await;

        let missing_session = SessionId::new();
        let err = engine
            .prompt(SessionPrompt {
                session_id: missing_session,
                text: "hello".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));

        let cancel_err = engine.cancel(missing_session).await.unwrap_err();
        assert!(matches!(cancel_err, SessionError::NoActiveRun(_)));
        let history = storage
            .list_history_with_parts(missing_session)
            .await
            .unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn prompt_persists_user_turn_and_assistant_shell() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .unwrap();

        let engine = build_engine(
            Arc::clone(&storage),
            Arc::new(StubProvider::done()),
            Some("stub/test-model".into()),
        )
        .await;

        let handle = engine
            .prompt(SessionPrompt {
                session_id,
                text: "hello world".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap();

        assert_eq!(handle.session_id, session_id);
        assert_eq!(handle.resolved_model.as_deref(), Some("stub/test-model"));
        assert!(handle.assistant_message_id.is_some());

        let history = storage.list_history_with_parts(session_id).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].info.data["role"], "user");
        assert_eq!(history[0].parts[0].data["text"], "hello world");
        assert_eq!(history[1].info.data["role"], "assistant");
    }

    #[tokio::test]
    async fn prompt_completion_publishes_terminal_event_matching_persisted_state() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .unwrap();

        let bus = Arc::new(BroadcastBus::default_capacity());
        let mut rx = bus.subscribe();

        let registry = Arc::new(ModelRegistry::new());
        registry
            .register("stub", Arc::new(StubProvider::delta_then_done()))
            .await;

        let engine = SessionEngine::new(
            Arc::clone(&storage),
            Arc::clone(&bus),
            registry,
            Some("stub/test-model".into()),
        );

        let handle = engine
            .prompt(SessionPrompt {
                session_id,
                text: "hello world".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap();

        let history = storage.list_history_with_parts(session_id).await.unwrap();
        assert_eq!(history.len(), 2);
        let assistant = &history[1];
        assert_eq!(assistant.info.id, handle.assistant_message_id.unwrap());
        assert_eq!(assistant.parts.len(), 2);
        assert_eq!(assistant.parts[0].data["text"], "hello");
        assert_eq!(assistant.parts[1].data["text"], " world");

        let events = [
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
        ];
        assert!(
            matches!(events[0], BusEvent::MessageAdded { session_id: sid, .. } if sid == session_id)
        );
        assert!(
            matches!(events[1], BusEvent::MessageAdded { session_id: sid, message_id } if sid == session_id && Some(message_id) == handle.assistant_message_id)
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, BusEvent::SessionUpdated { session_id: sid } if *sid == session_id))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, BusEvent::SessionCompleted { session_id: sid } if *sid == session_id))
        );

        let err = engine.cancel(session_id).await.unwrap_err();
        assert!(matches!(err, SessionError::NoActiveRun(_)));
    }

    #[tokio::test]
    async fn cancel_returns_no_active_run_when_idle() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let idle_session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(idle_session_id, project_id))
            .await
            .unwrap();

        let existing_message_id = MessageId::new();
        let existing_part_id = PartId::new();
        storage
            .append_message(
                MessageRow {
                    id: existing_message_id,
                    session_id: idle_session_id,
                    time_created: 10,
                    time_updated: 10,
                    data: serde_json::json!({"role": "assistant"}),
                },
                vec![PartRow {
                    id: existing_part_id,
                    message_id: existing_message_id,
                    session_id: idle_session_id,
                    time_created: 11,
                    time_updated: 11,
                    data: serde_json::json!({"type": "text", "text": "existing"}),
                }],
            )
            .await
            .unwrap();

        let history_before = storage
            .list_history_with_parts(idle_session_id)
            .await
            .unwrap();
        assert_eq!(history_before.len(), 1);
        assert_eq!(history_before[0].parts.len(), 1);
        assert_eq!(history_before[0].parts[0].data["text"], "existing");

        let engine = build_engine(
            Arc::clone(&storage),
            Arc::new(StubProvider::done()),
            Some("stub/test-model".into()),
        )
        .await;

        let err = engine.cancel(idle_session_id).await.unwrap_err();
        assert!(matches!(err, SessionError::NoActiveRun(_)));

        let history_after = storage
            .list_history_with_parts(idle_session_id)
            .await
            .unwrap();
        assert_eq!(history_after.len(), 1);
        assert_eq!(history_after[0].info.id, existing_message_id);
        assert_eq!(history_after[0].parts.len(), 1);
        assert_eq!(history_after[0].parts[0].id, existing_part_id);
        assert_eq!(history_after[0].parts[0].data["text"], "existing");
    }

    #[tokio::test]
    async fn cancel_stops_active_prompt_and_releases_run_state() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .unwrap();

        let engine = Arc::new(
            build_engine(
                Arc::clone(&storage),
                Arc::new(StubProvider::pending()),
                Some("stub/test-model".into()),
            )
            .await,
        );

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "hang".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        let mut cancelled = false;
        for _ in 0..20 {
            match engine.cancel(session_id).await {
                Ok(()) => {
                    cancelled = true;
                    break;
                }
                Err(SessionError::NoActiveRun(_)) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(other) => panic!("unexpected cancel error: {other}"),
            }
        }

        assert!(cancelled, "expected an active run to be cancellable");

        let prompt_result = tokio::time::timeout(std::time::Duration::from_millis(200), runner)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(prompt_result, Err(SessionError::Cancelled)));

        let err = engine.cancel(session_id).await.unwrap_err();
        assert!(matches!(err, SessionError::NoActiveRun(_)));
    }

    #[tokio::test]
    async fn concurrent_prompt_is_denied_while_run_active() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .unwrap();

        let started = Arc::new(tokio::sync::Notify::new());
        let engine = Arc::new(
            build_engine(
                Arc::clone(&storage),
                Arc::new(StubProvider::pending_with_start_signal(Arc::clone(
                    &started,
                ))),
                Some("stub/test-model".into()),
            )
            .await,
        );

        let first_prompt = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "first".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        tokio::time::timeout(std::time::Duration::from_millis(200), started.notified())
            .await
            .expect("first prompt should start provider stream");

        let second_result = engine
            .prompt(SessionPrompt {
                session_id,
                text: "second".into(),
                model: None,
                plan_mode: false,
            })
            .await;
        assert!(matches!(second_result, Err(SessionError::Busy(_))));

        engine.cancel(session_id).await.unwrap();
        let first_result =
            tokio::time::timeout(std::time::Duration::from_millis(200), first_prompt)
                .await
                .unwrap()
                .unwrap();
        assert!(matches!(first_result, Err(SessionError::Cancelled)));
    }

    #[tokio::test]
    async fn cancelled_run_allows_future_prompt_to_start() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        storage
            .upsert_project(project_row(project_id))
            .await
            .unwrap();
        storage
            .create_session(session_row(session_id, project_id))
            .await
            .unwrap();

        let started = Arc::new(tokio::sync::Notify::new());
        let engine = Arc::new(
            build_engine(
                Arc::clone(&storage),
                Arc::new(StubProvider::pending_with_start_signal(Arc::clone(
                    &started,
                ))),
                Some("stub/test-model".into()),
            )
            .await,
        );

        let first_prompt = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "first".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        tokio::time::timeout(std::time::Duration::from_millis(200), started.notified())
            .await
            .expect("first prompt should start provider stream");
        engine.cancel(session_id).await.unwrap();
        let first_result =
            tokio::time::timeout(std::time::Duration::from_millis(200), first_prompt)
                .await
                .unwrap()
                .unwrap();
        assert!(matches!(first_result, Err(SessionError::Cancelled)));

        let second_prompt = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "second".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        tokio::time::timeout(std::time::Duration::from_millis(200), started.notified())
            .await
            .expect("second prompt should start after cancellation");
        engine.cancel(session_id).await.unwrap();
        let second_result =
            tokio::time::timeout(std::time::Duration::from_millis(200), second_prompt)
                .await
                .unwrap()
                .unwrap();
        assert!(matches!(second_result, Err(SessionError::Cancelled)));
    }
}
