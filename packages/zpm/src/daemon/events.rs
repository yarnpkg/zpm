use zpm_tasks::TaskId;

use super::scheduler::PreparedTask;

#[derive(Debug, Clone)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    pub fn as_str(&self) -> &'static str {
        match self {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    TaskReady {
        task_id: TaskId,
        prepared: PreparedTask,
    },
    TaskCompleted {
        task_id: TaskId,
        exit_code: i32,
    },
    TaskFailed {
        task_id: TaskId,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub enum ExecutorEvent {
    Started {
        task_id: String,
    },
    Output {
        task_id: String,
        line: String,
        stream: Stream,
    },
    Finished {
        task_id: String,
        exit_code: i32,
    },
    Failed {
        task_id: String,
        error: String,
    },
}
