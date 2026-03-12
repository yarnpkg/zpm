use std::collections::HashSet;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use futures::Future;
use tokio::sync::mpsc;

use super::super::events::ExecutorEvent;
use super::super::process_registry::ProcessRegistry;
use super::super::scheduler::{format_contextual_task_id, ContextualTaskId, PreparedTask};
use super::output::OutputLine;
use super::runner::TaskRunner;
use crate::error::Error;

type TaskResult = (ContextualTaskId, Result<(ContextualTaskId, ExitStatus), Error>);
type TaskFuture = Pin<Box<dyn Future<Output = TaskResult> + Send>>;

pub struct ExecutorPool {
    tasks: FuturesUnordered<TaskFuture>,
    running: HashSet<ContextualTaskId>,
    event_tx: mpsc::UnboundedSender<ExecutorEvent>,
    daemon_url: String,
    process_registry: Arc<ProcessRegistry>,
}

impl ExecutorPool {
    pub fn new(event_tx: mpsc::UnboundedSender<ExecutorEvent>, daemon_url: String, process_registry: Arc<ProcessRegistry>) -> Self {
        Self {
            tasks: FuturesUnordered::new(),
            running: HashSet::new(),
            event_tx,
            daemon_url,
            process_registry,
        }
    }

    pub fn spawn(&mut self, task_id: ContextualTaskId, prepared: PreparedTask) {
        let task_id_str = format_contextual_task_id(&task_id);
        let event_tx = self.event_tx.clone();
        let task_id_clone = task_id.clone();
        let task_id_for_result = task_id.clone();
        let daemon_url = self.daemon_url.clone();
        let process_registry = self.process_registry.clone();

        let _ = event_tx.send(ExecutorEvent::Started {
            task_id: task_id_str.clone(),
        });

        let (output_tx, mut output_rx) = mpsc::unbounded_channel::<OutputLine>();

        let event_tx_output = event_tx.clone();
        let task_id_for_output = task_id_str.clone();

        tokio::spawn(async move {
            while let Some(output) = output_rx.recv().await {
                let _ = event_tx_output.send(ExecutorEvent::Output {
                    task_id: task_id_for_output.clone(),
                    line: output.line,
                    stream: output.stream,
                });
            }
        });

        self.running.insert(task_id.clone());

        let future: TaskFuture = Box::pin(async move {
            let runner = TaskRunner::new(prepared, task_id_str, daemon_url, process_registry);
            let result = runner.run(output_tx).await;
            (task_id_for_result, result.map(|status| (task_id_clone, status)))
        });

        self.tasks.push(future);
    }

    pub fn running_tasks(&self) -> impl Iterator<Item = &ContextualTaskId> {
        self.running.iter()
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    pub async fn wait_next(&mut self) -> Option<(ContextualTaskId, Result<ExitStatus, Error>)> {
        if self.running.is_empty() {
            return None;
        }

        // FuturesUnordered::next() is cancel-safe: dropping the future
        // does not remove any tasks from the stream
        let (completed_task_id, result) = self.tasks.next().await?;

        self.running.remove(&completed_task_id);

        let task_id_str = format_contextual_task_id(&completed_task_id);

        match result {
            Ok((_, status)) => {
                let exit_code = status.code().unwrap_or(-1);
                let _ = self.event_tx.send(ExecutorEvent::Finished {
                    task_id: task_id_str,
                    exit_code,
                });
                Some((completed_task_id, Ok(status)))
            }
            Err(e) => {
                let _ = self.event_tx.send(ExecutorEvent::Failed {
                    task_id: task_id_str,
                    error: e.to_string(),
                });
                Some((completed_task_id, Err(e)))
            }
        }
    }
}
