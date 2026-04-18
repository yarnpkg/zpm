use std::collections::{HashMap, HashSet};

use tokio::sync::{broadcast, mpsc};

use super::super::ipc::{DaemonNotification, SubscriptionScope};
use super::super::scheduler::ContextualTaskId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub u64);

#[derive(Debug, Clone)]
pub struct SubscriptionFilter {
    pub output_scope: SubscriptionScope,
    pub status_scope: SubscriptionScope,
    pub target_task_ids: HashSet<ContextualTaskId>,
    pub all_task_ids: HashSet<ContextualTaskId>,
    pub context_id: Option<String>,
}

impl SubscriptionFilter {
    pub fn new(output_scope: SubscriptionScope, status_scope: SubscriptionScope, context_id: Option<String>) -> Self {
        Self {
            output_scope,
            status_scope,
            target_task_ids: HashSet::new(),
            all_task_ids: HashSet::new(),
            context_id,
        }
    }

    pub fn matches(&self, notification: &DaemonNotification) -> bool {
        let (task_id, scope) = match notification {
            DaemonNotification::TaskOutputLine { task_id, .. } => (task_id, self.output_scope),
            DaemonNotification::TaskStarted { task_id } => (task_id, self.status_scope),
            DaemonNotification::TaskCompleted { task_id, .. } => (task_id, self.status_scope),
            DaemonNotification::TaskCancelled { task_id } => (task_id, self.status_scope),
            DaemonNotification::TaskWarmUpComplete { task_id } => (task_id, self.status_scope),
            // Global notifications are always delivered.
            DaemonNotification::DeclaredTasksChanged { .. } => return true,
        };

        let is_explicit_target = self.target_task_ids.contains(task_id);

        if let Some(ref ctx) = self.context_id {
            if !is_explicit_target && task_id.context_id != *ctx {
                return false;
            }
        }

        match scope {
            SubscriptionScope::None => false,
            SubscriptionScope::TargetOnly => is_explicit_target,
            SubscriptionScope::FullTree => {
                if is_explicit_target {
                    return true;
                }
                match &self.context_id {
                    Some(ctx) => task_id.context_id == *ctx,
                    None => true,
                }
            }
        }
    }

    pub fn add_target_task(&mut self, task_id: ContextualTaskId) {
        self.target_task_ids.insert(task_id.clone());
        self.all_task_ids.insert(task_id);
    }

    pub fn add_dependency_task(&mut self, task_id: ContextualTaskId) {
        self.all_task_ids.insert(task_id);
    }
}

struct Subscription {
    filter: SubscriptionFilter,
    sender: mpsc::UnboundedSender<DaemonNotification>,
}

/// Owns notification subscriptions.
/// Only modified by the coordinator event loop — no locks needed.
pub struct SubscriptionManager {
    subscriptions: HashMap<SubscriptionId, Subscription>,
    next_subscription_id: u64,
    /// Broadcast channel for global notifications (e.g. taskfile changes).
    /// All WebSocket connections subscribe to this on connect.
    global_tx: broadcast::Sender<DaemonNotification>,
}

impl SubscriptionManager {
    pub fn new() -> Self {
        let (global_tx, _) = broadcast::channel(64);
        Self {
            subscriptions: HashMap::new(),
            next_subscription_id: 1,
            global_tx,
        }
    }

    /// Subscribe to global notifications (taskfile changes, etc.).
    pub fn subscribe_global(&self) -> broadcast::Receiver<DaemonNotification> {
        self.global_tx.subscribe()
    }

    pub fn create(
        &mut self,
        output_scope: SubscriptionScope,
        status_scope: SubscriptionScope,
        context_id: Option<String>,
    ) -> (SubscriptionId, mpsc::UnboundedReceiver<DaemonNotification>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let filter = SubscriptionFilter::new(output_scope, status_scope, context_id);

        let id = SubscriptionId(self.next_subscription_id);
        self.next_subscription_id += 1;

        self.subscriptions.insert(id, Subscription { filter, sender: tx });

        (id, rx)
    }

    pub fn add_tasks(
        &mut self,
        subscription_id: SubscriptionId,
        target_task_ids: Vec<ContextualTaskId>,
        dependency_task_ids: Vec<ContextualTaskId>,
    ) {
        if let Some(sub) = self.subscriptions.get_mut(&subscription_id) {
            for task_id in target_task_ids {
                sub.filter.add_target_task(task_id);
            }
            for task_id in dependency_task_ids {
                sub.filter.add_dependency_task(task_id);
            }
        }
    }

    pub fn remove(&mut self, subscription_id: SubscriptionId) {
        self.subscriptions.remove(&subscription_id);
    }

    pub fn broadcast(&self, notification: DaemonNotification) {
        // Global notifications go through the broadcast channel.
        if matches!(notification, DaemonNotification::DeclaredTasksChanged { .. }) {
            let _ = self.global_tx.send(notification);
            return;
        }

        for sub in self.subscriptions.values() {
            if sub.filter.matches(&notification) {
                let _ = sub.sender.send(notification.clone());
            }
        }
    }
}
