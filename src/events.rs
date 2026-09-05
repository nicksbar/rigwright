//! Protocol-neutral unsolicited radio event routing.

use crate::{ControlId, MeterId, Mode};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

const MAX_SUBSCRIBER_EVENTS: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum RadioEvent {
    FrequencyChanged {
        frequency_hz: u64,
    },
    ModeChanged {
        mode: Mode,
    },
    PttChanged {
        enabled: bool,
    },
    ControlChanged {
        id: ControlId,
        value: crate::ControlValue,
    },
    MeterChanged {
        id: MeterId,
        value: u8,
    },
    ReceiverChanged {
        receiver: u8,
    },
    /// An unsolicited frame the protocol decoder did not map to a typed event.
    Raw {
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

#[derive(Debug, Default)]
struct RouterState {
    next_id: u64,
    subscribers: HashMap<SubscriptionId, VecDeque<RadioEvent>>,
    dropped_events: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RadioEventRouter(Arc<Mutex<RouterState>>);

impl RadioEventRouter {
    pub fn subscribe(&self) -> RadioEventSubscription {
        let mut state = self.0.lock().expect("radio event router lock poisoned");
        state.next_id = state.next_id.wrapping_add(1).max(1);
        let id = SubscriptionId(state.next_id);
        state.subscribers.insert(id, VecDeque::new());
        RadioEventSubscription {
            id,
            router: self.clone(),
        }
    }

    pub fn publish(&self, event: RadioEvent) {
        if let Ok(mut state) = self.0.lock() {
            let mut dropped = 0;
            for queue in state.subscribers.values_mut() {
                if queue.len() >= MAX_SUBSCRIBER_EVENTS {
                    queue.pop_front();
                    dropped += 1;
                }
                queue.push_back(event.clone());
            }
            state.dropped_events = state.dropped_events.saturating_add(dropped);
        }
    }

    pub fn dropped_events(&self) -> u64 {
        self.0
            .lock()
            .map(|state| state.dropped_events)
            .unwrap_or_default()
    }

    fn drain(&self, id: SubscriptionId) -> Vec<RadioEvent> {
        self.0
            .lock()
            .ok()
            .and_then(|mut state| {
                state
                    .subscribers
                    .get_mut(&id)
                    .map(|queue| queue.drain(..).collect())
            })
            .unwrap_or_default()
    }

    fn unsubscribe(&self, id: SubscriptionId) {
        if let Ok(mut state) = self.0.lock() {
            state.subscribers.remove(&id);
        }
    }
}

#[derive(Debug)]
pub struct RadioEventSubscription {
    id: SubscriptionId,
    router: RadioEventRouter,
}

impl RadioEventSubscription {
    pub fn id(&self) -> SubscriptionId {
        self.id
    }
    pub fn drain(&self) -> Vec<RadioEvent> {
        self.router.drain(self.id)
    }
}

impl Drop for RadioEventSubscription {
    fn drop(&mut self) {
        self.router.unsubscribe(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_are_independent_and_lifecycle_safe() {
        let router = RadioEventRouter::default();
        let first = router.subscribe();
        let second = router.subscribe();
        router.publish(RadioEvent::PttChanged { enabled: true });
        assert_eq!(first.drain().len(), 1);
        assert_eq!(second.drain().len(), 1);
        drop(first);
        router.publish(RadioEvent::FrequencyChanged {
            frequency_hz: 7_000_000,
        });
        assert_eq!(second.drain().len(), 1);
    }

    #[test]
    fn subscriber_queue_is_bounded_and_reports_drops() {
        let router = RadioEventRouter::default();
        let subscription = router.subscribe();
        for frequency_hz in 0..300 {
            router.publish(RadioEvent::FrequencyChanged { frequency_hz });
        }

        assert_eq!(subscription.drain().len(), 256);
        assert_eq!(router.dropped_events(), 44);
    }
}
