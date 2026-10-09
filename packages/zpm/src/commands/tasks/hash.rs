use std::{
    collections::BTreeMap,
    process::ExitStatus,
};

use clipanion::cli;
use zpm_tasks::{
    TaskId,
    TaskName,
};
use zpm_utils::{
    Hash64,
    ToFileString,
};

use crate::{
    error::Error,
    project::Project,
    task_cache::{
        CacheRunOptions,
        CacheTaskInfo,
        FingerprintDetails,
        TaskCache,
        dependency_fingerprints,
        entry_path,
        tasks_needing_fingerprint,
        topological_order,
    },
};

/// Print the task cache fingerprint of a task
///
/// This command computes the fingerprint the task cache would use for the given task (and the tasks it depends on), without running
/// anything. It's useful to understand why a task doesn't get restored from the cache: compare the output of two runs to see which
/// inputs changed.
///
/// Environment variable values are printed as hashes so that secrets don't leak.
///
/// Fingerprints assume that the outputs of the tasks the target depends on are present on disk in their current state.
///
#[cli::command]
#[cli::path("tasks", "hash")]
#[cli::category("Task management commands")]
pub struct TaskHash {
    /// Print all the details as JSON (one object per task)
    #[cli::option("--json", default = false)]
    json: bool,

    /// Name of the task to fingerprint
    name: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskHashOutput {
    #[serde(flatten)]
    details: FingerprintDetails,
    is_cached: bool,
    has_entry: bool,
}

impl TaskHash {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = Project::new(None).await?;

        project.lazy_install().await?;

        let task_name = TaskName::new(&self.name)
            .map_err(|_| Error::TaskNameParseError(self.name.clone()))?;

        let root_task = TaskId {
            workspace: project.active_workspace()?.name.clone(),
            task_name,
        };

        let resolved
            = project.resolve_task(&root_task)?.resolved;

        let options
            = CacheRunOptions::from_project(&project, true)?;

        let task_cache
            = TaskCache::new(&project);

        let needed
            = tasks_needing_fingerprint(&resolved, resolved.tasks.keys())?;

        let mut fingerprints: BTreeMap<TaskId, Hash64>
            = BTreeMap::new();

        let mut outputs
            = Vec::new();

        for task_id in topological_order(&resolved.tasks) {
            if !needed.contains(&task_id) && task_id != root_task {
                continue;
            }

            let task = resolved.task_files.get(&task_id.workspace)
                .and_then(|tf| tf.tasks.get(task_id.task_name.as_str()))
                .expect("Resolved tasks always have a definition");

            let prerequisites
                = resolved.tasks.get(&task_id).cloned().unwrap_or_default();

            let Some(dependencies) = dependency_fingerprints(&prerequisites, |prerequisite| fingerprints.get(prerequisite).cloned()) else {
                continue;
            };

            let script
                = task.script.join("\n");

            if script.is_empty() {
                let fingerprint
                    = task_cache.aggregate_fingerprint(&task_id, &dependencies);

                fingerprints.insert(task_id.clone(), fingerprint.clone());

                // Aggregators never get an entry; they're reported with
                // their upstream fingerprints, the only thing they hash
                let is_cached
                    = task.cache_spec().ok().flatten().is_some();

                outputs.push(TaskHashOutput {
                    is_cached,
                    has_entry: false,
                    details: FingerprintDetails {
                        task: task_id.to_file_string(),
                        fingerprint: fingerprint.to_file_string(),
                        components: BTreeMap::from([("aggregate".to_string(), "no script".to_string())]),
                        env: BTreeMap::new(),
                        global_inputs: BTreeMap::new(),
                        inputs: BTreeMap::new(),
                        dependencies: dependencies.iter()
                            .map(|(task, hash)| (task.clone(), hash.to_file_string()))
                            .collect(),
                    },
                });

                continue;
            }

            let workspace
                = project.workspace_by_ident(&task_id.workspace)?;

            let info
                = CacheTaskInfo::new(&project, workspace, task_id.clone(), script, task.cache_spec().ok().flatten());

            let Some(fingerprint) = task_cache.compute_fingerprint(&info, &options, &dependencies)? else {
                continue;
            };

            fingerprints.insert(task_id.clone(), fingerprint.hash.clone());

            outputs.push(TaskHashOutput {
                is_cached: info.is_cached,
                has_entry: info.is_cached && entry_path(&options.cache_folder, &fingerprint.hash).fs_exists(),
                details: fingerprint.details,
            });
        }

        task_cache.save_state();

        if self.json {
            for output in &outputs {
                println!("{}", serde_json::to_string(output).unwrap());
            }

            return Ok(super::runner::exit_status_from_code(0));
        }

        let Some(root) = outputs.iter().find(|output| output.details.task == root_task.to_file_string()) else {
            return Err(Error::TaskCacheError(format!("{} can't be fingerprinted (it may depend on a long-lived task, or have no lockfile tree hash)", root_task.to_file_string())));
        };

        println!("Task:        {}", root.details.task);
        println!("Fingerprint: {}", root.details.fingerprint);
        println!("Cached:      {}", if !root.is_cached {"no (missing @cache)"} else if root.has_entry {"yes (entry found)"} else {"yes (no entry yet)"});
        println!();

        for (label, value) in &root.details.components {
            println!("{:<18} {}", label, value.replace('\n', "\\n"));
        }

        let sections: [(&str, &BTreeMap<String, String>); 4] = [
            ("Environment", &root.details.env),
            ("Global inputs", &root.details.global_inputs),
            ("Inputs", &root.details.inputs),
            ("Dependencies", &root.details.dependencies),
        ];

        for (title, entries) in sections {
            println!();
            println!("{} ({}):", title, entries.len());

            for (key, value) in entries {
                println!("  {}  {}", value, key);
            }
        }

        Ok(super::runner::exit_status_from_code(0))
    }
}
