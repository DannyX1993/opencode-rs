//! Session engine types: requests, handles, frames, and state.

use opencode_core::id::{MessageId, SessionId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Runtime occupancy exposed to server handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRuntimeStatus {
    /// No active run currently leased for this session.
    Idle,
    /// A run is currently active for this session.
    Busy,
    /// Session is paused waiting for user input.
    Blocked {
        /// Which interactive runtime owns the pending request.
        kind: SessionBlockedKind,
        /// Correlation id for the pending request.
        request_id: String,
    },
}

impl Serialize for SessionRuntimeStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Idle => serializer.serialize_str("idle"),
            Self::Busy => serializer.serialize_str("busy"),
            Self::Blocked { kind, request_id } => {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct BlockedStatus<'a> {
                    #[serde(rename = "type")]
                    status_type: &'static str,
                    kind: &'a SessionBlockedKind,
                    #[serde(rename = "requestID")]
                    request_id: &'a str,
                }

                BlockedStatus {
                    status_type: "blocked",
                    kind,
                    request_id,
                }
                .serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for SessionRuntimeStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SessionRuntimeStatusWire {
            Simple(String),
            Blocked {
                #[serde(rename = "type")]
                status_type: Option<String>,
                kind: SessionBlockedKind,
                #[serde(rename = "requestID")]
                request_id: String,
            },
        }

        match SessionRuntimeStatusWire::deserialize(deserializer)? {
            SessionRuntimeStatusWire::Simple(value) if value == "idle" => Ok(Self::Idle),
            SessionRuntimeStatusWire::Simple(value) if value == "busy" => Ok(Self::Busy),
            SessionRuntimeStatusWire::Simple(value) => Err(serde::de::Error::custom(format!(
                "unknown session runtime status: {value}"
            ))),
            SessionRuntimeStatusWire::Blocked {
                status_type,
                kind,
                request_id,
            } => {
                if status_type
                    .as_deref()
                    .is_some_and(|value| value != "blocked")
                {
                    return Err(serde::de::Error::custom(
                        "blocked status type must be 'blocked'",
                    ));
                }
                Ok(Self::Blocked { kind, request_id })
            }
        }
    }
}

/// Runtime category that currently blocks a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionBlockedKind {
    /// Waiting on a permission reply.
    Permission,
    /// Waiting on a question reply/reject.
    Question,
}

/// Optional message+tool call linkage for runtime prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolCallRef {
    /// Owning message id.
    #[serde(rename = "messageID")]
    pub message_id: MessageId,
    /// Tool call correlation id.
    #[serde(rename = "callID")]
    pub call_id: String,
}

/// Pending permission request surfaced to server routes and events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    /// Runtime request id.
    pub id: String,
    /// Session id waiting on this permission.
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    /// Permission name, e.g. `bash`.
    pub permission: String,
    /// Requested permission patterns.
    pub patterns: Vec<String>,
    /// Arbitrary metadata associated with the ask.
    pub metadata: serde_json::Value,
    /// Patterns that can be persisted when user selects `always`.
    pub always: Vec<String>,
    /// Optional tool linkage.
    #[serde(default)]
    pub tool: Option<RuntimeToolCallRef>,
}

/// Client reply mode for a pending permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReplyKind {
    /// Approve this request once.
    Once,
    /// Approve and persist for future matching requests.
    Always,
    /// Reject the request.
    Reject,
}

/// Reply payload submitted by clients for permission asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReply {
    /// Session id owning the pending request.
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    /// Permission request id.
    #[serde(rename = "requestID")]
    pub request_id: String,
    /// Reply behavior.
    pub reply: PermissionReplyKind,
}

/// Multiple-choice option for runtime questions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    /// Display label for the option.
    pub label: String,
    /// Human-readable option description.
    pub description: String,
}

/// Runtime question descriptor shown to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionInfo {
    /// Complete user-visible question.
    pub question: String,
    /// Short prompt header.
    pub header: String,
    /// Available options.
    pub options: Vec<QuestionOption>,
    /// Whether multiple options can be selected.
    #[serde(default)]
    pub multiple: Option<bool>,
    /// Whether custom free-form answers are allowed.
    #[serde(default)]
    pub custom: Option<bool>,
}

/// Pending runtime question request surfaced to server routes and events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRequest {
    /// Runtime request id.
    pub id: String,
    /// Session id waiting on this question.
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    /// Ordered questions for the user.
    pub questions: Vec<QuestionInfo>,
    /// Optional tool linkage.
    #[serde(default)]
    pub tool: Option<RuntimeToolCallRef>,
}

/// Client reply payload for a pending question request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReply {
    /// Session id owning the pending question.
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    /// Question request id.
    #[serde(rename = "requestID")]
    pub request_id: String,
    /// Answers in the same order as `QuestionRequest::questions`.
    pub answers: Vec<Vec<String>>,
}

/// A request to begin a new prompt turn in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPrompt {
    /// Session to prompt into.
    pub session_id: SessionId,
    /// User message text.
    pub text: String,
    /// Optional model override for this turn.
    #[serde(default)]
    pub model: Option<String>,
    /// Run in plan-only mode (no write tools).
    #[serde(default)]
    pub plan_mode: bool,
}

/// An opaque handle returned when a prompt is submitted.
///
/// Callers can use the `session_id` to subscribe to bus events for streaming
/// updates.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    /// The session this handle belongs to.
    pub session_id: SessionId,
    /// Assistant message id allocated for this turn, when available.
    pub assistant_message_id: Option<MessageId>,
    /// Optional model actually selected for this turn.
    pub resolved_model: Option<String>,
}

/// Detached prompt acceptance metadata returned immediately to HTTP callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedPromptAccepted {
    /// Session that accepted detached execution.
    pub session_id: SessionId,
    /// Assistant message id allocated for this turn when available.
    #[serde(default)]
    pub assistant_message_id: Option<MessageId>,
    /// Optional resolved model selected for this run.
    #[serde(default)]
    pub resolved_model: Option<String>,
}

impl From<SessionHandle> for DetachedPromptAccepted {
    fn from(handle: SessionHandle) -> Self {
        Self {
            session_id: handle.session_id,
            assistant_message_id: handle.assistant_message_id,
            resolved_model: handle.resolved_model,
        }
    }
}

impl SessionHandle {
    /// Create a new handle with empty metadata.
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            assistant_message_id: None,
            resolved_model: None,
        }
    }

    /// Attach assistant message id metadata.
    pub fn with_assistant_message_id(mut self, message_id: MessageId) -> Self {
        self.assistant_message_id = Some(message_id);
        self
    }

    /// Attach resolved model metadata.
    pub fn with_resolved_model(mut self, model: impl Into<String>) -> Self {
        self.resolved_model = Some(model.into());
        self
    }
}

/// A streamed frame of output from the agent loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionFrame {
    /// A text delta from the LLM.
    TextDelta {
        /// The appended text.
        delta: String,
    },
    /// A tool call was dispatched.
    ToolCall {
        /// Tool name.
        tool: String,
        /// Correlation id.
        call_id: String,
    },
    /// A tool call completed.
    ToolResult {
        /// Correlation id.
        call_id: String,
        /// Whether it succeeded.
        ok: bool,
    },
    /// The turn is complete.
    Done {
        /// Total input tokens used in this turn.
        input_tokens: u32,
        /// Total output tokens generated in this turn.
        output_tokens: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_handle_can_be_created_without_metadata() {
        let sid = SessionId::new();
        let handle = SessionHandle::new(sid);

        assert_eq!(handle.session_id, sid);
        assert!(handle.assistant_message_id.is_none());
        assert!(handle.resolved_model.is_none());
    }

    #[test]
    fn session_handle_supports_runtime_metadata() {
        let sid = SessionId::new();
        let mid = MessageId::new();

        let handle = SessionHandle::new(sid)
            .with_assistant_message_id(mid)
            .with_resolved_model("gpt-4o-mini");

        assert_eq!(handle.session_id, sid);
        assert_eq!(handle.assistant_message_id, Some(mid));
        assert_eq!(handle.resolved_model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn runtime_status_serializes_to_idle_busy_shape() {
        assert_eq!(
            serde_json::to_value(SessionRuntimeStatus::Idle).unwrap(),
            serde_json::json!("idle")
        );
        assert_eq!(
            serde_json::to_value(SessionRuntimeStatus::Busy).unwrap(),
            serde_json::json!("busy")
        );
    }

    #[test]
    fn detached_prompt_acceptance_preserves_handle_metadata() {
        let sid = SessionId::new();
        let mid = MessageId::new();
        let accepted = DetachedPromptAccepted::from(
            SessionHandle::new(sid)
                .with_assistant_message_id(mid)
                .with_resolved_model("anthropic/claude-sonnet"),
        );

        assert_eq!(accepted.session_id, sid);
        assert_eq!(accepted.assistant_message_id, Some(mid));
        assert_eq!(
            accepted.resolved_model.as_deref(),
            Some("anthropic/claude-sonnet")
        );
    }

    #[test]
    fn runtime_status_supports_blocked_permission_shape() {
        let status = SessionRuntimeStatus::Blocked {
            kind: SessionBlockedKind::Permission,
            request_id: "perm_req_1".into(),
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({
                "type": "blocked",
                "kind": "permission",
                "requestID": "perm_req_1"
            })
        );
    }

    #[test]
    fn permission_and_question_requests_roundtrip() {
        let session_id = SessionId::new();
        let permission = PermissionRequest {
            id: "perm_1".into(),
            session_id,
            permission: "bash".into(),
            patterns: vec!["git:*".into()],
            metadata: serde_json::json!({"reason": "repo access"}),
            always: vec!["git:status".into()],
            tool: Some(RuntimeToolCallRef {
                message_id: MessageId::new(),
                call_id: "call_1".into(),
            }),
        };
        let payload = serde_json::to_value(&permission).unwrap();
        assert_eq!(payload["sessionID"], serde_json::json!(session_id));
        assert_eq!(payload["always"], serde_json::json!(["git:status"]));

        let question = QuestionRequest {
            id: "question_1".into(),
            session_id,
            questions: vec![QuestionInfo {
                question: "How should we proceed?".into(),
                header: "Proceed".into(),
                options: vec![QuestionOption {
                    label: "Continue".into(),
                    description: "Keep going".into(),
                }],
                multiple: Some(false),
                custom: Some(true),
            }],
            tool: None,
        };
        let encoded = serde_json::to_string(&question).unwrap();
        let decoded: QuestionRequest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.id, "question_1");
        assert_eq!(decoded.session_id, session_id);
    }

    #[test]
    fn permission_and_question_replies_encode_contract() {
        let session_id = SessionId::new();
        let permission = PermissionReply {
            session_id,
            request_id: "perm_1".into(),
            reply: PermissionReplyKind::Always,
        };
        assert_eq!(
            serde_json::to_value(permission).unwrap(),
            serde_json::json!({
                "sessionID": session_id,
                "requestID": "perm_1",
                "reply": "always"
            })
        );

        let question = QuestionReply {
            session_id,
            request_id: "question_1".into(),
            answers: vec![vec!["Yes".into()], vec!["No".into(), "Later".into()]],
        };
        assert_eq!(
            serde_json::to_value(question).unwrap(),
            serde_json::json!({
                "sessionID": session_id,
                "requestID": "question_1",
                "answers": [["Yes"], ["No", "Later"]]
            })
        );
    }
}
