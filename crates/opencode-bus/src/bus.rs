//! [`EventBus`] trait and [`BroadcastBus`] implementation.

use crate::event::{BusEvent, EventKind};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;

/// Default broadcast channel capacity.
const DEFAULT_CAPACITY: usize = 1024;

/// Errors emitted by the bus.
#[derive(Debug, Error)]
pub enum BusError {
    /// All receivers have been dropped; no one is listening.
    #[error("bus send failed — no active receivers")]
    NoReceivers,
}

/// The in-process event bus abstraction.
///
/// Implementors wrap a broadcast channel; multiple consumers can call
/// [`subscribe`](EventBus::subscribe) independently.
pub trait EventBus: Send + Sync {
    /// Publish an event to all current subscribers.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::NoReceivers`] when the channel has lagged or has no
    /// subscribers.  Callers should treat this as non-fatal.
    fn publish(&self, ev: BusEvent) -> Result<(), BusError>;

    /// Subscribe to all events.
    fn subscribe(&self) -> broadcast::Receiver<BusEvent>;

    /// Subscribe and filter to a specific [`EventKind`].
    ///
    /// Returns a plain `Receiver` — callers should skip events whose `kind()`
    /// does not match.  A filtered wrapper is provided via
    /// [`BroadcastBus::subscribe_kind`].
    fn subscribe_kind(&self, kind: EventKind) -> broadcast::Receiver<BusEvent>;
}

/// [`tokio::broadcast`]-backed implementation of [`EventBus`].
///
/// Cheap to clone — the inner channel is wrapped in `Arc`.
#[derive(Clone)]
pub struct BroadcastBus {
    tx: Arc<broadcast::Sender<BusEvent>>,
}

impl BroadcastBus {
    /// Create a new bus with the given channel capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx: Arc::new(tx) }
    }

    /// Create a new bus with the default capacity (1024).
    #[must_use]
    pub fn default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl Default for BroadcastBus {
    fn default() -> Self {
        Self::default_capacity()
    }
}

impl EventBus for BroadcastBus {
    fn publish(&self, ev: BusEvent) -> Result<(), BusError> {
        // `send` errors only when there are 0 receivers — treat as no-op.
        self.tx.send(ev).map(|_| ()).map_err(|_| BusError::NoReceivers)
    }

    fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.tx.subscribe()
    }

    fn subscribe_kind(&self, kind: EventKind) -> broadcast::Receiver<BusEvent> {
        // We return the same receiver; callers filter by kind.
        // A future version could maintain per-kind sub-channels if needed.
        let _ = kind;
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_core::id::{ProjectId, SessionId};

    fn session_created() -> BusEvent {
        BusEvent::SessionCreated {
            session_id: SessionId::new(),
            project_id: ProjectId::new(),
        }
    }

    #[tokio::test]
    async fn single_subscriber_receives_event() {
        let bus = BroadcastBus::default_capacity();
        let mut rx = bus.subscribe();
        bus.publish(session_created()).unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, BusEvent::SessionCreated { .. }));
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive() {
        let bus = BroadcastBus::default_capacity();
        let mut rxs: Vec<_> = (0..3).map(|_| bus.subscribe()).collect();
        bus.publish(session_created()).unwrap();
        for rx in &mut rxs {
            let ev = rx.recv().await.unwrap();
            assert!(matches!(ev, BusEvent::SessionCreated { .. }));
        }
    }

    #[tokio::test]
    async fn publish_with_no_receivers_is_non_fatal() {
        let bus = BroadcastBus::default_capacity();
        // No subscribers — should return BusError but not panic.
        let result = bus.publish(session_created());
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn hundred_events_delivered() {
        let bus = BroadcastBus::new(512);
        let mut rx = bus.subscribe();
        for _ in 0..100 {
            bus.publish(BusEvent::ConfigChanged).unwrap();
        }
        let mut count = 0usize;
        while let Ok(_) = rx.try_recv() {
            count += 1;
        }
        assert_eq!(count, 100);
    }
}
