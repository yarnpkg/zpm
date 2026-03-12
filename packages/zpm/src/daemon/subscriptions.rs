use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;

use super::ipc::{DaemonNotification, SubscriptionScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

#[derive(Debug, Clone)]
pub struct SubscriptionFilter {
    pub output_scope: SubscriptionScope,
    pub status_scope: SubscriptionScope,
    pub target_task_ids: HashSet<String>,
    pub all_task_ids: HashSet<String>,
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
        let (task_id, scope)
            = match notification {
                DaemonNotification::TaskOutputLine { task_id, .. } => (task_id, self.output_scope),
                DaemonNotification::TaskStarted { task_id } => (task_id, self.status_scope),
                DaemonNotification::TaskCompleted { task_id, .. } => (task_id, self.status_scope),
                DaemonNotification::TaskFailed { task_id, .. } => (task_id, self.status_scope),
                DaemonNotification::TaskWarmUpComplete { task_id } => (task_id, self.status_scope),
            };

        let is_explicit_target
            = self.target_task_ids.contains(task_id);

        if let Some(ref ctx) = self.context_id {
            if !is_explicit_target && !task_id.ends_with(&format!("@{}", ctx)) {
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
                    Some(ctx) => task_id.ends_with(&format!("@{}", ctx)),
                    None => true,
                }
            }
        }
    }

    pub fn add_target_task(&mut self, task_id: String) {
        self.target_task_ids.insert(task_id.clone());
        self.all_task_ids.insert(task_id);
    }

    pub fn add_dependency_task(&mut self, task_id: String) {
        self.all_task_ids.insert(task_id);
    }
}

struct SubscriptionEntry {
    filter: SubscriptionFilter,
    sender: mpsc::UnboundedSender<DaemonNotification>,
}

struct SubscriptionRegistryInner {
    subscriptions: HashMap<SubscriptionId, SubscriptionEntry>,
    next_id: u64,
}

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

    pub fn create_subscription(
        &self,
        output_scope: SubscriptionScope,
        status_scope: SubscriptionScope,
        context_id: Option<String>,
    ) -> (SubscriptionId, mpsc::UnboundedReceiver<DaemonNotification>) {
        let (tx, rx)
            = mpsc::unbounded_channel();

        let filter
            = SubscriptionFilter::new(output_scope, status_scope, context_id);

        let mut inner
            = self.inner.write().expect("subscription registry lock poisoned");

        let id
            = SubscriptionId(inner.next_id);

        inner.next_id += 1;

        inner.subscriptions.insert(
            id,
            SubscriptionEntry { filter, sender: tx },
        );

        (id, rx)
    }

    pub fn add_tasks_to_subscription(
        &self,
        subscription_id: SubscriptionId,
        target_task_ids: Vec<String>,
        dependency_task_ids: Vec<String>,
    ) {
        let mut inner
            = self.inner.write().expect("subscription registry lock poisoned");

        if let Some(entry) = inner.subscriptions.get_mut(&subscription_id) {
            for task_id in target_task_ids {
                entry.filter.add_target_task(task_id);
            }
            for task_id in dependency_task_ids {
                entry.filter.add_dependency_task(task_id);
            }
        }
    }

    pub fn remove_subscription(&self, subscription_id: SubscriptionId) {
        let mut inner
            = self.inner.write().expect("subscription registry lock poisoned");

        inner.subscriptions.remove(&subscription_id);
    }

    pub fn broadcast(&self, notification: DaemonNotification) {
        let inner
            = self.inner.read().expect("subscription registry lock poisoned");

        for entry in inner.subscriptions.values() {
            if entry.filter.matches(&notification) {
                let _ = entry.sender.send(notification.clone());
            }
        }
    }
}

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
