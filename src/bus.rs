//! Event bus for pub/sub communication between modules.
//!
//! This module provides a simple event bus similar to opencode-ts's Bus module,
//! allowing different parts of the application to communicate through events.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Maximum number of events to buffer in channels
const CHANNEL_CAPACITY: usize = 1000;

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

    /// Subscribe to events of a specific type
    pub async fn subscribe<E: Event>(&self) -> broadcast::Receiver<E> {
        let type_id = TypeId::of::<E>();

        // First, try to get existing sender
        {
            let senders = self.senders.read().await;
            if let Some(sender) = senders.get(&type_id) {
                if let Some(tx) = sender.downcast_ref::<broadcast::Sender<E>>() {
                    return tx.subscribe();
                }
            }
        }

        // Create new sender if it doesn't exist
        let mut senders = self.senders.write().await;

        // Double-check after acquiring write lock
        if let Some(sender) = senders.get(&type_id) {
            if let Some(tx) = sender.downcast_ref::<broadcast::Sender<E>>() {
                return tx.subscribe();
            }
        }

        let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
        senders.insert(type_id, Box::new(tx));
        rx
    }
}

/// Trait for events that can be published on the bus
pub trait Event: Clone + Send + Sync + 'static {}

// Global event bus instance
lazy_static::lazy_static! {
    static ref GLOBAL_BUS: Arc<EventBus> = Arc::new(EventBus::new());
}

/// Get the global event bus
pub fn global() -> Arc<EventBus> {
    GLOBAL_BUS.clone()
}

/// Publish an event to the global bus
pub async fn publish<E: Event>(event: E) {
    global().publish(event).await;
}

/// Subscribe to events on the global bus
pub async fn subscribe<E: Event>() -> broadcast::Receiver<E> {
    global().subscribe().await
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
    async fn test_pub_sub() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe::<TestEvent>().await;

        let event = TestEvent {
            message: "hello".to_string(),
        };

        bus.publish(event.clone()).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received, event);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe::<TestEvent>().await;
        let mut rx2 = bus.subscribe::<TestEvent>().await;

        let event = TestEvent {
            message: "test".to_string(),
        };

        bus.publish(event.clone()).await;

        assert_eq!(rx1.recv().await.unwrap(), event);
        assert_eq!(rx2.recv().await.unwrap(), event);
    }
}
