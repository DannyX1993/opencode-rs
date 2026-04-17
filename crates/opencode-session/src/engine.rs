//! [`Session`] trait and core [`SessionEngine`] runtime implementation.

use async_trait::async_trait;
use opencode_bus::{BroadcastBus, BusEvent, EventBus};
use opencode_core::{
    context::SessionCtx,
    dto::{MessageRow, MessageWithParts, PartRow},
    error::SessionError,
    id::{MessageId, PartId, SessionId},
};
use opencode_provider::{
    ModelRegistry,
    types::{ContentPart, ModelMessage, ModelRequest},
};
use opencode_storage::Storage;
use opencode_tool::{Ctx, ToolCall, ToolRegistry, ToolResult};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::SystemTime,
};

use crate::run_state::RunState;
use crate::runtime;
use crate::types::{
    DetachedPromptAccepted, PermissionReply, PermissionReplyKind, PermissionRequest, QuestionInfo,
    QuestionOption, QuestionRequest, RuntimeToolCallRef, SessionBlockedKind, SessionHandle,
    SessionPrompt, SessionRuntimeStatus,
};

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

    /// Submit a prompt in detached/background mode.
    ///
    /// Returns acceptance metadata immediately while execution continues
    /// asynchronously.
    async fn prompt_detached(
        &self,
        req: SessionPrompt,
    ) -> Result<DetachedPromptAccepted, SessionError>;

    /// Cancel an in-progress prompt turn.
    ///
    /// Implementations may also resolve pending interactive runtime requests
    /// (permission/question) for the same session.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NoActiveRun`] if no active turn exists.
    async fn cancel(&self, session_id: SessionId) -> Result<(), SessionError>;

    /// Read current runtime occupancy for one session.
    ///
    /// Status may be `idle`, `busy`, or `blocked` when waiting on permission
    /// or question runtime input.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NotFound`] when the session does not exist.
    async fn status(&self, session_id: SessionId) -> Result<SessionRuntimeStatus, SessionError>;

    /// List currently active runtime statuses.
    ///
    /// Implementations may return only non-idle entries and blocked sessions.
    async fn list_statuses(&self)
    -> Result<HashMap<SessionId, SessionRuntimeStatus>, SessionError>;
}

/// Constructor-backed runtime session engine.
pub struct SessionEngine {
    storage: Arc<dyn Storage>,
    bus: Arc<BroadcastBus>,
    registry: Arc<ModelRegistry>,
    default_model: Option<String>,
    runs: Arc<RunState>,
    permission_runtime: Arc<dyn crate::permission_runtime::PermissionRuntime>,
    question_runtime: Arc<dyn crate::question_runtime::QuestionRuntime>,
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
        let permission_runtime: Arc<dyn crate::permission_runtime::PermissionRuntime> =
            Arc::new(crate::permission_runtime::InMemoryPermissionRuntime::new(
                Arc::clone(&storage),
                Arc::clone(&bus),
            ));
        let question_runtime: Arc<dyn crate::question_runtime::QuestionRuntime> = Arc::new(
            crate::question_runtime::InMemoryQuestionRuntime::new(Arc::clone(&bus)),
        );

        Self::with_runtimes(
            storage,
            bus,
            registry,
            default_model,
            permission_runtime,
            question_runtime,
        )
    }

    /// Construct a [`SessionEngine`] with externally supplied runtime services.
    #[must_use]
    pub fn with_runtimes(
        storage: Arc<dyn Storage>,
        bus: Arc<BroadcastBus>,
        registry: Arc<ModelRegistry>,
        default_model: Option<String>,
        permission_runtime: Arc<dyn crate::permission_runtime::PermissionRuntime>,
        question_runtime: Arc<dyn crate::question_runtime::QuestionRuntime>,
    ) -> Self {
        Self {
            storage,
            bus,
            registry,
            default_model,
            runs: Arc::new(RunState::default()),
            permission_runtime,
            question_runtime,
        }
    }
}

#[async_trait]
impl Session for SessionEngine {
    async fn prompt(&self, req: SessionPrompt) -> Result<SessionHandle, SessionError> {
        let session_row = self
            .storage
            .get_session(req.session_id)
            .await
            .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?
            .ok_or_else(|| SessionError::NotFound(req.session_id.to_string()))?;
        let project_row = self
            .storage
            .get_project(session_row.project_id)
            .await
            .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?
            .ok_or_else(|| {
                SessionError::RuntimeInternal(format!(
                    "project not found for session {}",
                    req.session_id
                ))
            })?;

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

            let supports_runtime_tools = provider_supports_runtime_tools(provider_id);
            let tool_registry = if supports_runtime_tools {
                Some(ToolRegistry::with_builtins(runtime_tool_ctx(
                    &project_row.worktree,
                    &session_row.directory,
                )))
            } else {
                None
            };

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

            loop {
                let history = self
                    .storage
                    .list_history_with_parts(req.session_id)
                    .await
                    .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?;

                let tool_defs = if let Some(registry) = &tool_registry {
                    let defs = registry.definitions().await;
                    defs.into_iter()
                        .map(|d| {
                            (
                                d.name.clone(),
                                serde_json::json!({
                                    "name": d.name,
                                    "description": d.description,
                                    "input_schema": d.input_schema,
                                }),
                            )
                        })
                        .collect::<BTreeMap<_, _>>()
                } else {
                    BTreeMap::new()
                };

                let request = ModelRequest {
                    model: provider_model.to_string(),
                    system: Vec::new(),
                    messages: history_to_model_messages(&history),
                    tools: tool_defs,
                    max_tokens: None,
                    temperature: None,
                };

                let stream = provider
                    .stream(request)
                    .await
                    .map_err(|err| SessionError::Provider(err.to_string()))?;

                let runtime_ctx = runtime::RuntimeStreamContext {
                    session_id: req.session_id,
                    assistant_message_id,
                    storage: Arc::clone(&self.storage),
                    bus: Arc::clone(&self.bus),
                    provider_id: provider_id.to_string(),
                    model_id: provider_model.to_string(),
                };

                match runtime::run_prompt_stream(runtime_ctx, stream, run.cancellation_token()).await?
                {
                    runtime::PromptTurnOutcome::Done => break,
                    runtime::PromptTurnOutcome::ToolCall { id, name, input } => {
                        if !supports_runtime_tools {
                            return Err(SessionError::Provider(format!(
                                "provider '{provider_id}' does not support runtime tool execution in this MVP"
                            )));
                        }

                        let tool_ref = RuntimeToolCallRef {
                            message_id: assistant_message_id,
                            call_id: id.clone(),
                        };

                        let permission_patterns = permission_patterns_for_tool_call(&name, &input);
                        self.permission_runtime
                            .ask(PermissionRequest {
                                id: format!("permission-{id}"),
                                session_id: req.session_id,
                                permission: name.clone(),
                                patterns: permission_patterns.clone(),
                                metadata: serde_json::json!({
                                    "source": "tool_loop",
                                    "tool": name,
                                    "input": input,
                                }),
                                always: permission_patterns,
                                tool: Some(tool_ref.clone()),
                            })
                            .await?;

                        let mut invocation_input = input;
                        if let Some(questions) = take_runtime_questions(&mut invocation_input)? {
                            let _answers = self
                                .question_runtime
                                .ask(QuestionRequest {
                                    id: format!("question-{id}"),
                                    session_id: req.session_id,
                                    questions,
                                    tool: Some(tool_ref.clone()),
                                })
                                .await?;
                        }

                        let _ = self.bus.publish(BusEvent::ToolStarted {
                            session_id: req.session_id,
                            tool: name.clone(),
                            call_id: id.clone(),
                        });

                        let result = if let Some(registry) = &tool_registry {
                            match registry
                                .invoke(ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    args: invocation_input,
                                })
                                .await
                            {
                                Ok(result) => result,
                                Err(err) => ToolResult::err(id.clone(), err.to_string()),
                            }
                        } else {
                            ToolResult::err(id.clone(), "tool registry unavailable".into())
                        };

                        let persist_ctx = runtime::RuntimeStreamContext {
                            session_id: req.session_id,
                            assistant_message_id,
                            storage: Arc::clone(&self.storage),
                            bus: Arc::clone(&self.bus),
                            provider_id: provider_id.to_string(),
                            model_id: provider_model.to_string(),
                        };
                        runtime::persist_tool_result(&persist_ctx, &id, &result).await?;

                        let _ = self.bus.publish(BusEvent::ToolFinished {
                            session_id: req.session_id,
                            tool: name,
                            call_id: id,
                            ok: !result.is_err,
                        });
                    }
                }
            }

            Ok(SessionHandle::new(req.session_id)
                .with_assistant_message_id(assistant_message_id)
                .with_resolved_model(resolved_model))
        })
        .await
    }

    async fn prompt_detached(
        &self,
        req: SessionPrompt,
    ) -> Result<DetachedPromptAccepted, SessionError> {
        let exists = self
            .storage
            .get_session(req.session_id)
            .await
            .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?
            .is_some();
        if !exists {
            return Err(SessionError::NotFound(req.session_id.to_string()));
        }

        let accepted = DetachedPromptAccepted {
            session_id: req.session_id,
            assistant_message_id: None,
            resolved_model: req.model.clone().or_else(|| self.default_model.clone()),
        };

        let detached_engine = Self {
            storage: Arc::clone(&self.storage),
            bus: Arc::clone(&self.bus),
            registry: Arc::clone(&self.registry),
            default_model: self.default_model.clone(),
            runs: Arc::clone(&self.runs),
            permission_runtime: Arc::clone(&self.permission_runtime),
            question_runtime: Arc::clone(&self.question_runtime),
        };

        let accepted_session_id = accepted.session_id;

        tokio::spawn(async move {
            if let Err(err) = detached_engine.prompt(req).await {
                if !matches!(err, SessionError::Cancelled | SessionError::NoActiveRun(_)) {
                    let _ = detached_engine.bus.publish(BusEvent::SessionError {
                        session_id: accepted_session_id,
                        error: err.to_string(),
                    });
                }
            }
        });

        Ok(accepted)
    }

    async fn cancel(&self, session_id: SessionId) -> Result<(), SessionError> {
        let mut handled_pending = false;

        for pending in self
            .permission_runtime
            .list()
            .await?
            .into_iter()
            .filter(|pending| pending.session_id == session_id)
        {
            handled_pending = true;
            let _ = self
                .permission_runtime
                .reply(PermissionReply {
                    session_id,
                    request_id: pending.id,
                    reply: PermissionReplyKind::Reject,
                })
                .await?;
        }

        for pending in self
            .question_runtime
            .list()
            .await?
            .into_iter()
            .filter(|pending| pending.session_id == session_id)
        {
            handled_pending = true;
            let _ = self.question_runtime.reject(pending.id).await?;
        }

        if self.runs.cancel(session_id).await || handled_pending {
            Ok(())
        } else {
            Err(SessionError::NoActiveRun(session_id.to_string()))
        }
    }

    async fn status(&self, session_id: SessionId) -> Result<SessionRuntimeStatus, SessionError> {
        let exists = self
            .storage
            .get_session(session_id)
            .await
            .map_err(|err| SessionError::RuntimeInternal(err.to_string()))?
            .is_some();
        if !exists {
            return Err(SessionError::NotFound(session_id.to_string()));
        }

        let snapshot = self.runs.snapshot(session_id).await;
        if let Some(blocked) = self.blocked_status_for_session(session_id).await? {
            return Ok(blocked);
        }
        if snapshot.is_active {
            Ok(SessionRuntimeStatus::Busy)
        } else {
            Ok(SessionRuntimeStatus::Idle)
        }
    }

    async fn list_statuses(
        &self,
    ) -> Result<HashMap<SessionId, SessionRuntimeStatus>, SessionError> {
        let mut statuses = HashMap::new();
        for session_id in self.runs.list_active_sessions().await {
            statuses.insert(session_id, SessionRuntimeStatus::Busy);
        }

        for pending in self.permission_runtime.list().await? {
            statuses.insert(
                pending.session_id,
                SessionRuntimeStatus::Blocked {
                    kind: SessionBlockedKind::Permission,
                    request_id: pending.id,
                },
            );
        }

        for pending in self.question_runtime.list().await? {
            statuses.insert(
                pending.session_id,
                SessionRuntimeStatus::Blocked {
                    kind: SessionBlockedKind::Question,
                    request_id: pending.id,
                },
            );
        }

        Ok(statuses)
    }
}

impl SessionEngine {
    async fn blocked_status_for_session(
        &self,
        session_id: SessionId,
    ) -> Result<Option<SessionRuntimeStatus>, SessionError> {
        if let Some(pending) = self
            .permission_runtime
            .list()
            .await?
            .into_iter()
            .find(|pending| pending.session_id == session_id)
        {
            return Ok(Some(SessionRuntimeStatus::Blocked {
                kind: SessionBlockedKind::Permission,
                request_id: pending.id,
            }));
        }

        if let Some(pending) = self
            .question_runtime
            .list()
            .await?
            .into_iter()
            .find(|pending| pending.session_id == session_id)
        {
            return Ok(Some(SessionRuntimeStatus::Blocked {
                kind: SessionBlockedKind::Question,
                request_id: pending.id,
            }));
        }

        Ok(None)
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn provider_supports_runtime_tools(provider_id: &str) -> bool {
    // Bounded MVP gate: only providers whose adapters can round-trip persisted tool history
    // without lossy replay are allowed into the runtime tool loop.
    matches!(provider_id, "anthropic" | "google")
}

fn runtime_tool_ctx(project_worktree: &str, session_directory: &str) -> Ctx {
    // Runtime contract preservation:
    // - tool root comes from persisted `project.worktree`
    // - tool cwd comes from persisted `session.directory`
    let mut tool_ctx = Ctx::default_for(PathBuf::from(project_worktree));
    tool_ctx.cwd = PathBuf::from(session_directory);
    tool_ctx
}

fn permission_patterns_for_tool_call(name: &str, input: &serde_json::Value) -> Vec<String> {
    if name == "bash"
        && let Some(command) = input.get("command").and_then(serde_json::Value::as_str)
    {
        return vec![format!("bash:{command}")];
    }

    vec![format!("{name}:*")]
}

fn take_runtime_questions(
    input: &mut serde_json::Value,
) -> Result<Option<Vec<QuestionInfo>>, SessionError> {
    let Some(map) = input.as_object_mut() else {
        return Ok(None);
    };

    let Some(payload) = map.remove("_opencode_question") else {
        return Ok(None);
    };

    let questions = payload
        .get("questions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            SessionError::RuntimeInternal(
                "_opencode_question payload must include a questions array".into(),
            )
        })?;

    let parsed = questions
        .iter()
        .enumerate()
        .map(|(index, value)| parse_runtime_question_info(index, value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(parsed))
}

fn parse_runtime_question_info(
    index: usize,
    value: &serde_json::Value,
) -> Result<QuestionInfo, SessionError> {
    let question = value
        .get("question")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            SessionError::RuntimeInternal(format!(
                "question entry {index} missing string field 'question'"
            ))
        })?
        .to_string();

    let header = value
        .get("header")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            SessionError::RuntimeInternal(format!(
                "question entry {index} missing string field 'header'"
            ))
        })?
        .to_string();

    let options = value
        .get("options")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            SessionError::RuntimeInternal(format!(
                "question entry {index} missing array field 'options'"
            ))
        })?
        .iter()
        .enumerate()
        .map(|(opt_index, option)| parse_runtime_question_option(index, opt_index, option))
        .collect::<Result<Vec<_>, _>>()?;

    let multiple = value.get("multiple").and_then(serde_json::Value::as_bool);
    let custom = value.get("custom").and_then(serde_json::Value::as_bool);

    Ok(QuestionInfo {
        question,
        header,
        options,
        multiple,
        custom,
    })
}

fn parse_runtime_question_option(
    question_index: usize,
    option_index: usize,
    value: &serde_json::Value,
) -> Result<QuestionOption, SessionError> {
    let label = value
        .get("label")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            SessionError::RuntimeInternal(format!(
                "question entry {question_index} option {option_index} missing string field 'label'"
            ))
        })?
        .to_string();
    let description = value
        .get("description")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            SessionError::RuntimeInternal(format!(
                "question entry {question_index} option {option_index} missing string field 'description'"
            ))
        })?
        .to_string();

    Ok(QuestionOption { label, description })
}

fn history_to_model_messages(history: &[MessageWithParts]) -> Vec<ModelMessage> {
    // Replay is storage-first by design. Each provider pass is rebuilt from persisted history
    // so cancellation/retry behavior never depends on transient in-memory tool state.
    let mut out = Vec::new();
    for message in history {
        let role = message
            .info
            .data
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_string();

        let mut content = Vec::new();
        for part in &message.parts {
            let Some(part_type) = part.data.get("type").and_then(|v| v.as_str()) else {
                continue;
            };
            match part_type {
                "text" => {
                    if let Some(text) = part.data.get("text").and_then(|v| v.as_str()) {
                        content.push(ContentPart::Text {
                            text: text.to_string(),
                        });
                    }
                }
                "tool_use" => {
                    let Some(id) = part.data.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let Some(name) = part.data.get("name").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let input = part
                        .data
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let thought_signature = part
                        .data
                        .get("thought_signature")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    content.push(ContentPart::ToolUse {
                        id: id.to_string(),
                        name: name.to_string(),
                        input,
                        thought_signature,
                    });
                }
                "tool_result" => {
                    let Some(tool_use_id) = part.data.get("tool_use_id").and_then(|v| v.as_str())
                    else {
                        continue;
                    };
                    let content_text = part
                        .data
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    content.push(ContentPart::ToolResult {
                        tool_use_id: tool_use_id.to_string(),
                        content: content_text,
                    });
                }
                _ => {}
            }
        }

        if !content.is_empty() {
            out.push(ModelMessage { role, content });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use opencode_bus::{BroadcastBus, BusEvent, EventBus};
    use opencode_core::{
        context::BoxStream,
        dto::{MessageRow, MessageWithParts, PartRow, ProjectRow, SessionRow},
        id::{MessageId, PartId, ProjectId, SessionId},
    };
    use opencode_provider::{
        LanguageModel, ModelEvent, ModelInfo, ModelRegistry, ModelRequest, ProviderError,
        types::ContentPart,
    };
    use opencode_storage::{Storage, StorageImpl, connect};
    use std::sync::Arc;
    use tempfile::NamedTempFile;
    use tokio::time::{Duration, sleep};

    enum StubMode {
        Done,
        DeltaThenDone,
        Pending,
    }

    struct StubProvider {
        mode: StubMode,
        started: Option<Arc<tokio::sync::Notify>>,
    }

    struct ToolLoopProvider {
        calls: std::sync::Mutex<usize>,
    }

    struct ToolLoopFailureProvider {
        calls: std::sync::Mutex<usize>,
    }

    struct ToolLoopSecondPassPendingProvider {
        calls: std::sync::Mutex<usize>,
        second_pass_started: Arc<tokio::sync::Notify>,
    }

    struct ToolLoopQuestionProvider {
        calls: std::sync::Mutex<usize>,
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

    impl ToolLoopProvider {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
            }
        }
    }

    impl ToolLoopFailureProvider {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
            }
        }
    }

    impl ToolLoopSecondPassPendingProvider {
        fn new(second_pass_started: Arc<tokio::sync::Notify>) -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
                second_pass_started,
            }
        }
    }

    impl ToolLoopQuestionProvider {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
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
            assert!(!req.messages.is_empty());
            assert!(req.messages.iter().any(|msg| {
                msg.role == "user"
                    && msg
                        .content
                        .iter()
                        .any(|part| matches!(part, ContentPart::Text { .. }))
            }));

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

    #[async_trait]
    impl LanguageModel for ToolLoopProvider {
        fn provider(&self) -> &'static str {
            "anthropic"
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "anthropic/test-model".into(),
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
            let mut guard = self.calls.lock().unwrap();
            let call = *guard;
            *guard += 1;
            drop(guard);

            if call == 0 {
                assert!(
                    !req.tools.is_empty(),
                    "expected tool declarations on first pass"
                );
                return Ok(Box::pin(stream::iter([
                    Ok(ModelEvent::ToolUseStart {
                        id: "call_1".into(),
                        name: "bash".into(),
                        thought_signature: None,
                    }),
                    Ok(ModelEvent::ToolUseInputDelta {
                        id: "call_1".into(),
                        delta: "{\"command\":\"echo hi\",\"description\":\"Echo\"}".into(),
                    }),
                    Ok(ModelEvent::ToolUseEnd {
                        id: "call_1".into(),
                    }),
                ])));
            }

            assert!(
                req.messages.iter().any(|m| {
                    m.content
                        .iter()
                        .any(|p| matches!(p, ContentPart::ToolResult { .. }))
                }),
                "expected replayed tool_result on second pass"
            );

            Ok(Box::pin(stream::iter([
                Ok(ModelEvent::TextDelta {
                    delta: "done".into(),
                }),
                Ok(ModelEvent::Done {
                    reason: "stop".into(),
                }),
            ])))
        }
    }

    #[async_trait]
    impl LanguageModel for ToolLoopFailureProvider {
        fn provider(&self) -> &'static str {
            "anthropic"
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "anthropic/test-model".into(),
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
            let mut guard = self.calls.lock().unwrap();
            let call = *guard;
            *guard += 1;
            drop(guard);

            if call == 0 {
                assert!(
                    !req.tools.is_empty(),
                    "expected tool declarations on first pass"
                );
                return Ok(Box::pin(stream::iter([
                    Ok(ModelEvent::ToolUseStart {
                        id: "call_fail_1".into(),
                        name: "bash".into(),
                        thought_signature: None,
                    }),
                    Ok(ModelEvent::ToolUseInputDelta {
                        id: "call_fail_1".into(),
                        delta: "{\"command\":\"echo hi\"}".into(),
                    }),
                    Ok(ModelEvent::ToolUseEnd {
                        id: "call_fail_1".into(),
                    }),
                ])));
            }

            assert!(
                req.messages.iter().any(|m| {
                    m.content.iter().any(|p| {
                        matches!(p, ContentPart::ToolResult { content, .. } if content.contains("invalid args for bash"))
                    })
                }),
                "expected replayed failed tool_result on second pass"
            );

            Ok(Box::pin(stream::iter([
                Ok(ModelEvent::TextDelta {
                    delta: "recovered".into(),
                }),
                Ok(ModelEvent::Done {
                    reason: "stop".into(),
                }),
            ])))
        }
    }

    #[async_trait]
    impl LanguageModel for ToolLoopSecondPassPendingProvider {
        fn provider(&self) -> &'static str {
            "anthropic"
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "anthropic/test-model".into(),
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
            let mut guard = self.calls.lock().unwrap();
            let call = *guard;
            *guard += 1;
            drop(guard);

            if call == 0 {
                assert!(
                    !req.tools.is_empty(),
                    "expected tool declarations on first pass"
                );
                return Ok(Box::pin(stream::iter([
                    Ok(ModelEvent::ToolUseStart {
                        id: "call_cancel_1".into(),
                        name: "bash".into(),
                        thought_signature: None,
                    }),
                    Ok(ModelEvent::ToolUseInputDelta {
                        id: "call_cancel_1".into(),
                        delta: "{\"command\":\"echo hi\",\"description\":\"Echo\"}".into(),
                    }),
                    Ok(ModelEvent::ToolUseEnd {
                        id: "call_cancel_1".into(),
                    }),
                ])));
            }

            self.second_pass_started.notify_waiters();
            Ok(Box::pin(stream::pending()))
        }
    }

    #[async_trait]
    impl LanguageModel for ToolLoopQuestionProvider {
        fn provider(&self) -> &'static str {
            "anthropic"
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "anthropic/test-model".into(),
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
            let mut guard = self.calls.lock().unwrap();
            let call = *guard;
            *guard += 1;
            drop(guard);

            if call == 0 {
                assert!(
                    !req.tools.is_empty(),
                    "expected tool declarations on first pass"
                );
                return Ok(Box::pin(stream::iter([
                    Ok(ModelEvent::ToolUseStart {
                        id: "call_question_1".into(),
                        name: "bash".into(),
                        thought_signature: None,
                    }),
                    Ok(ModelEvent::ToolUseInputDelta {
                        id: "call_question_1".into(),
                        delta: "{\"command\":\"echo hi\",\"description\":\"Echo\",\"_opencode_question\":{\"questions\":[{\"question\":\"Pick environment\",\"header\":\"Env\",\"options\":[{\"label\":\"dev\",\"description\":\"Development\"}],\"multiple\":false,\"custom\":false}]}}".into(),
                    }),
                    Ok(ModelEvent::ToolUseEnd {
                        id: "call_question_1".into(),
                    }),
                ])));
            }

            assert!(
                req.messages.iter().any(|m| {
                    m.content
                        .iter()
                        .any(|p| matches!(p, ContentPart::ToolResult { .. }))
                }),
                "expected replayed tool_result on second pass"
            );

            Ok(Box::pin(stream::iter([
                Ok(ModelEvent::TextDelta {
                    delta: "question-done".into(),
                }),
                Ok(ModelEvent::Done {
                    reason: "stop".into(),
                }),
            ])))
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

    fn question_request(session_id: SessionId, request_id: &str) -> crate::types::QuestionRequest {
        crate::types::QuestionRequest {
            id: request_id.into(),
            session_id,
            questions: vec![crate::types::QuestionInfo {
                question: "Pick one".into(),
                header: "Question".into(),
                options: vec![crate::types::QuestionOption {
                    label: "a".into(),
                    description: "Option A".into(),
                }],
                multiple: Some(false),
                custom: Some(false),
            }],
            tool: None,
        }
    }

    async fn wait_for_pending_question_count(engine: &SessionEngine, expected: usize) {
        for _ in 0..50 {
            if engine
                .question_runtime
                .list()
                .await
                .expect("list pending questions")
                .len()
                == expected
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for pending question count {expected}");
    }

    async fn wait_for_pending_permission_count(engine: &SessionEngine, expected: usize) {
        for _ in 0..50 {
            if engine
                .permission_runtime
                .list()
                .await
                .expect("list pending permissions")
                .len()
                == expected
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for pending permission count {expected}");
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
    async fn with_runtimes_uses_shared_runtime_instances_for_status_visibility() {
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
        let permission_runtime: Arc<dyn crate::permission_runtime::PermissionRuntime> =
            Arc::new(crate::permission_runtime::InMemoryPermissionRuntime::new(
                Arc::clone(&storage),
                Arc::clone(&bus),
            ));
        let question_runtime: Arc<dyn crate::question_runtime::QuestionRuntime> = Arc::new(
            crate::question_runtime::InMemoryQuestionRuntime::new(Arc::clone(&bus)),
        );

        let registry = Arc::new(ModelRegistry::new());
        registry
            .register("stub", Arc::new(StubProvider::pending()))
            .await;

        let engine = SessionEngine::with_runtimes(
            Arc::clone(&storage),
            Arc::clone(&bus),
            registry,
            Some("stub/test-model".into()),
            Arc::clone(&permission_runtime),
            Arc::clone(&question_runtime),
        );

        let ask_task = tokio::spawn({
            let question_runtime = Arc::clone(&question_runtime);
            async move {
                question_runtime
                    .ask(question_request(session_id, "shared_question"))
                    .await
            }
        });

        wait_for_pending_question_count(&engine, 1).await;
        let status = engine.status(session_id).await.unwrap();
        assert_eq!(
            status,
            SessionRuntimeStatus::Blocked {
                kind: crate::types::SessionBlockedKind::Question,
                request_id: "shared_question".into(),
            }
        );

        question_runtime
            .reply(crate::types::QuestionReply {
                session_id,
                request_id: "shared_question".into(),
                answers: vec![vec!["a".into()]],
            })
            .await
            .unwrap();
        assert_eq!(
            ask_task.await.unwrap().unwrap(),
            vec![vec![String::from("a")]]
        );
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
    async fn status_reports_busy_for_active_run_then_idle_after_cancel() {
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

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "long run".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        tokio::time::timeout(std::time::Duration::from_millis(200), started.notified())
            .await
            .expect("prompt should start");

        assert_eq!(
            engine.status(session_id).await.unwrap(),
            crate::types::SessionRuntimeStatus::Busy
        );

        engine.cancel(session_id).await.unwrap();
        let joined = tokio::time::timeout(std::time::Duration::from_millis(200), runner)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(joined, Err(SessionError::Cancelled)));

        assert_eq!(
            engine.status(session_id).await.unwrap(),
            crate::types::SessionRuntimeStatus::Idle
        );
    }

    #[tokio::test]
    async fn list_statuses_returns_only_busy_sessions_and_unknown_status_is_not_found() {
        let (storage, _file) = make_storage().await;
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        let unknown_id = SessionId::new();
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

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "long run".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        tokio::time::timeout(std::time::Duration::from_millis(200), started.notified())
            .await
            .expect("prompt should start");

        let statuses = engine.list_statuses().await.unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses.get(&session_id),
            Some(&crate::types::SessionRuntimeStatus::Busy)
        );

        let status_err = engine.status(unknown_id).await.unwrap_err();
        assert!(matches!(status_err, SessionError::NotFound(_)));

        engine.cancel(session_id).await.unwrap();
        let joined = tokio::time::timeout(std::time::Duration::from_millis(200), runner)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(joined, Err(SessionError::Cancelled)));

        assert!(engine.list_statuses().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn status_and_list_statuses_report_blocked_question_requests() {
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
                Arc::new(StubProvider::done()),
                Some("stub/test-model".into()),
            )
            .await,
        );

        let waiter = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .question_runtime
                    .ask(question_request(session_id, "question_status"))
                    .await
            })
        };

        wait_for_pending_question_count(&engine, 1).await;

        assert_eq!(
            engine.status(session_id).await.unwrap(),
            SessionRuntimeStatus::Blocked {
                kind: crate::types::SessionBlockedKind::Question,
                request_id: "question_status".into(),
            }
        );

        let statuses = engine.list_statuses().await.unwrap();
        assert_eq!(
            statuses.get(&session_id),
            Some(&SessionRuntimeStatus::Blocked {
                kind: crate::types::SessionBlockedKind::Question,
                request_id: "question_status".into(),
            })
        );

        let answered = engine
            .question_runtime
            .reply(crate::types::QuestionReply {
                session_id,
                request_id: "question_status".into(),
                answers: vec![vec!["a".into()]],
            })
            .await
            .unwrap();
        assert!(answered);

        let result = waiter.await.unwrap().unwrap();
        assert_eq!(result, vec![vec!["a".to_string()]]);
        assert_eq!(
            engine.status(session_id).await.unwrap(),
            SessionRuntimeStatus::Idle
        );
    }

    #[tokio::test]
    async fn cancel_rejects_pending_question_waiters_for_session() {
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
                Arc::new(StubProvider::done()),
                Some("stub/test-model".into()),
            )
            .await,
        );

        let waiter = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .question_runtime
                    .ask(question_request(session_id, "question_cancel"))
                    .await
            })
        };

        wait_for_pending_question_count(&engine, 1).await;
        engine.cancel(session_id).await.unwrap();

        let err = waiter.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("question request rejected"));
        assert!(engine.question_runtime.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn detached_prompt_accepts_immediately_and_starts_background_run() {
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

        let accepted = engine
            .prompt_detached(SessionPrompt {
                session_id,
                text: "detach".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap();
        assert_eq!(accepted.session_id, session_id);

        tokio::time::timeout(std::time::Duration::from_millis(200), started.notified())
            .await
            .expect("detached prompt should start provider stream");
        assert_eq!(
            engine.status(session_id).await.unwrap(),
            crate::types::SessionRuntimeStatus::Busy
        );

        engine.cancel(session_id).await.unwrap();
    }

    #[tokio::test]
    async fn detached_prompt_publishes_session_error_on_background_failure() {
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
            .register("stub", Arc::new(StubProvider::done()))
            .await;
        let engine = SessionEngine::new(
            Arc::clone(&storage),
            Arc::clone(&bus),
            registry,
            Some("invalid-model-id".into()),
        );

        let accepted = engine
            .prompt_detached(SessionPrompt {
                session_id,
                text: "detach-fail".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap();
        assert_eq!(accepted.session_id, session_id);

        let mut observed = None;
        for _ in 0..4 {
            let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
                .await
                .expect("expected bus event")
                .expect("event should decode");
            if let BusEvent::SessionError {
                session_id: sid,
                error,
            } = event
            {
                observed = Some((sid, error));
                break;
            }
        }

        let (sid, error) = observed.expect("detached failure should publish SessionError event");
        assert_eq!(sid, session_id);
        assert!(error.contains("invalid model id"));
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

    #[tokio::test]
    async fn supported_provider_tool_loop_persists_artifacts_and_completes() {
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

        let registry = Arc::new(ModelRegistry::new());
        registry
            .register("anthropic", Arc::new(ToolLoopProvider::new()))
            .await;
        let bus = Arc::new(BroadcastBus::default_capacity());
        let mut rx = bus.subscribe();

        let engine = Arc::new(SessionEngine::new(
            Arc::clone(&storage),
            Arc::clone(&bus),
            registry,
            Some("anthropic/test-model".into()),
        ));

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "list files".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        wait_for_pending_permission_count(&engine, 1).await;
        let permission = engine.permission_runtime.list().await.unwrap();
        engine
            .permission_runtime
            .reply(PermissionReply {
                session_id,
                request_id: permission[0].id.clone(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .unwrap();

        runner.await.unwrap().unwrap();

        let history = storage.list_history_with_parts(session_id).await.unwrap();
        assert!(
            history
                .iter()
                .any(|m| m.info.data["role"] == "tool" && m.parts[0].data["type"] == "tool_result")
        );
        assert!(history.iter().any(|m| {
            m.info.data["role"] == "assistant"
                && m.parts
                    .iter()
                    .any(|p| p.data["type"] == "tool_use" && p.data["name"] == "bash")
        }));

        let mut events = Vec::new();
        for _ in 0..16 {
            let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
                .await
                .expect("expected session events")
                .expect("bus event should be available");
            let done = matches!(
                event,
                BusEvent::SessionCompleted { session_id: sid } if sid == session_id
            );
            events.push(event);
            if done {
                break;
            }
        }

        let started_idx = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    BusEvent::ToolStarted {
                        session_id: sid,
                        tool,
                        call_id
                    } if *sid == session_id && tool == "bash" && call_id == "call_1"
                )
            })
            .expect("expected ToolStarted event for successful tool call");
        let finished_idx = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    BusEvent::ToolFinished {
                        session_id: sid,
                        tool,
                        call_id,
                        ok
                    } if *sid == session_id && tool == "bash" && call_id == "call_1" && *ok
                )
            })
            .expect("expected ToolFinished(ok=true) event for successful tool call");
        assert!(
            started_idx < finished_idx,
            "ToolStarted must be published before ToolFinished(ok=true)"
        );
    }

    #[tokio::test]
    async fn tool_loop_blocks_on_permission_runtime_and_resumes_after_reply() {
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

        let registry = Arc::new(ModelRegistry::new());
        registry
            .register("anthropic", Arc::new(ToolLoopProvider::new()))
            .await;

        let engine = Arc::new(SessionEngine::new(
            Arc::clone(&storage),
            Arc::new(BroadcastBus::default_capacity()),
            registry,
            Some("anthropic/test-model".into()),
        ));

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "list files".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        wait_for_pending_permission_count(&engine, 1).await;
        let pending = engine.permission_runtime.list().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].permission, "bash");
        assert_eq!(
            pending[0].tool.as_ref().map(|tool| tool.call_id.as_str()),
            Some("call_1")
        );

        let approved = engine
            .permission_runtime
            .reply(PermissionReply {
                session_id,
                request_id: pending[0].id.clone(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .unwrap();
        assert!(approved);

        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn tool_loop_blocks_on_question_runtime_and_resumes_after_reply() {
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

        let registry = Arc::new(ModelRegistry::new());
        registry
            .register("anthropic", Arc::new(ToolLoopQuestionProvider::new()))
            .await;

        let engine = Arc::new(SessionEngine::new(
            Arc::clone(&storage),
            Arc::new(BroadcastBus::default_capacity()),
            registry,
            Some("anthropic/test-model".into()),
        ));

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "question tool".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        wait_for_pending_permission_count(&engine, 1).await;
        let permission = engine.permission_runtime.list().await.unwrap();
        assert_eq!(permission.len(), 1);
        engine
            .permission_runtime
            .reply(PermissionReply {
                session_id,
                request_id: permission[0].id.clone(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .unwrap();

        wait_for_pending_question_count(&engine, 1).await;
        let questions = engine.question_runtime.list().await.unwrap();
        assert_eq!(questions.len(), 1);
        assert_eq!(
            questions[0].tool.as_ref().map(|tool| tool.call_id.as_str()),
            Some("call_question_1")
        );

        let answered = engine
            .question_runtime
            .reply(crate::types::QuestionReply {
                session_id,
                request_id: questions[0].id.clone(),
                answers: vec![vec!["dev".into()]],
            })
            .await
            .unwrap();
        assert!(answered);

        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancel_stops_in_flight_tool_capable_loop_and_releases_run_state() {
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

        let second_pass_started = Arc::new(tokio::sync::Notify::new());
        let registry = Arc::new(ModelRegistry::new());
        registry
            .register(
                "anthropic",
                Arc::new(ToolLoopSecondPassPendingProvider::new(Arc::clone(
                    &second_pass_started,
                ))),
            )
            .await;
        let engine = Arc::new(SessionEngine::new(
            Arc::clone(&storage),
            Arc::new(BroadcastBus::default_capacity()),
            registry,
            Some("anthropic/test-model".into()),
        ));

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "run tool then wait".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        wait_for_pending_permission_count(&engine, 1).await;
        let permission = engine.permission_runtime.list().await.unwrap();
        engine
            .permission_runtime
            .reply(PermissionReply {
                session_id,
                request_id: permission[0].id.clone(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .unwrap();

        tokio::time::timeout(
            std::time::Duration::from_millis(300),
            second_pass_started.notified(),
        )
        .await
        .expect("second provider pass should start after tool execution");

        engine.cancel(session_id).await.unwrap();

        let prompt_result = tokio::time::timeout(std::time::Duration::from_millis(300), runner)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(prompt_result, Err(SessionError::Cancelled)));

        let history = storage.list_history_with_parts(session_id).await.unwrap();
        assert!(history.iter().any(|message| {
            message.info.data["role"] == "assistant"
                && message.parts.iter().any(|part| {
                    part.data["type"] == "tool_use" && part.data["id"] == "call_cancel_1"
                })
        }));
        assert!(history.iter().any(|message| {
            message.info.data["role"] == "tool"
                && message.parts.iter().any(|part| {
                    part.data["type"] == "tool_result"
                        && part.data["tool_use_id"] == "call_cancel_1"
                        && part.data["is_error"] == false
                })
        }));

        let err = engine.cancel(session_id).await.unwrap_err();
        assert!(matches!(err, SessionError::NoActiveRun(_)));
    }

    #[tokio::test]
    async fn failed_tool_execution_persists_error_result_and_publishes_failed_lifecycle() {
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
            .register("anthropic", Arc::new(ToolLoopFailureProvider::new()))
            .await;
        let engine = Arc::new(SessionEngine::new(
            Arc::clone(&storage),
            Arc::clone(&bus),
            registry,
            Some("anthropic/test-model".into()),
        ));

        let runner = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .prompt(SessionPrompt {
                        session_id,
                        text: "trigger failure".into(),
                        model: None,
                        plan_mode: false,
                    })
                    .await
            })
        };

        wait_for_pending_permission_count(&engine, 1).await;
        let permission = engine.permission_runtime.list().await.unwrap();
        engine
            .permission_runtime
            .reply(PermissionReply {
                session_id,
                request_id: permission[0].id.clone(),
                reply: PermissionReplyKind::Once,
            })
            .await
            .unwrap();

        runner.await.unwrap().unwrap();

        let history = storage.list_history_with_parts(session_id).await.unwrap();
        let failed_tool_result = history
            .iter()
            .find(|message| message.info.data["role"] == "tool")
            .and_then(|message| message.parts.first())
            .expect("expected persisted tool result message");
        assert_eq!(failed_tool_result.data["type"], "tool_result");
        assert_eq!(failed_tool_result.data["tool_use_id"], "call_fail_1");
        assert_eq!(failed_tool_result.data["is_error"], true);
        assert!(
            failed_tool_result.data["content"]
                .as_str()
                .unwrap_or_default()
                .contains("invalid args for bash")
        );

        let mut events = Vec::new();
        for _ in 0..16 {
            let event = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
                .await
                .expect("expected session events")
                .expect("bus event should be available");
            let done = matches!(
                event,
                BusEvent::SessionCompleted { session_id: sid } if sid == session_id
            );
            events.push(event);
            if done {
                break;
            }
        }

        let started_idx = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    BusEvent::ToolStarted {
                        session_id: sid,
                        tool,
                        call_id
                    } if *sid == session_id && tool == "bash" && call_id == "call_fail_1"
                )
            })
            .expect("expected ToolStarted event for failed tool call");
        let finished_idx = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    BusEvent::ToolFinished {
                        session_id: sid,
                        tool,
                        call_id,
                        ok
                    } if *sid == session_id && tool == "bash" && call_id == "call_fail_1" && !ok
                )
            })
            .expect("expected ToolFinished(ok=false) event for failed tool call");
        assert!(
            started_idx < finished_idx,
            "ToolStarted must be published before ToolFinished(ok=false)"
        );
    }

    #[tokio::test]
    async fn out_of_scope_provider_tool_call_returns_provider_error() {
        struct StubToolCallProvider;

        #[async_trait]
        impl LanguageModel for StubToolCallProvider {
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
                _req: ModelRequest,
            ) -> Result<BoxStream<Result<ModelEvent, ProviderError>>, ProviderError> {
                Ok(Box::pin(stream::iter([
                    Ok(ModelEvent::ToolUseStart {
                        id: "call_1".into(),
                        name: "bash".into(),
                        thought_signature: None,
                    }),
                    Ok(ModelEvent::ToolUseInputDelta {
                        id: "call_1".into(),
                        delta: "{}".into(),
                    }),
                    Ok(ModelEvent::ToolUseEnd {
                        id: "call_1".into(),
                    }),
                ])))
            }
        }

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

        let registry = Arc::new(ModelRegistry::new());
        registry
            .register("stub", Arc::new(StubToolCallProvider))
            .await;
        let engine = SessionEngine::new(
            Arc::clone(&storage),
            Arc::new(BroadcastBus::default_capacity()),
            registry,
            Some("stub/test-model".into()),
        );

        let err = engine
            .prompt(SessionPrompt {
                session_id,
                text: "hi".into(),
                model: None,
                plan_mode: false,
            })
            .await
            .unwrap_err();

        match err {
            SessionError::Provider(message) => {
                assert!(
                    message.contains("does not support runtime tool execution in this MVP"),
                    "expected explicit deferred-scope signal, got: {message}"
                );
            }
            other => panic!("expected provider error, got {other}"),
        }

        let history = storage.list_history_with_parts(session_id).await.unwrap();
        assert!(history.iter().any(|message| {
            message.info.data["role"] == "assistant"
                && message
                    .parts
                    .iter()
                    .any(|part| part.data["type"] == "tool_use" && part.data["name"] == "bash")
        }));
        assert!(
            !history
                .iter()
                .any(|message| message.info.data["role"] == "tool"),
            "out-of-scope provider path must not pretend runtime tool execution by persisting tool_result"
        );
    }

    #[test]
    fn history_to_model_messages_preserves_tool_use_thought_signature() {
        let sid = SessionId::new();
        let mid = MessageId::new();
        let pid = PartId::new();

        let history = vec![MessageWithParts {
            info: MessageRow {
                id: mid,
                session_id: sid,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({ "role": "assistant" }),
            },
            parts: vec![PartRow {
                id: pid,
                message_id: mid,
                session_id: sid,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({
                    "type": "tool_use",
                    "id": "call_sig_1",
                    "name": "bash",
                    "input": { "command": "ls" },
                    "thought_signature": "sig-history"
                }),
            }],
        }];

        let messages = history_to_model_messages(&history);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert!(matches!(
            &messages[0].content[0],
            ContentPart::ToolUse {
                id,
                name,
                input,
                thought_signature,
            } if id == "call_sig_1"
                && name == "bash"
                && input["command"] == "ls"
                && thought_signature.as_deref() == Some("sig-history")
        ));
    }

    #[test]
    fn runtime_tool_ctx_uses_project_worktree_as_root() {
        let ctx = runtime_tool_ctx("/workspace/project", "/workspace/project/subdir");
        assert_eq!(ctx.root, std::path::PathBuf::from("/workspace/project"));
    }

    #[test]
    fn runtime_tool_ctx_uses_session_directory_as_cwd() {
        let ctx = runtime_tool_ctx("/workspace/project", "/workspace/project/session-cwd");
        assert_eq!(
            ctx.cwd,
            std::path::PathBuf::from("/workspace/project/session-cwd")
        );
    }
}
