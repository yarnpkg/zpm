use std::collections::{HashMap, VecDeque};

use super::super::ipc::BufferedOutputLine;
use super::super::scheduler::ContextualTaskId;

/// Owns output storage and closed-task eviction.
/// Only modified by the coordinator event loop — no locks needed.
pub struct OutputBuffer {
    /// Output lines per task
    buffer: HashMap<ContextualTaskId, Vec<BufferedOutputLine>>,
    /// Closed tasks in order (for LRU cleanup)
    closed_tasks: VecDeque<ContextualTaskId>,
    /// Max lines per task
    max_lines: usize,
    /// Max closed tasks to keep
    max_closed_tasks: usize,
}

impl OutputBuffer {
    pub fn new(max_lines: usize, max_closed_tasks: usize) -> Self {
        Self {
            buffer: HashMap::new(),
            closed_tasks: VecDeque::new(),
            max_lines,
            max_closed_tasks,
        }
    }

    pub fn append(&mut self, task_id: ContextualTaskId, line: BufferedOutputLine) {
        let lines = self.buffer.entry(task_id).or_default();
        lines.push(line);

        if lines.len() > self.max_lines {
            let excess = lines.len() - self.max_lines;
            lines.drain(0..excess);
        }
    }

    pub fn get(&self, task_id: &ContextualTaskId) -> Vec<BufferedOutputLine> {
        self.buffer.get(task_id).cloned().unwrap_or_default()
    }

    /// Mark a task as closed. Returns task IDs that were evicted
    /// (oldest closed tasks beyond the limit) so the caller can clean up
    /// related state in other sub-structs.
    pub fn mark_closed(&mut self, task_id: ContextualTaskId) -> Vec<ContextualTaskId> {
        self.closed_tasks.push_back(task_id);

        let mut evicted = Vec::new();

        while self.closed_tasks.len() > self.max_closed_tasks {
            if let Some(oldest_task_id) = self.closed_tasks.pop_front() {
                self.buffer.remove(&oldest_task_id);
                evicted.push(oldest_task_id);
            }
        }

        evicted
    }

    pub fn buffer_count(&self) -> usize {
        self.buffer.len()
    }

    pub fn closed_tasks_count(&self) -> usize {
        self.closed_tasks.len()
    }
}
