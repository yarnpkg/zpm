use std::collections::BTreeMap;

use crate::ast::{
    Task,
    TaskFile,
    TaskName,
};

/// Attribute marking a root taskfile task as a default applying to every
/// non-root workspace unless it defines its own task with the same name.
pub const DEFAULT_TASK_ATTRIBUTE: &str = "defaultTask";

pub fn is_workspace_default(task: &Task) -> bool {
    task.attributes.iter().any(|attr| attr.name == DEFAULT_TASK_ATTRIBUTE)
}

/// Extract the `@defaultTask` tasks from the root taskfile.
pub fn extract_workspace_defaults(root_task_file: &TaskFile) -> BTreeMap<TaskName, Task> {
    root_task_file.tasks.iter()
        .filter(|(_, task)| is_workspace_default(task))
        .map(|(name, task)| (name.clone(), task.clone()))
        .collect()
}

/// Remove the `@defaultTask` tasks from the root workspace's own taskfile;
/// they are templates for the other workspaces, not root tasks.
pub fn strip_workspace_defaults(task_file: &mut TaskFile) {
    task_file.tasks.retain(|_, task| !is_workspace_default(task));
}

/// Build the effective taskfile of a workspace by layering the root defaults
/// below its local taskfile (local definitions always win).
///
/// An inherited task without commands runs the workspace's matching
/// `package.json` script through `yarn run`, when present. Otherwise, it only
/// waits for its task dependencies. For example, with `build: *:build` and a
/// dependency chain app -> bridge -> core, app still waits for core's build
/// even if bridge has no build script.
pub fn apply_workspace_defaults<S>(
    local_task_file: Option<TaskFile>,
    defaults: &BTreeMap<TaskName, Task>,
    has_script: S,
) -> Option<TaskFile>
where
    S: Fn(&str) -> bool,
{
    if defaults.is_empty() {
        return local_task_file;
    }

    let mut task_file
        = local_task_file.unwrap_or_else(|| TaskFile {
            includes: Vec::new(),
            tasks: BTreeMap::new(),
        });

    for (name, task) in defaults {
        if task_file.tasks.contains_key(name) {
            continue;
        }

        let mut task
            = task.clone();

        if task.script.is_empty() && has_script(name.as_str()) {
            task.script.push(format!("yarn run {} \"$@\"", name.as_str()));
        }

        task_file.tasks.insert(name.clone(), task);
    }

    Some(task_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn test_defaults_layering() {
        let root
            = parse("@defaultTask\nbuild: *:build\n\n@defaultTask\ncheck: test-ci\n\nown:\n  echo own").unwrap();

        let defaults
            = extract_workspace_defaults(&root);

        assert_eq!(defaults.len(), 2);

        let local
            = parse("check:\n  echo local").unwrap();

        let tf
            = apply_workspace_defaults(Some(local), &defaults, |name| name == "build").unwrap();

        assert_eq!(tf.tasks["build"].script, vec!["yarn run build \"$@\""]);
        assert_eq!(tf.tasks["check"].script, vec!["echo local"]);
        assert!(tf.tasks["check"].dependencies.is_empty());
        assert!(!tf.tasks.contains_key("own"));

        let tf
            = apply_workspace_defaults(None, &defaults, |_| false).unwrap();

        assert!(tf.tasks["build"].script.is_empty());

        let mut root
            = root;

        strip_workspace_defaults(&mut root);
        assert_eq!(root.tasks.len(), 1);
    }
}
