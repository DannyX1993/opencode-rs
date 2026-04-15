//! All bus event variants and the `EventKind` discriminant.

use opencode_core::id::{MessageId, PartId, ProjectId, SessionId};
use serde::{Deserialize, Serialize};

/// Optional linkage between a runtime prompt and the originating tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolCallRef {
    /// Message that owns the tool call.
    #[serde(rename = "messageID")]
    pub message_id: MessageId,
    /// Tool call correlation id.
    #[serde(rename = "callID")]
    pub call_id: String,
}

/// Reply mode for permission prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReplyKind {
    /// Approve this request once.
    Once,
    /// Persist approval for future matches.
    Always,
    /// Reject the request.
    Reject,
}

/// Runtime question option displayed to users.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    /// Short option label.
    pub label: String,
    /// Option description.
    pub description: String,
}

/// Runtime question metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionInfo {
    /// Complete user-visible question.
    pub question: String,
    /// Short header used in compact UIs.
    pub header: String,
    /// Available answer options.
    pub options: Vec<QuestionOption>,
    /// Whether multiple options may be selected.
    #[serde(default)]
    pub multiple: Option<bool>,
    /// Whether free-form custom answers are allowed.
    #[serde(default)]
    pub custom: Option<bool>,
}

/// Discriminant for fast `subscribe_kind` filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventKind {
    /// Session lifecycle events.
    Session,
    /// Message/part content events.
    Message,
    /// Tool execution events.
    Tool,
    /// Provider / LLM events.
    Provider,
    /// Configuration changed.
    Config,
    /// Permission gate events.
    Permission,
    /// Todo list updated.
    Todo,
}

/// Every event that flows through the opencode in-process bus.
///
/// All variants are `Clone + Serialize + Deserialize` so they can be forwarded
/// over SSE/WebSocket connections by the server layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BusEvent {
    // ── Session lifecycle ───────────────────────────────────────────────────
    /// A new session was created.
    SessionCreated {
        /// Session that was created.
        session_id: SessionId,
        /// Owning project.
        project_id: ProjectId,
    },
    /// A session's metadata was updated.
    SessionUpdated {
        /// Session that was updated.
        session_id: SessionId,
    },
    /// A session was cancelled by the user.
    SessionCancelled {
        /// Session that was cancelled.
        session_id: SessionId,
    },
    /// A session finished its prompt loop normally.
    SessionCompleted {
        /// Session that completed.
        session_id: SessionId,
    },
    /// A session was compacted (context window reduction).
    SessionCompacted {
        /// Session that was compacted.
        session_id: SessionId,
        /// Approximate number of tokens freed.
        tokens_freed: u32,
    },
    /// A session run failed with a terminal error payload.
    SessionError {
        /// Session that failed.
        session_id: SessionId,
        /// Human-readable error description.
        error: String,
    },

    // ── Messages & Parts ───────────────────────────────────────────────────
    /// A new message was appended to a session.
    MessageAdded {
        /// Owning session.
        session_id: SessionId,
        /// The new message.
        message_id: MessageId,
    },
    /// A streaming part was appended to a message.
    PartAdded {
        /// Owning session.
        session_id: SessionId,
        /// Owning message.
        message_id: MessageId,
        /// The new part.
        part_id: PartId,
    },

    // ── Tools ──────────────────────────────────────────────────────────────
    /// A tool invocation started.
    ToolStarted {
        /// Session.
        session_id: SessionId,
        /// Tool name.
        tool: String,
        /// Call id (matches `ToolCall::id`).
        call_id: String,
    },
    /// A tool invocation completed.
    ToolFinished {
        /// Session.
        session_id: SessionId,
        /// Tool name.
        tool: String,
        /// Call id.
        call_id: String,
        /// Whether the tool succeeded.
        ok: bool,
    },

    // ── Provider ───────────────────────────────────────────────────────────
    /// Tokens were consumed in this turn.
    ProviderTokensUsed {
        /// Session.
        session_id: SessionId,
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// Input tokens used.
        input: u32,
        /// Output tokens generated.
        output: u32,
    },

    // ── Permissions ────────────────────────────────────────────────────────
    /// A permission request was raised and is now waiting for a reply.
    PermissionAsked {
        /// Session owning the pending request.
        session_id: SessionId,
        /// Runtime request id.
        request_id: String,
        /// Permission name (for example `bash`).
        permission: String,
        /// Permission patterns being requested.
        patterns: Vec<String>,
        /// Additional metadata associated with the request.
        metadata: serde_json::Value,
        /// Patterns eligible for durable `always` approval.
        always: Vec<String>,
        /// Optional originating tool linkage.
        #[serde(default)]
        tool: Option<RuntimeToolCallRef>,
    },
    /// A pending permission request received a reply.
    PermissionReplied {
        /// Session owning the request.
        session_id: SessionId,
        /// Runtime request id.
        request_id: String,
        /// Reply mode selected by the client.
        reply: PermissionReplyKind,
    },
    /// A question request was raised and is now waiting for a reply.
    QuestionAsked {
        /// Session owning the pending request.
        session_id: SessionId,
        /// Runtime request id.
        request_id: String,
        /// Ordered runtime questions.
        questions: Vec<QuestionInfo>,
        /// Optional originating tool linkage.
        #[serde(default)]
        tool: Option<RuntimeToolCallRef>,
    },
    /// A question request received answers.
    QuestionReplied {
        /// Session owning the request.
        session_id: SessionId,
        /// Runtime request id.
        request_id: String,
        /// Answers preserving the original question order.
        answers: Vec<Vec<String>>,
    },
    /// A pending question request was explicitly rejected.
    QuestionRejected {
        /// Session owning the request.
        session_id: SessionId,
        /// Runtime request id.
        request_id: String,
    },

    // ── Todos ──────────────────────────────────────────────────────────────
    /// The todo list was replaced for a session.
    TodosUpdated {
        /// Session.
        session_id: SessionId,
    },

    // ── Config ─────────────────────────────────────────────────────────────
    /// The global or project config was reloaded.
    ConfigChanged,
}

impl BusEvent {
    /// Return the coarse-grained [`EventKind`] for this event.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            Self::SessionCreated { .. }
            | Self::SessionUpdated { .. }
            | Self::SessionCancelled { .. }
            | Self::SessionCompleted { .. }
            | Self::SessionCompacted { .. }
            | Self::SessionError { .. } => EventKind::Session,

            Self::MessageAdded { .. } | Self::PartAdded { .. } => EventKind::Message,

            Self::ToolStarted { .. } | Self::ToolFinished { .. } => EventKind::Tool,

            Self::ProviderTokensUsed { .. } => EventKind::Provider,

            Self::PermissionAsked { .. }
            | Self::PermissionReplied { .. }
            | Self::QuestionAsked { .. }
            | Self::QuestionReplied { .. }
            | Self::QuestionRejected { .. } => EventKind::Permission,

            Self::TodosUpdated { .. } => EventKind::Todo,

            Self::ConfigChanged => EventKind::Config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::id::{MessageId, PartId, ProjectId, SessionId};

    fn sid() -> SessionId {
        SessionId::new()
    }
    fn pid() -> ProjectId {
        ProjectId::new()
    }
    fn mid() -> MessageId {
        MessageId::new()
    }
    fn par() -> PartId {
        PartId::new()
    }

    #[test]
    fn session_events_have_session_kind() {
        assert_eq!(
            BusEvent::SessionCreated {
                session_id: sid(),
                project_id: pid()
            }
            .kind(),
            EventKind::Session
        );
        assert_eq!(
            BusEvent::SessionUpdated { session_id: sid() }.kind(),
            EventKind::Session
        );
        assert_eq!(
            BusEvent::SessionCancelled { session_id: sid() }.kind(),
            EventKind::Session
        );
        assert_eq!(
            BusEvent::SessionCompleted { session_id: sid() }.kind(),
            EventKind::Session
        );
        assert_eq!(
            BusEvent::SessionCompacted {
                session_id: sid(),
                tokens_freed: 100
            }
            .kind(),
            EventKind::Session
        );
        assert_eq!(
            BusEvent::SessionError {
                session_id: sid(),
                error: "detached failed".into()
            }
            .kind(),
            EventKind::Session
        );
    }

    #[test]
    fn message_events_have_message_kind() {
        assert_eq!(
            BusEvent::MessageAdded {
                session_id: sid(),
                message_id: mid()
            }
            .kind(),
            EventKind::Message
        );
        assert_eq!(
            BusEvent::PartAdded {
                session_id: sid(),
                message_id: mid(),
                part_id: par()
            }
            .kind(),
            EventKind::Message
        );
    }

    #[test]
    fn tool_events_have_tool_kind() {
        assert_eq!(
            BusEvent::ToolStarted {
                session_id: sid(),
                tool: "bash".into(),
                call_id: "c1".into()
            }
            .kind(),
            EventKind::Tool
        );
        assert_eq!(
            BusEvent::ToolFinished {
                session_id: sid(),
                tool: "bash".into(),
                call_id: "c1".into(),
                ok: true
            }
            .kind(),
            EventKind::Tool
        );
    }

    #[test]
    fn other_events_have_correct_kinds() {
        assert_eq!(
            BusEvent::ProviderTokensUsed {
                session_id: sid(),
                provider: "anthropic".into(),
                model: "claude".into(),
                input: 10,
                output: 20
            }
            .kind(),
            EventKind::Provider
        );
        assert_eq!(
            BusEvent::PermissionAsked {
                session_id: sid(),
                request_id: "perm_1".into(),
                permission: "bash".into(),
                patterns: vec!["git:*".into()],
                metadata: serde_json::json!({"reason": "repo"}),
                always: vec!["git:status".into()],
                tool: None,
            }
            .kind(),
            EventKind::Permission
        );
        assert_eq!(
            BusEvent::PermissionReplied {
                session_id: sid(),
                request_id: "perm_1".into(),
                reply: PermissionReplyKind::Always,
            }
            .kind(),
            EventKind::Permission
        );
        assert_eq!(
            BusEvent::QuestionAsked {
                session_id: sid(),
                request_id: "q_1".into(),
                questions: vec![QuestionInfo {
                    question: "Continue?".into(),
                    header: "Continue".into(),
                    options: vec![QuestionOption {
                        label: "Yes".into(),
                        description: "Continue run".into(),
                    }],
                    multiple: Some(false),
                    custom: Some(true),
                }],
                tool: None,
            }
            .kind(),
            EventKind::Permission
        );
        assert_eq!(
            BusEvent::QuestionReplied {
                session_id: sid(),
                request_id: "q_1".into(),
                answers: vec![vec!["Yes".into()]],
            }
            .kind(),
            EventKind::Permission
        );
        assert_eq!(
            BusEvent::QuestionRejected {
                session_id: sid(),
                request_id: "q_1".into(),
            }
            .kind(),
            EventKind::Permission
        );
        assert_eq!(
            BusEvent::TodosUpdated { session_id: sid() }.kind(),
            EventKind::Todo
        );
        assert_eq!(BusEvent::ConfigChanged.kind(), EventKind::Config);
    }

    #[test]
    fn event_kind_serde_roundtrip() {
        let kind = EventKind::Session;
        let json = serde_json::to_string(&kind).unwrap();
        let back: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }
}
