use std::collections::HashMap;
use std::process::ExitStatus;
use std::sync::Arc;

use futures::future::select_all;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::super::events::ExecutorEvent;
use super::super::process_registry::ProcessRegistry;
use super::super::scheduler::{format_contextual_task_id, ContextualTaskId, PreparedTask};
use super::output::OutputLine;
use super::runner::TaskRunner;
use crate::error::Error;

pub struct ExecutorPool {
    handles: HashMap<ContextualTaskId, JoinHandle<Result<(ContextualTaskId, ExitStatus), Error>>>,
    event_tx: mpsc::UnboundedSender<ExecutorEvent>,
    daemon_url: String,
    process_registry: Arc<ProcessRegistry>,
}

impl ExecutorPool {
    pub fn new(event_tx: mpsc::UnboundedSender<ExecutorEvent>, daemon_url: String, process_registry: Arc<ProcessRegistry>) -> Self {
        Self {
            handles: HashMap::new(),
            event_tx,
            daemon_url,
            process_registry,
        }
    }

    pub fn spawn(&mut self, task_id: ContextualTaskId, prepared: PreparedTask) {
        let task_id_str = format_contextual_task_id(&task_id);
        let event_tx = self.event_tx.clone();
        let task_id_clone = task_id.clone();
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

        let handle = tokio::spawn(async move {
            let runner = TaskRunner::new(prepared, task_id_str, daemon_url, process_registry);
            let result = runner.run(output_tx).await;
            result.map(|status| (task_id_clone, status))
        });

        self.handles.insert(task_id, handle);
    }

    pub fn running_tasks(&self) -> impl Iterator<Item = &ContextualTaskId> {
        self.handles.keys()
    }

    pub fn running_count(&self) -> usize {
        self.handles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    pub async fn wait_next(&mut self) -> Option<(ContextualTaskId, Result<ExitStatus, Error>)> {
        if self.handles.is_empty() {
            return None;
        }

        let handles: Vec<_> = self.handles.drain().collect();
        let task_ids: Vec<_> = handles.iter().map(|(id, _)| id.clone()).collect();
        let futures: Vec<_> = handles
            .into_iter()
            .map(|(_, h)| Box::pin(async move { h.await }))
            .collect();

        let (result, idx, remaining) = select_all(futures).await;

        for (future, task_id) in remaining.into_iter().zip(
            task_ids
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, id)| id.clone()),
        ) {
            self.handles.insert(
                task_id,
                tokio::spawn(async move { future.await.unwrap() }),
            );
        }

        let completed_task_id = task_ids[idx].clone();
        let task_id_str = format_contextual_task_id(&completed_task_id);

        match result {
            Ok(Ok((_, status))) => {
                let exit_code = status.code().unwrap_or(-1);
                let _ = self.event_tx.send(ExecutorEvent::Finished {
                    task_id: task_id_str,
                    exit_code,
                });
                Some((completed_task_id, Ok(status)))
            }
            Ok(Err(e)) => {
                let _ = self.event_tx.send(ExecutorEvent::Failed {
                    task_id: task_id_str,
                    error: e.to_string(),
                });
                Some((completed_task_id, Err(e)))
            }
            Err(e) => {
                let _ = self.event_tx.send(ExecutorEvent::Failed {
                    task_id: task_id_str,
                    error: e.to_string(),
                });
                Some((
                    completed_task_id,
                    Err(Error::TaskJoinError(e.to_string())),
                ))
            }
        }
    }
}
