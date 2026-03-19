use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::ipc::TaskEvent;

const MAX_EVENTS: usize = 1000;

/// Fixed-capacity ring buffer of task events.
pub struct EventHistory {
    events: VecDeque<TaskEvent>,
}

impl EventHistory {
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(MAX_EVENTS),
        }
    }

    pub fn push(&mut self, event: TaskEvent) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn list(&self) -> Vec<TaskEvent> {
        self.events.iter().cloned().collect()
    }
}

/// Return the current time as milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
