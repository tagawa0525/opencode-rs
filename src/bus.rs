//! Event bus for pub/sub communication between modules.
//!
//! This module provides a simple event bus similar to opencode-ts's Bus module,
//! allowing different parts of the application to communicate through events.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// A type-erased event sender
type BoxedSender = Box<dyn Any + Send + Sync>;

/// Global event bus instance
pub struct EventBus {
    senders: RwLock<HashMap<TypeId, BoxedSender>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            senders: RwLock::new(HashMap::new()),
        }
    }

    /// Publish an event to all subscribers
    pub async fn publish<E: Event>(&self, event: E) {
        let type_id = TypeId::of::<E>();
        let senders = self.senders.read().await;

        if let Some(sender) = senders.get(&type_id) {
            if let Some(tx) = sender.downcast_ref::<broadcast::Sender<E>>() {
                let _ = tx.send(event);
            }
        }
    }
}

/// Trait for events that can be published on the bus
pub trait Event: Clone + Send + Sync + 'static {}

// Global event bus instance
static GLOBAL_BUS: std::sync::LazyLock<Arc<EventBus>> =
    std::sync::LazyLock::new(|| Arc::new(EventBus::new()));

/// Get the global event bus
pub fn global() -> Arc<EventBus> {
    GLOBAL_BUS.clone()
}

/// Publish an event to the global bus
pub async fn publish<E: Event>(event: E) {
    global().publish(event).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TestEvent {
        message: String,
    }

    impl Event for TestEvent {}

    #[tokio::test]
    async fn test_publish() {
        let bus = EventBus::new();

        let event = TestEvent {
            message: "hello".to_string(),
        };

        bus.publish(event.clone()).await;
        // Just verify publish doesn't panic
    }
}
