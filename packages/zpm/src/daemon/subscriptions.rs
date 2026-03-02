use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;

use super::ipc::{DaemonNotification, SubscriptionScope};

/// Unique identifier for a subscription
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

/// Configuration for what notifications a subscription receives
#[derive(Debug, Clone)]
pub struct SubscriptionFilter {
    /// Scope for output notifications (TaskOutputLine)
    pub output_scope: SubscriptionScope,
    /// Scope for status notifications (TaskStarted, TaskCompleted, TaskFailed)
    pub status_scope: SubscriptionScope,
    /// Set of target task IDs (directly requested tasks)
    /// Used when scope is TargetOnly
    pub target_task_ids: HashSet<String>,
    /// Set of all task IDs in the dependency tree
    /// Used when scope is FullTree
    pub all_task_ids: HashSet<String>,
    /// Optional context ID to filter notifications by context
    /// If set, only notifications for tasks in this context are received
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

    /// Check if this notification should be sent based on the filter
    pub fn matches(&self, notification: &DaemonNotification) -> bool {
        let (task_id, scope) = match notification {
            DaemonNotification::TaskOutputLine { task_id, .. } => (task_id, self.output_scope),
            DaemonNotification::TaskStarted { task_id } => (task_id, self.status_scope),
            DaemonNotification::TaskCompleted { task_id, .. } => (task_id, self.status_scope),
            DaemonNotification::TaskFailed { task_id, .. } => (task_id, self.status_scope),
        };

        // Filter by context first if set
        if let Some(ref ctx) = self.context_id {
            if !task_id.ends_with(&format!("@{}", ctx)) {
                return false;
            }
        }

        match scope {
            SubscriptionScope::None => false,
            SubscriptionScope::TargetOnly => self.target_task_ids.contains(task_id),
            // FullTree accepts all notifications - important for dynamically pushed subtasks
            // that weren't known when the subscription was created
            SubscriptionScope::FullTree => true,
        }
    }

    /// Add a target task ID (directly requested task)
    pub fn add_target_task(&mut self, task_id: String) {
        self.target_task_ids.insert(task_id.clone());
        self.all_task_ids.insert(task_id);
    }

    /// Add a dependency task ID (part of tree, but not a target)
    pub fn add_dependency_task(&mut self, task_id: String) {
        self.all_task_ids.insert(task_id);
    }
}

/// Internal subscription entry in the registry
struct SubscriptionEntry {
    filter: SubscriptionFilter,
    sender: mpsc::UnboundedSender<DaemonNotification>,
}

struct SubscriptionRegistryInner {
    subscriptions: HashMap<SubscriptionId, SubscriptionEntry>,
    next_id: u64,
}

/// Thread-safe registry for managing notification subscriptions
pub struct SubscriptionRegistry {
    inner: RwLock<SubscriptionRegistryInner>,
}

impl SubscriptionRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(SubscriptionRegistryInner {
                subscriptions: HashMap::new(),
                next_id: 1,
            }),
        }
    }

    /// Create a new subscription with initial scopes and optional context filter.
    /// Returns the subscription ID and a receiver channel for notifications.
    ///
    /// IMPORTANT: The subscription is created with empty task ID sets.
    /// Task IDs must be added via `add_tasks_to_subscription` before
    /// tasks become visible to the coordinator.
    pub fn create_subscription(
        &self,
        output_scope: SubscriptionScope,
        status_scope: SubscriptionScope,
        context_id: Option<String>,
    ) -> (SubscriptionId, mpsc::UnboundedReceiver<DaemonNotification>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let filter = SubscriptionFilter::new(output_scope, status_scope, context_id);

        let mut inner = self.inner.write().unwrap();
        let id = SubscriptionId(inner.next_id);
        inner.next_id += 1;

        inner.subscriptions.insert(
            id,
            SubscriptionEntry { filter, sender: tx },
        );

        (id, rx)
    }

    /// Add task IDs to an existing subscription.
    /// This should be called atomically with adding tasks to the scheduler.
    pub fn add_tasks_to_subscription(
        &self,
        subscription_id: SubscriptionId,
        target_task_ids: Vec<String>,
        dependency_task_ids: Vec<String>,
    ) {
        let mut inner = self.inner.write().unwrap();
        if let Some(entry) = inner.subscriptions.get_mut(&subscription_id) {
            for task_id in target_task_ids {
                entry.filter.add_target_task(task_id);
            }
            for task_id in dependency_task_ids {
                entry.filter.add_dependency_task(task_id);
            }
        }
    }

    /// Remove a subscription from the registry.
    /// Called when a connection closes.
    pub fn remove_subscription(&self, subscription_id: SubscriptionId) {
        let mut inner = self.inner.write().unwrap();
        inner.subscriptions.remove(&subscription_id);
    }

    /// Broadcast a notification to all matching subscriptions.
    /// This is called by the coordinator when events occur.
    pub fn broadcast(&self, notification: DaemonNotification) {
        let inner = self.inner.read().unwrap();
        for entry in inner.subscriptions.values() {
            if entry.filter.matches(&notification) {
                // Ignore send errors - means the receiver was dropped
                let _ = entry.sender.send(notification.clone());
            }
        }
    }
}

/// RAII guard that removes a subscription when dropped.
/// Each connection should hold one of these per active subscription.
pub struct SubscriptionGuard {
    subscription_id: SubscriptionId,
    registry: Arc<SubscriptionRegistry>,
}

impl SubscriptionGuard {
    pub fn new(subscription_id: SubscriptionId, registry: Arc<SubscriptionRegistry>) -> Self {
        Self {
            subscription_id,
            registry,
        }
    }

    pub fn id(&self) -> SubscriptionId {
        self.subscription_id
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        self.registry.remove_subscription(self.subscription_id);
    }
}
