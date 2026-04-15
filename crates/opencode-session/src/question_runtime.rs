//! Question runtime trait and in-memory service implementation.

use crate::types::{QuestionReply, QuestionRequest};
use async_trait::async_trait;
use opencode_bus::{BroadcastBus, BusEvent, EventBus};
use opencode_core::error::SessionError;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, oneshot};

const QUESTION_REJECTED_MESSAGE: &str = "question request rejected";

#[derive(Debug)]
struct PendingQuestion {
    request: QuestionRequest,
    responder: oneshot::Sender<Result<Vec<Vec<String>>, SessionError>>,
}

/// Runtime service responsible for question ask/reply/reject flows.
#[async_trait]
pub trait QuestionRuntime: Send + Sync {
    /// Register pending questions and block until answered.
    async fn ask(&self, req: QuestionRequest) -> Result<Vec<Vec<String>>, SessionError>;

    /// Reply to a pending question request.
    ///
    /// Returns `true` when a pending request was resolved, `false` for unknown ids.
    async fn reply(&self, req: QuestionReply) -> Result<bool, SessionError>;

    /// Reject a pending question request by id.
    ///
    /// Returns `true` when a pending request was rejected, `false` for unknown ids.
    async fn reject(&self, request_id: String) -> Result<bool, SessionError>;

    /// List all pending question requests.
    async fn list(&self) -> Result<Vec<QuestionRequest>, SessionError>;
}

/// In-memory question runtime backed by pending waiters.
pub struct InMemoryQuestionRuntime {
    /// Bus publisher for question lifecycle events.
    pub bus: Arc<BroadcastBus>,
    pending: Mutex<HashMap<String, PendingQuestion>>,
}

impl InMemoryQuestionRuntime {
    /// Build a new question runtime service.
    #[must_use]
    pub fn new(bus: Arc<BroadcastBus>) -> Self {
        Self {
            bus,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl QuestionRuntime for InMemoryQuestionRuntime {
    async fn ask(&self, req: QuestionRequest) -> Result<Vec<Vec<String>>, SessionError> {
        let request_id = req.id.clone();
        let session_id = req.session_id;
        let questions = req
            .questions
            .iter()
            .cloned()
            .map(to_bus_question_info)
            .collect();
        let tool = req.tool.clone().map(to_bus_tool_call_ref);

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(
            request_id.clone(),
            PendingQuestion {
                request: req,
                responder: tx,
            },
        );

        let _ = self.bus.publish(BusEvent::QuestionAsked {
            session_id,
            request_id,
            questions,
            tool,
        });

        match rx.await {
            Ok(result) => result,
            Err(_closed) => Err(SessionError::RuntimeInternal(
                "question waiter dropped before reply".into(),
            )),
        }
    }

    async fn reply(&self, req: QuestionReply) -> Result<bool, SessionError> {
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

        let _ = self.bus.publish(BusEvent::QuestionReplied {
            session_id: req.session_id,
            request_id: req.request_id,
            answers: req.answers.clone(),
        });

        let _ = pending.responder.send(Ok(req.answers));
        Ok(true)
    }

    async fn reject(&self, request_id: String) -> Result<bool, SessionError> {
        let pending = {
            let mut pending_map = self.pending.lock().await;
            pending_map.remove(&request_id)
        };

        let Some(pending) = pending else {
            return Ok(false);
        };

        let _ = self.bus.publish(BusEvent::QuestionRejected {
            session_id: pending.request.session_id,
            request_id,
        });
        let _ = pending.responder.send(Err(SessionError::RuntimeInternal(
            QUESTION_REJECTED_MESSAGE.into(),
        )));
        Ok(true)
    }

    async fn list(&self) -> Result<Vec<QuestionRequest>, SessionError> {
        let pending = self.pending.lock().await;
        Ok(pending.values().map(|item| item.request.clone()).collect())
    }
}

fn to_bus_question_info(question: crate::types::QuestionInfo) -> opencode_bus::QuestionInfo {
    opencode_bus::QuestionInfo {
        question: question.question,
        header: question.header,
        options: question
            .options
            .into_iter()
            .map(|option| opencode_bus::QuestionOption {
                label: option.label,
                description: option.description,
            })
            .collect(),
        multiple: question.multiple,
        custom: question.custom,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{QuestionInfo, QuestionOption};
    use opencode_core::id::SessionId;
    use tokio::time::{Duration, sleep, timeout};

    fn question_request(session_id: SessionId, request_id: &str) -> QuestionRequest {
        QuestionRequest {
            id: request_id.into(),
            session_id,
            questions: vec![
                QuestionInfo {
                    question: "Choose deployment region".into(),
                    header: "Region".into(),
                    options: vec![
                        QuestionOption {
                            label: "us-east".into(),
                            description: "US East".into(),
                        },
                        QuestionOption {
                            label: "eu-west".into(),
                            description: "EU West".into(),
                        },
                    ],
                    multiple: Some(false),
                    custom: Some(false),
                },
                QuestionInfo {
                    question: "Choose notification channels".into(),
                    header: "Notify".into(),
                    options: vec![
                        QuestionOption {
                            label: "email".into(),
                            description: "Email".into(),
                        },
                        QuestionOption {
                            label: "slack".into(),
                            description: "Slack".into(),
                        },
                    ],
                    multiple: Some(true),
                    custom: Some(true),
                },
            ],
            tool: None,
        }
    }

    async fn wait_for_pending_count(runtime: &InMemoryQuestionRuntime, expected: usize) {
        for _ in 0..50 {
            if runtime.list().await.expect("list pending").len() == expected {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for pending count {expected}");
    }

    #[tokio::test]
    async fn ask_blocks_until_reply_and_preserves_answer_order() {
        let runtime = Arc::new(InMemoryQuestionRuntime::new(Arc::new(
            BroadcastBus::default_capacity(),
        )));
        let session_id = SessionId::new();
        let request = question_request(session_id, "question_1");

        let waiter = {
            let runtime = Arc::clone(&runtime);
            let request = request.clone();
            tokio::spawn(async move { runtime.ask(request).await })
        };

        wait_for_pending_count(&runtime, 1).await;

        let answered = runtime
            .reply(QuestionReply {
                session_id,
                request_id: "question_1".into(),
                answers: vec![vec!["eu-west".into()], vec!["slack".into(), "email".into()]],
            })
            .await
            .expect("reply succeeds");
        assert!(answered);

        let result = timeout(Duration::from_millis(200), waiter)
            .await
            .expect("ask should complete")
            .expect("join ask task")
            .expect("ask should return answers");
        assert_eq!(
            result,
            vec![
                vec!["eu-west".to_string()],
                vec!["slack".to_string(), "email".to_string()]
            ]
        );
        assert!(runtime.list().await.expect("list pending").is_empty());
    }

    #[tokio::test]
    async fn reject_fails_blocked_waiter_and_clears_pending() {
        let runtime = Arc::new(InMemoryQuestionRuntime::new(Arc::new(
            BroadcastBus::default_capacity(),
        )));
        let session_id = SessionId::new();

        let waiter = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move {
                runtime
                    .ask(question_request(session_id, "question_2"))
                    .await
            })
        };

        wait_for_pending_count(&runtime, 1).await;

        let rejected = runtime
            .reject("question_2".into())
            .await
            .expect("reject should succeed");
        assert!(rejected);

        let err = timeout(Duration::from_millis(200), waiter)
            .await
            .expect("ask should complete")
            .expect("join ask task")
            .unwrap_err();
        assert!(err.to_string().contains("question request rejected"));
        assert!(runtime.list().await.expect("list pending").is_empty());
    }

    #[tokio::test]
    async fn unknown_reply_and_reject_are_noops() {
        let runtime = InMemoryQuestionRuntime::new(Arc::new(BroadcastBus::default_capacity()));
        let session_id = SessionId::new();

        let reply = runtime
            .reply(QuestionReply {
                session_id,
                request_id: "missing".into(),
                answers: vec![vec!["a".into()]],
            })
            .await
            .expect("reply should not fail");
        let reject = runtime
            .reject("missing".into())
            .await
            .expect("reject should not fail");

        assert!(!reply);
        assert!(!reject);
        assert!(runtime.list().await.expect("list pending").is_empty());
    }
}
