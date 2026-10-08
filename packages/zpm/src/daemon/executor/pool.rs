use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

use zpm_utils::Hash64;

use super::{
    super::{
        coordinator_commands::{CommandSender, CoordinatorCommand, TaskCompletionResult},
        coordinator_state::{ContextualTaskId, PreparedTask},
        events::Stream,
    },
    runner::TaskRunner,
};
use crate::{
    error::Error,
    task_cache::{
        CacheRunOptions,
        CacheTaskInfo,
        LogLine,
        TaskCache,
    },
};

/// Everything needed to fingerprint a task and look it up in the cache.
pub struct CacheJob {
    pub info: CacheTaskInfo,
    pub options: Arc<CacheRunOptions>,
    pub task_cache: Arc<TaskCache>,
    pub dependencies: BTreeMap<String, Hash64>,
}

enum CacheLookup {
    /// The outputs got restored; the logs must be replayed
    Hit {fingerprint: Hash64, logs: Vec<LogLine>},
    /// The task must run; store its results if `store` is set
    Miss {fingerprint: Hash64, store: bool},
    /// The task can't be fingerprinted in this context; just run it
    Uncacheable,
}

fn lookup(job: &CacheJob) -> Result<CacheLookup, Error> {
    let Some(fingerprint) = job.task_cache.compute_fingerprint(&job.info, &job.options, &job.dependencies)? else {
        return Ok(CacheLookup::Uncacheable);
    };

    if !job.info.is_cached {
        return Ok(CacheLookup::Miss {fingerprint: fingerprint.hash, store: false});
    }

    if job.options.read {
        if let Some(entry) = job.task_cache.read_entry(&job.options, &fingerprint.hash)? {
            job.task_cache.restore(&job.info, &entry)?;

            return Ok(CacheLookup::Hit {fingerprint: fingerprint.hash, logs: entry.meta.logs});
        }
    }

    Ok(CacheLookup::Miss {fingerprint: fingerprint.hash, store: true})
}

fn send_stderr(command_tx: &CommandSender, task_id: &ContextualTaskId, line: String) {
    let _ = command_tx.send(CoordinatorCommand::TaskOutput {
        task_id: task_id.clone(),
        line,
        stream: Stream::Stderr,
    });
}

/// ExecutorPool that communicates exclusively via commands.
/// All events including completion go through the command channel.
pub struct ExecutorPool {
    running: HashSet<ContextualTaskId>,
    daemon_url: String,
    command_tx: CommandSender,
}

impl ExecutorPool {
    pub fn new(daemon_url: String, command_tx: CommandSender) -> Self {
        Self {
            running: HashSet::new(),
            daemon_url,
            command_tx,
        }
    }

    pub fn spawn(&mut self, task_id: ContextualTaskId, prepared: PreparedTask, cache_job: Option<CacheJob>) {
        let daemon_url = self.daemon_url.clone();
        let command_tx = self.command_tx.clone();

        self.running.insert(task_id.clone());

        tokio::spawn(async move {
            let mut store_as: Option<(Hash64, CacheJob)> = None;

            if let Some(job) = cache_job {
                let (job, result)
                    = tokio::task::spawn_blocking(move || {
                        let result = lookup(&job);
                        (job, result)
                    }).await.expect("the cache lookup panicked");

                match result {
                    Ok(CacheLookup::Hit {fingerprint, logs}) => {
                        let _ = command_tx.send(CoordinatorCommand::TaskCacheHit {
                            task_id: task_id.clone(),
                            fingerprint: fingerprint.clone(),
                        });

                        for log in logs {
                            let _ = command_tx.send(CoordinatorCommand::TaskOutput {
                                task_id: task_id.clone(),
                                line: log.line,
                                stream: if log.stream == "stderr" {Stream::Stderr} else {Stream::Stdout},
                            });
                        }

                        let _ = command_tx.send(CoordinatorCommand::TaskFingerprint {
                            task_id: task_id.clone(),
                            fingerprint,
                        });

                        let _ = command_tx.send(CoordinatorCommand::TaskCompleted {
                            task_id,
                            result: TaskCompletionResult::Cached,
                        });

                        return;
                    },

                    Ok(CacheLookup::Miss {fingerprint, store}) => {
                        let _ = command_tx.send(CoordinatorCommand::TaskFingerprint {
                            task_id: task_id.clone(),
                            fingerprint: fingerprint.clone(),
                        });

                        if store {
                            store_as = Some((fingerprint, job));
                        }
                    },

                    Ok(CacheLookup::Uncacheable) => {},

                    Err(err) => {
                        send_stderr(&command_tx, &task_id, format!("Task cache error: {}", err));

                        let _ = command_tx.send(CoordinatorCommand::TaskCompleted {
                            task_id,
                            result: TaskCompletionResult::Error(err.to_string()),
                        });

                        return;
                    },
                }
            }

            let capture
                = store_as.as_ref().map(|_| Arc::new(Mutex::new(Vec::<LogLine>::new())));

            let runner = TaskRunner::new(prepared, task_id.clone(), daemon_url, command_tx.clone(), capture.clone());
            let result = runner.run().await;

            let succeeded
                = matches!(&result, Ok(status) if status.success());

            if let (true, Some((fingerprint, job)), Some(capture)) = (succeeded, store_as, capture) {
                let logs
                    = std::mem::take(&mut *capture.lock().unwrap());

                let store_result
                    = tokio::task::spawn_blocking(move || job.task_cache.store(&job.info, &job.options, &fingerprint, logs)).await;

                if let Ok(Err(err)) = store_result {
                    send_stderr(&command_tx, &task_id, format!("Failed to store the task results in the cache: {}", err));
                }
            }

            // Send TaskCompleted through the command channel.
            // This happens AFTER stream_output() completes, so all TaskOutput
            // commands are already in the channel ahead of this one.
            let completion_result = match result {
                Ok(status) => TaskCompletionResult::Exited(status),
                Err(e) => TaskCompletionResult::Error(e.to_string()),
            };

            let _ = command_tx.send(CoordinatorCommand::TaskCompleted {
                task_id,
                result: completion_result,
            });
        });
    }

    pub fn running_tasks(&self) -> impl Iterator<Item = &ContextualTaskId> {
        self.running.iter()
    }

    /// Mark a task as no longer running.
    /// Called when TaskCompleted is processed by the coordinator.
    pub fn mark_completed(&mut self, task_id: &ContextualTaskId) {
        self.running.remove(task_id);
    }
}
