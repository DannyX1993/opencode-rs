//! `/api/v1/event` SSE route and bus-to-wire translation.

use std::{convert::Infallible, future::Future, pin::Pin};

use axum::{
    extract::State,
    response::{IntoResponse, Sse, sse::Event},
};
use futures::StreamExt;
use opencode_bus::{BusEvent, EventBus};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::state::{AppState, EventHeartbeat};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct ServerEventPayload {
    #[serde(rename = "type")]
    event_type: String,
    properties: serde_json::Value,
}

impl ServerEventPayload {
    fn connected() -> Self {
        Self {
            event_type: "server.connected".into(),
            properties: serde_json::json!({}),
        }
    }

    fn heartbeat() -> Self {
        Self {
            event_type: "server.heartbeat".into(),
            properties: serde_json::json!({}),
        }
    }
}

pub(crate) fn translate_bus_event(ev: &BusEvent) -> Option<ServerEventPayload> {
    match ev {
        BusEvent::SessionUpdated { session_id } => Some(ServerEventPayload {
            event_type: "session.updated".into(),
            properties: serde_json::json!({ "session_id": session_id }),
        }),
        BusEvent::MessageAdded {
            session_id,
            message_id,
        } => Some(ServerEventPayload {
            event_type: "message.added".into(),
            properties: serde_json::json!({
                "session_id": session_id,
                "message_id": message_id,
            }),
        }),
        BusEvent::PartAdded {
            session_id,
            message_id,
            part_id,
        } => Some(ServerEventPayload {
            event_type: "part.added".into(),
            properties: serde_json::json!({
                "session_id": session_id,
                "message_id": message_id,
                "part_id": part_id,
            }),
        }),
        BusEvent::ToolStarted {
            session_id,
            tool,
            call_id,
        } => Some(ServerEventPayload {
            event_type: "tool.started".into(),
            properties: serde_json::json!({
                "session_id": session_id,
                "tool": tool,
                "call_id": call_id,
            }),
        }),
        BusEvent::ToolFinished {
            session_id,
            tool,
            call_id,
            ok,
        } => Some(ServerEventPayload {
            event_type: "tool.finished".into(),
            properties: serde_json::json!({
                "session_id": session_id,
                "tool": tool,
                "call_id": call_id,
                "ok": ok,
            }),
        }),
        BusEvent::ProviderTokensUsed {
            session_id,
            provider,
            model,
            input,
            output,
        } => Some(ServerEventPayload {
            event_type: "provider.tokens.used".into(),
            properties: serde_json::json!({
                "session_id": session_id,
                "provider": provider,
                "model": model,
                "input": input,
                "output": output,
            }),
        }),
        BusEvent::SessionError { session_id, error } => Some(ServerEventPayload {
            event_type: "session.error".into(),
            properties: serde_json::json!({
                "session_id": session_id,
                "error": error,
            }),
        }),
        _ => None,
    }
}

async fn forward_sse_payloads(
    mut bus_rx: broadcast::Receiver<BusEvent>,
    tx: mpsc::Sender<ServerEventPayload>,
    heartbeat: EventHeartbeat,
) {
    if tx.send(ServerEventPayload::connected()).await.is_err() {
        return;
    }

    let mut next_heartbeat = heartbeat_future(&heartbeat);
    loop {
        tokio::select! {
            _ = &mut next_heartbeat => {
                if tx.send(ServerEventPayload::heartbeat()).await.is_err() {
                    break;
                }
                next_heartbeat = heartbeat_future(&heartbeat);
            }
            recv = bus_rx.recv() => {
                match recv {
                    Ok(event) => {
                        if let Some(payload) = translate_bus_event(&event)
                            && tx.send(payload).await.is_err() {
                                break;
                            }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

/// `GET /api/v1/event` — stream translated runtime events.
pub(crate) async fn stream(State(s): State<AppState>) -> impl IntoResponse {
    let (tx, rx) = mpsc::channel::<ServerEventPayload>(64);
    let bus_rx = s.bus.subscribe();
    tokio::spawn(forward_sse_payloads(bus_rx, tx, s.event_heartbeat));

    let stream = ReceiverStream::new(rx).map(|payload| {
        let data = serde_json::to_string(&payload).unwrap_or_else(|_| {
            serde_json::json!({"type": "server.error", "properties": {}}).to_string()
        });
        Ok::<Event, Infallible>(Event::default().data(data))
    });

    Sse::new(stream).into_response()
}

fn heartbeat_future(heartbeat: &EventHeartbeat) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
    match heartbeat {
        EventHeartbeat::Interval(duration) => Box::pin(tokio::time::sleep(*duration)),
        EventHeartbeat::Manual(notify) => Box::pin(notify.notified()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_bus::{BroadcastBus, EventBus};
    use opencode_core::id::{MessageId, SessionId};
    use std::time::Duration;

    #[test]
    fn translate_bus_event_maps_supported_variants() {
        let sid = SessionId::new();
        let mid = MessageId::new();

        let payload = translate_bus_event(&BusEvent::MessageAdded {
            session_id: sid,
            message_id: mid,
        })
        .expect("message.added should be translated");

        assert_eq!(payload.event_type, "message.added");
        assert_eq!(payload.properties["session_id"], sid.to_string());
        assert_eq!(payload.properties["message_id"], mid.to_string());
    }

    #[test]
    fn translate_bus_event_maps_additional_supported_variants() {
        let sid = SessionId::new();
        let mid = MessageId::new();
        let part_id = opencode_core::id::PartId::new();

        let session_updated =
            translate_bus_event(&BusEvent::SessionUpdated { session_id: sid }).unwrap();
        assert_eq!(session_updated.event_type, "session.updated");
        assert_eq!(session_updated.properties["session_id"], sid.to_string());

        let part_added = translate_bus_event(&BusEvent::PartAdded {
            session_id: sid,
            message_id: mid,
            part_id,
        })
        .unwrap();
        assert_eq!(part_added.event_type, "part.added");
        assert_eq!(part_added.properties["message_id"], mid.to_string());
        assert_eq!(part_added.properties["part_id"], part_id.to_string());

        let tool_started = translate_bus_event(&BusEvent::ToolStarted {
            session_id: sid,
            tool: "shell".into(),
            call_id: "call-1".into(),
        })
        .unwrap();
        assert_eq!(tool_started.event_type, "tool.started");
        assert_eq!(tool_started.properties["tool"], "shell");
        assert_eq!(tool_started.properties["call_id"], "call-1");

        let tool_finished = translate_bus_event(&BusEvent::ToolFinished {
            session_id: sid,
            tool: "shell".into(),
            call_id: "call-1".into(),
            ok: true,
        })
        .unwrap();
        assert_eq!(tool_finished.event_type, "tool.finished");
        assert_eq!(tool_finished.properties["ok"], true);

        let provider_tokens_used = translate_bus_event(&BusEvent::ProviderTokensUsed {
            session_id: sid,
            provider: "anthropic".into(),
            model: "claude".into(),
            input: 12,
            output: 34,
        })
        .unwrap();
        assert_eq!(provider_tokens_used.event_type, "provider.tokens.used");
        assert_eq!(provider_tokens_used.properties["provider"], "anthropic");
        assert_eq!(provider_tokens_used.properties["model"], "claude");
        assert_eq!(provider_tokens_used.properties["input"], 12);
        assert_eq!(provider_tokens_used.properties["output"], 34);

        let session_error = translate_bus_event(&BusEvent::SessionError {
            session_id: sid,
            error: "boom".into(),
        })
        .unwrap();
        assert_eq!(session_error.event_type, "session.error");
        assert_eq!(session_error.properties["error"], "boom");
    }

    #[test]
    fn translate_bus_event_drops_internal_or_unsupported_variants() {
        assert!(translate_bus_event(&BusEvent::ConfigChanged).is_none());
        assert!(
            translate_bus_event(&BusEvent::SessionCreated {
                session_id: SessionId::new(),
                project_id: opencode_core::id::ProjectId::new(),
            })
            .is_none()
        );
    }

    #[tokio::test]
    async fn forwarder_emits_connected_then_heartbeats_when_idle() {
        let bus = BroadcastBus::new(16);
        let (tx, mut rx) = mpsc::channel(16);
        let bus_rx = bus.subscribe();

        tokio::spawn(forward_sse_payloads(
            bus_rx,
            tx,
            EventHeartbeat::Interval(Duration::from_millis(5)),
        ));

        let first = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.event_type, "server.connected");

        let second = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.event_type, "server.heartbeat");
    }

    #[tokio::test]
    async fn forwarder_preserves_order_and_filters_unsupported_events() {
        let bus = BroadcastBus::new(16);
        let (tx, mut rx) = mpsc::channel(16);
        let bus_rx = bus.subscribe();

        tokio::spawn(forward_sse_payloads(
            bus_rx,
            tx,
            EventHeartbeat::Interval(Duration::from_millis(50)),
        ));
        let _ = rx.recv().await; // connected

        let sid = SessionId::new();
        let mid = MessageId::new();
        let _ = bus.publish(BusEvent::ConfigChanged);
        let _ = bus.publish(BusEvent::MessageAdded {
            session_id: sid,
            message_id: mid,
        });

        let next = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.event_type, "message.added");
        assert_eq!(next.properties["session_id"], sid.to_string());
    }

    #[tokio::test]
    async fn forwarder_recovers_from_lagged_bus_receivers() {
        let (bus_tx, bus_rx) = broadcast::channel(1);
        let (tx, mut rx) = mpsc::channel(16);

        let sid = SessionId::new();
        let mid = MessageId::new();

        let _ = bus_tx.send(BusEvent::ConfigChanged);
        let _ = bus_tx.send(BusEvent::ConfigChanged);

        tokio::spawn(forward_sse_payloads(
            bus_rx,
            tx,
            EventHeartbeat::Interval(Duration::from_secs(1)),
        ));

        let first = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.event_type, "server.connected");

        let _ = bus_tx.send(BusEvent::MessageAdded {
            session_id: sid,
            message_id: mid,
        });

        let next = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.event_type, "message.added");
        assert_eq!(next.properties["message_id"], mid.to_string());
    }

    #[tokio::test]
    async fn forwarder_stops_when_bus_is_closed() {
        let (bus_tx, bus_rx) = broadcast::channel(1);
        let (tx, mut rx) = mpsc::channel(16);

        tokio::spawn(forward_sse_payloads(
            bus_rx,
            tx,
            EventHeartbeat::Interval(Duration::from_secs(1)),
        ));

        let first = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.event_type, "server.connected");

        drop(bus_tx);

        let end = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap();
        assert!(end.is_none());
    }
}
