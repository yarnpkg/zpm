use std::collections::BTreeMap;

use crate::ast::{
    Task,
    TaskFile,
    TaskName,
};

/// Attribute marking a root taskfile task as a default applying to every
/// workspace of the project (similar to a `turbo.json` task definition).
pub const WORKSPACES_ATTRIBUTE: &str = "workspaces";

pub fn is_workspace_default(task: &Task) -> bool {
    task.attributes.iter().any(|attr| attr.name == WORKSPACES_ATTRIBUTE)
}

/// Extract the `@workspaces` tasks from the root taskfile.
pub fn extract_workspace_defaults(root_task_file: &TaskFile) -> BTreeMap<TaskName, Task> {
    root_task_file.tasks.iter()
        .filter(|(_, task)| is_workspace_default(task))
        .map(|(name, task)| (name.clone(), task.clone()))
        .collect()
}

/// Remove the `@workspaces` tasks from the root workspace's own taskfile;
/// they are templates for the other workspaces, not root tasks.
pub fn strip_workspace_defaults(task_file: &mut TaskFile) {
    task_file.tasks.retain(|_, task| !is_workspace_default(task));
}

/// Build the effective taskfile of a workspace by layering the root defaults
/// below its local taskfile (local definitions always win).
///
/// A default task without a script body is "script-backed": if the workspace
/// declares a `package.json` script of the same name it runs it (through
/// `yarn run`, so binaries and environment are the same as when running the
/// script manually), otherwise it's a no-op that still propagates ordering.
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
            = parse("@workspaces\nbuild: ^build\n\n@workspaces\ncheck: test:ci\n\nown:\n  echo own").unwrap();

        let defaults
            = extract_workspace_defaults(&root);

        assert_eq!(defaults.len(), 2);

        let local
            = parse("check:\n  echo local").unwrap();

        let tf
            = apply_workspace_defaults(Some(local), &defaults, |name| name == "build").unwrap();

        assert_eq!(tf.tasks["build"].script, vec!["yarn run build \"$@\""]);
        assert_eq!(tf.tasks["check"].script, vec!["echo local"]);
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
