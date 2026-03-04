use std::collections::HashMap;
use std::sync::RwLock;
use std::time::SystemTime;

use zpm_tasks::TaskId;

#[derive(Debug, Clone)]
pub struct LongLivedEntry {
    pub task_id: TaskId,
    pub contextual_task_id: String,
    pub warm_up_complete: bool,
    pub process_id: Option<u32>,
    pub started_at: SystemTime,
}

struct LongLivedRegistryInner {
    entries: HashMap<TaskId, LongLivedEntry>,
}

pub struct LongLivedRegistry {
    inner: RwLock<LongLivedRegistryInner>,
}

impl LongLivedRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(LongLivedRegistryInner {
                entries: HashMap::new(),
            }),
        }
    }

    pub fn register(&self, task_id: TaskId, contextual_task_id: String) {
        let mut inner
            = self.inner.write().unwrap();

        inner.entries.insert(
            task_id.clone(),
            LongLivedEntry {
                task_id,
                contextual_task_id,
                warm_up_complete: false,
                process_id: None,
                started_at: SystemTime::now(),
            },
        );
    }

    pub fn get_existing(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        let inner
            = self.inner.read().unwrap();

        inner.entries.get(task_id).cloned()
    }

    pub fn set_process_id(&self, task_id: &TaskId, process_id: u32) {
        let mut inner
            = self.inner.write().unwrap();

        if let Some(entry) = inner.entries.get_mut(task_id) {
            entry.process_id = Some(process_id);
        }
    }

    pub fn mark_warm_up_complete(&self, task_id: &TaskId) -> bool {
        let mut inner
            = self.inner.write().unwrap();

        if let Some(entry) = inner.entries.get_mut(task_id) {
            entry.warm_up_complete = true;
            true
        } else {
            false
        }
    }

    pub fn is_warm_up_complete(&self, task_id: &TaskId) -> bool {
        let inner
            = self.inner.read().unwrap();

        inner
            .entries
            .get(task_id)
            .map(|e| e.warm_up_complete)
            .unwrap_or(false)
    }

    pub fn remove(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        let mut inner
            = self.inner.write().unwrap();

        inner.entries.remove(task_id)
    }

    pub fn get_by_contextual_id(&self, contextual_task_id: &str) -> Option<LongLivedEntry> {
        let inner
            = self.inner.read().unwrap();

        inner
            .entries
            .values()
            .find(|e| e.contextual_task_id == contextual_task_id)
            .cloned()
    }

    pub fn list_all_entries(&self) -> Vec<LongLivedEntry> {
        let inner
            = self.inner.read().unwrap();

        inner.entries.values().cloned().collect()
    }
}
