use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::Notify;
use zpm_tasks::TaskId;

#[derive(Debug, Clone)]
pub struct LongLivedEntry {
    pub task_id: TaskId,
    pub contextual_task_id: String,
    pub warm_up_complete: bool,
    pub process_id: Option<u32>,
    pub started_at: SystemTime,
}

/// Internal entry that tracks registration state
struct RegistrationEntry {
    entry: LongLivedEntry,
    /// When the registration was claimed (for orphan detection)
    claimed_at: Option<Instant>,
}

struct LongLivedRegistryInner {
    entries: HashMap<TaskId, RegistrationEntry>,
}

pub struct LongLivedRegistry {
    inner: RwLock<LongLivedRegistryInner>,
    /// Notified when a registration is completed or cancelled
    registration_notify: Notify,
}

/// Timeout for considering an in-progress registration as orphaned.
/// If a registration is claimed but not completed within this duration,
/// it will be cleaned up and a new registration can be claimed.
const ORPHAN_TIMEOUT: Duration = Duration::from_secs(30);

/// Result of attempting to claim or wait for a registration
pub enum RegistrationResult {
    /// Successfully attached to an existing, fully registered task
    AttachedToExisting(LongLivedEntry),
    /// Successfully claimed the registration - caller should create the task
    Claimed,
    /// Timed out waiting for another caller to complete registration
    TimedOut,
}

impl LongLivedRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(LongLivedRegistryInner {
                entries: HashMap::new(),
            }),
            registration_notify: Notify::new(),
        }
    }

    pub fn register(&self, task_id: TaskId, contextual_task_id: String) {
        let mut inner
            = self.inner.write().expect("long-lived registry lock poisoned");

        inner.entries.insert(
            task_id.clone(),
            RegistrationEntry {
                entry: LongLivedEntry {
                    task_id,
                    contextual_task_id,
                    warm_up_complete: false,
                    process_id: None,
                    started_at: SystemTime::now(),
                },
                claimed_at: None,
            },
        );
    }

    pub fn get_existing(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        let inner
            = self.inner.read().expect("long-lived registry lock poisoned");

        inner.entries.get(task_id).map(|r| r.entry.clone())
    }

    /// Atomically checks if a long-lived task exists, and if not, marks it as pending registration.
    /// Returns `Some(existing_entry)` if the task already exists, or `None` if this caller
    /// should proceed to create and register the task.
    ///
    /// This prevents race conditions where two concurrent callers both see "doesn't exist"
    /// and both try to create the same task.
    ///
    /// If an in-progress registration has been pending for longer than ORPHAN_TIMEOUT,
    /// it will be considered orphaned and this caller can claim it.
    pub fn try_claim_registration(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        let mut inner
            = self.inner.write().expect("long-lived registry lock poisoned");

        // If the task already exists, check its state
        if let Some(reg) = inner.entries.get(task_id) {
            // If registration is complete (has contextual_task_id), return it
            if !reg.entry.contextual_task_id.is_empty() {
                return Some(reg.entry.clone());
            }

            // Check if the in-progress registration is orphaned
            if let Some(claimed_at) = reg.claimed_at {
                if claimed_at.elapsed() > ORPHAN_TIMEOUT {
                    // Orphaned registration - remove it and let this caller claim
                    inner.entries.remove(task_id);
                } else {
                    // Still in progress, return the placeholder
                    return Some(reg.entry.clone());
                }
            } else {
                // Has entry but no claimed_at - shouldn't happen, but treat as complete
                return Some(reg.entry.clone());
            }
        }

        // Insert a placeholder entry to claim this task
        // The contextual_task_id will be updated when complete_registration() is called
        inner.entries.insert(
            task_id.clone(),
            RegistrationEntry {
                entry: LongLivedEntry {
                    task_id: task_id.clone(),
                    contextual_task_id: String::new(), // Placeholder, will be filled in
                    warm_up_complete: false,
                    process_id: None,
                    started_at: SystemTime::now(),
                },
                claimed_at: Some(Instant::now()),
            },
        );

        None
    }

    /// Wait for a registration to complete using async notification.
    /// This is more efficient than polling and uses tokio::sync::Notify.
    ///
    /// Returns the result of waiting:
    /// - `AttachedToExisting` if the task was registered by another caller
    /// - `Claimed` if the registration was orphaned and this caller claimed it
    /// - `TimedOut` if the timeout expired
    pub async fn wait_for_registration(
        self: &Arc<Self>,
        task_id: &TaskId,
        timeout: Duration,
    ) -> RegistrationResult {
        let deadline = Instant::now() + timeout;

        loop {
            // Check current state
            match self.try_claim_registration(task_id) {
                Some(entry) if !entry.contextual_task_id.is_empty() => {
                    // Registration completed
                    return RegistrationResult::AttachedToExisting(entry);
                }
                Some(_) => {
                    // Still in progress - wait for notification or timeout
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return RegistrationResult::TimedOut;
                    }

                    // Wait for notification with timeout
                    // Use a shorter wait to periodically check for orphaned registrations
                    let wait_duration = remaining.min(Duration::from_secs(1));
                    let _ = tokio::time::timeout(
                        wait_duration,
                        self.registration_notify.notified(),
                    )
                    .await;
                    // Continue loop to recheck state
                }
                None => {
                    // We claimed the registration (possibly after orphan cleanup)
                    return RegistrationResult::Claimed;
                }
            }
        }
    }

    /// Updates a previously claimed registration with the actual contextual task ID.
    /// Should be called after try_claim_registration returns None and the task has been scheduled.
    /// Notifies all waiters that the registration is complete.
    pub fn complete_registration(&self, task_id: &TaskId, contextual_task_id: String) {
        {
            let mut inner
                = self.inner.write().expect("long-lived registry lock poisoned");

            if let Some(reg) = inner.entries.get_mut(task_id) {
                reg.entry.contextual_task_id = contextual_task_id;
                reg.claimed_at = None; // Clear the claim timestamp
            }
        }
        // Notify all waiters that registration state has changed
        self.registration_notify.notify_waiters();
    }

    /// Removes a claimed registration if scheduling fails.
    /// Notifies all waiters so they can potentially claim the registration.
    pub fn cancel_registration(&self, task_id: &TaskId) {
        {
            let mut inner
                = self.inner.write().expect("long-lived registry lock poisoned");

            // Only remove if the contextual_task_id is still empty (placeholder)
            if let Some(reg) = inner.entries.get(task_id) {
                if reg.entry.contextual_task_id.is_empty() {
                    inner.entries.remove(task_id);
                }
            }
        }
        // Notify all waiters that registration state has changed
        self.registration_notify.notify_waiters();
    }

    pub fn set_process_id(&self, task_id: &TaskId, process_id: u32) {
        let mut inner
            = self.inner.write().expect("long-lived registry lock poisoned");

        if let Some(reg) = inner.entries.get_mut(task_id) {
            reg.entry.process_id = Some(process_id);
        }
    }

    pub fn mark_warm_up_complete(&self, task_id: &TaskId) -> bool {
        let mut inner
            = self.inner.write().expect("long-lived registry lock poisoned");

        if let Some(reg) = inner.entries.get_mut(task_id) {
            reg.entry.warm_up_complete = true;
            true
        } else {
            false
        }
    }

    pub fn is_warm_up_complete(&self, task_id: &TaskId) -> bool {
        let inner
            = self.inner.read().expect("long-lived registry lock poisoned");

        inner
            .entries
            .get(task_id)
            .map(|r| r.entry.warm_up_complete)
            .unwrap_or(false)
    }

    pub fn remove(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        let mut inner
            = self.inner.write().expect("long-lived registry lock poisoned");

        inner.entries.remove(task_id).map(|r| r.entry)
    }

    pub fn get_by_contextual_id(&self, contextual_task_id: &str) -> Option<LongLivedEntry> {
        let inner
            = self.inner.read().expect("long-lived registry lock poisoned");

        inner
            .entries
            .values()
            .find(|r| r.entry.contextual_task_id == contextual_task_id)
            .map(|r| r.entry.clone())
    }

    pub fn list_all_entries(&self) -> Vec<LongLivedEntry> {
        let inner
            = self.inner.read().expect("long-lived registry lock poisoned");

        inner.entries.values().map(|r| r.entry.clone()).collect()
    }
}
