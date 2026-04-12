//! # opencode-bus
//!
//! Typed in-process event bus replacing the Effect-TS `PubSub` / `GlobalBus`
//! used in the TypeScript codebase.
//!
//! All session lifecycle, tool, and provider events flow through [`EventBus`].
//! Consumers call [`EventBus::subscribe`] to get a `tokio::broadcast::Receiver`
//! that receives a clone of every published [`BusEvent`].

#![warn(missing_docs)]

mod bus;
mod event;

pub use bus::{BroadcastBus, BusError, EventBus};
pub use event::{BusEvent, EventKind};
