use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;
use zpm_primitives::{Ident, IdentGlob};
use zpm_utils::scc_tarjan_pearce;

use crate::ast::{Dependency, TaskFile, TaskId};
use crate::error::Error;

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedTasks {
    pub tasks: BTreeMap<TaskId, Vec<TaskId>>,
    pub task_files: BTreeMap<Ident, TaskFile>,
}

pub fn resolve<F, G, D>(
    root_task: &TaskId,
    get_task_file: F,
    resolve_ident_glob: G,
    is_dependency: D,
) -> Result<ResolvedTasks, Error>
where
    F: Fn(&Ident, Option<&str>) -> Option<TaskFile>,
    G: Fn(&IdentGlob, &Ident) -> Vec<Ident>,
    D: Fn(&Ident, &Ident) -> bool,
{
    let load_task_file_with_includes = |workspace: &Ident, task_files_cache: &mut BTreeMap<Ident, TaskFile>| -> Result<(), Error> {
        if task_files_cache.contains_key(workspace) {
            return Ok(());
        }

        let mut tf
            = get_task_file(workspace, None)
                .ok_or_else(|| Error::WorkspaceNotFound(workspace.clone()))?;

        for include in &tf.includes.clone() {
            if !is_dependency(workspace, &include.ident) {
                return Err(Error::IncludeNotDependency {
                    workspace: workspace.clone(),
                    include_ident: include.ident.clone(),
                });
            }

            let included_tf
                = get_task_file(&include.ident, include.path.as_deref())
                    .ok_or_else(|| Error::IncludeLoadError {
                    workspace: include.ident.clone(),
                    path: include.path.clone().unwrap_or_else(|| "taskfile".to_string()),
                })?;

            for (task_name, task) in included_tf.tasks {
                tf.tasks.entry(task_name).or_insert(task);
            }
        }

        task_files_cache.insert(workspace.clone(), tf);
        Ok(())
    };

    let mut task_data: HashMap<TaskId, Vec<(TaskId, bool)>>
        = HashMap::new();

    let mut graph: BTreeMap<TaskId, BTreeSet<TaskId>>
        = BTreeMap::new();

    let mut to_visit: Vec<TaskId>
        = Vec::new();

    let mut visited: HashSet<TaskId>
        = HashSet::new();

    let mut task_files: BTreeMap<Ident, TaskFile>
        = BTreeMap::new();

    to_visit.push(root_task.clone());

    while let Some(task_id) = to_visit.pop() {
        if visited.contains(&task_id) {
            continue;
        }
        visited.insert(task_id.clone());

        load_task_file_with_includes(&task_id.workspace, &mut task_files)?;

        let task_file
            = task_files.get(&task_id.workspace).unwrap();

        let task
            = task_file
            .tasks
            .get(task_id.task_name.as_str())
            .ok_or_else(|| Error::TaskNotFound {
                workspace: task_id.workspace.clone(),
                task_name: task_id.task_name.clone(),
            })?;

        let dependencies
            = task.dependencies.clone();

        let mut deps_with_parallel: Vec<(TaskId, bool)>
            = Vec::new();

        let mut all_deps: BTreeSet<TaskId>
            = BTreeSet::new();

        for dep in &dependencies {
            match dep {
                Dependency::Local { name, parallel } => {
                    let dep_id = TaskId {
                        workspace: task_id.workspace.clone(),
                        task_name: name.clone(),
                    };
                    deps_with_parallel.push((dep_id.clone(), *parallel));
                    all_deps.insert(dep_id.clone());
                    if !visited.contains(&dep_id) {
                        to_visit.push(dep_id);
                    }
                }
                Dependency::External { ident_glob, task_name, parallel } => {
                    let matching_workspaces
                        = resolve_ident_glob(ident_glob, &task_id.workspace);

                    for ws in matching_workspaces {
                        if !task_files.contains_key(&ws) {
                            if load_task_file_with_includes(&ws, &mut task_files).is_err() {
                                continue;
                            }
                        }

                        if let Some(ws_task_file) = task_files.get(&ws) {
                            if ws_task_file.tasks.contains_key(task_name.as_str()) {
                                let dep_id = TaskId {
                                    workspace: ws.clone(),
                                    task_name: task_name.clone(),
                                };
                                deps_with_parallel.push((dep_id.clone(), *parallel));
                                all_deps.insert(dep_id.clone());
                                if !visited.contains(&dep_id) {
                                    to_visit.push(dep_id);
                                }
                            }
                        }
                    }
                }
            }
        }

        task_data.insert(task_id.clone(), deps_with_parallel);
        graph.insert(task_id, all_deps);
    }

    let sccs
        = scc_tarjan_pearce(&graph);

    for scc in &sccs {
        if scc.len() > 1 {
            return Err(Error::CycleDetected(scc.clone()));
        }

        if scc.len() == 1 {
            let task_id = &scc[0];
            if let Some(deps) = graph.get(task_id) {
                if deps.contains(task_id) {
                    return Err(Error::CycleDetected(scc.clone()));
                }
            }
        }
    }

    let sorted: Vec<TaskId>
        = sccs.into_iter().map(|mut scc| scc.pop().unwrap()).collect();

    let mut happens_before: BTreeMap<TaskId, BTreeSet<TaskId>>
        = BTreeMap::new();

    for task_id in sorted.iter() {
        happens_before.insert(task_id.clone(), BTreeSet::new());
    }

    for (task_id, deps_with_parallel) in &task_data {
        let phases
            = build_dependency_phases(deps_with_parallel.clone());

        let mut barrier: BTreeSet<TaskId>
            = BTreeSet::new();

        for phase in &phases {
            for dep in phase {
                if let Some(entry) = happens_before.get_mut(dep) {
                    entry.extend(barrier.clone());
                }
            }

            barrier.extend(phase.iter().cloned());
        }

        if let Some(entry) = happens_before.get_mut(task_id) {
            entry.extend(barrier);
        }
    }

    for task_id in sorted.iter() {
        let direct_deps: BTreeSet<TaskId>
            = happens_before.get(task_id).cloned().unwrap_or_default();

        let mut all_deps
            = BTreeSet::new();

        for dep in &direct_deps {
            all_deps.insert(dep.clone());
            if let Some(dep_deps) = happens_before.get(dep) {
                all_deps.extend(dep_deps.clone());
            }
        }

        happens_before.insert(task_id.clone(), all_deps);
    }

    let tasks: BTreeMap<TaskId, Vec<TaskId>>
        = happens_before
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();

    Ok(ResolvedTasks { tasks, task_files })
}

fn build_dependency_phases(deps: Vec<(TaskId, bool)>) -> Vec<Vec<TaskId>> {
    if deps.is_empty() {
        return Vec::new();
    }

    let mut phases: Vec<Vec<TaskId>>
        = Vec::new();

    let mut current_parallel_group: Vec<TaskId>
        = Vec::new();

    for (task_id, parallel) in deps {
        if parallel {
            current_parallel_group.push(task_id);
        } else {
            if !current_parallel_group.is_empty() {
                phases.push(std::mem::take(&mut current_parallel_group));
            }

            phases.push(vec![task_id]);
        }
    }

    if !current_parallel_group.is_empty() {
        phases.push(current_parallel_group);
    }

    phases
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Task, TaskName};

    fn task_name(name: &str) -> TaskName {
        TaskName::new(name).unwrap()
    }

    fn make_task(deps: Vec<(&str, bool)>) -> Task {
        Task {
            attributes: Vec::new(),
            dependencies: deps
                .into_iter()
                .map(|(d, parallel)| Dependency::Local {
                    name: task_name(d),
                    parallel,
                })
                .collect(),
            script: Vec::new(),
        }
    }

    fn make_simple_task(deps: Vec<&str>) -> Task {
        make_task(deps.into_iter().map(|d| (d, false)).collect())
    }

    fn task_id(ws: &Ident, name: &str) -> TaskId {
        TaskId {
            workspace: ws.clone(),
            task_name: task_name(name),
        }
    }

    fn single_workspace_getter(
        ws: Ident,
        task_file: TaskFile,
    ) -> impl Fn(&Ident, Option<&str>) -> Option<TaskFile> {
        move |ident, _path| {
            if *ident == ws {
                Some(task_file.clone())
            } else {
                None
            }
        }
    }

    fn no_external_deps(_: &IdentGlob, _: &Ident) -> Vec<Ident> {
        vec![]
    }

    fn no_includes(_workspace: &Ident, _include_ident: &Ident) -> bool {
        false
    }

    fn make_task_file(tasks: BTreeMap<TaskName, Task>) -> TaskFile {
        TaskFile {
            includes: Vec::new(),
            tasks,
        }
    }

    #[test]
    fn test_resolve_simple_task() {
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("build"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "build"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(task_id(&ws, "build"), vec![]);

        assert_eq!(result.tasks, expected);
    }

    #[test]
    fn test_resolve_with_sequential_dependencies() {
        // c: a b (sequential)
        // Result: a: [], b: [a], c: [a, b]
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("c"), make_simple_task(vec!["a", "b"]));
        tasks.insert(task_name("a"), make_simple_task(vec![]));
        tasks.insert(task_name("b"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "c"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(task_id(&ws, "a"), vec![]);
        expected.insert(task_id(&ws, "b"), vec![task_id(&ws, "a")]);
        expected.insert(task_id(&ws, "c"), vec![task_id(&ws, "a"), task_id(&ws, "b")]);

        assert_eq!(result.tasks, expected);
    }

    #[test]
    fn test_resolve_with_parallel_dependencies() {
        // e: a b& c& d
        // Result: a: [], b: [a], c: [a], d: [a, b, c], e: [a, b, c, d]
        let mut tasks = BTreeMap::new();
        tasks.insert(
            task_name("e"),
            make_task(vec![("a", false), ("b", true), ("c", true), ("d", false)]),
        );
        tasks.insert(task_name("a"), make_simple_task(vec![]));
        tasks.insert(task_name("b"), make_simple_task(vec![]));
        tasks.insert(task_name("c"), make_simple_task(vec![]));
        tasks.insert(task_name("d"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "e"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(task_id(&ws, "a"), vec![]);
        expected.insert(task_id(&ws, "b"), vec![task_id(&ws, "a")]);
        expected.insert(task_id(&ws, "c"), vec![task_id(&ws, "a")]);
        expected.insert(
            task_id(&ws, "d"),
            vec![task_id(&ws, "a"), task_id(&ws, "b"), task_id(&ws, "c")],
        );
        expected.insert(
            task_id(&ws, "e"),
            vec![
                task_id(&ws, "a"),
                task_id(&ws, "b"),
                task_id(&ws, "c"),
                task_id(&ws, "d"),
            ],
        );

        assert_eq!(result.tasks, expected);
    }

    #[test]
    fn test_resolve_all_parallel() {
        // build: lint& typecheck& (both parallel)
        // Result: lint: [], typecheck: [], build: [lint, typecheck]
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("build"), make_task(vec![("lint", true), ("typecheck", true)]));
        tasks.insert(task_name("lint"), make_simple_task(vec![]));
        tasks.insert(task_name("typecheck"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "build"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(task_id(&ws, "lint"), vec![]);
        expected.insert(task_id(&ws, "typecheck"), vec![]);
        expected.insert(
            task_id(&ws, "build"),
            vec![task_id(&ws, "lint"), task_id(&ws, "typecheck")],
        );

        assert_eq!(result.tasks, expected);
    }

    #[test]
    fn test_resolve_chain() {
        // deploy: build, build: lint
        // Result: lint: [], build: [lint], deploy: [build, lint]
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("deploy"), make_simple_task(vec!["build"]));
        tasks.insert(task_name("build"), make_simple_task(vec!["lint"]));
        tasks.insert(task_name("lint"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "deploy"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(task_id(&ws, "lint"), vec![]);
        expected.insert(task_id(&ws, "build"), vec![task_id(&ws, "lint")]);
        expected.insert(
            task_id(&ws, "deploy"),
            vec![task_id(&ws, "build"), task_id(&ws, "lint")],
        );

        assert_eq!(result.tasks, expected);
    }

    #[test]
    fn test_detect_cycle() {
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("a"), make_simple_task(vec!["b"]));
        tasks.insert(task_name("b"), make_simple_task(vec!["c"]));
        tasks.insert(task_name("c"), make_simple_task(vec!["a"]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "a"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        );

        match result {
            Err(Error::CycleDetected(cycle)) => {
                assert_eq!(cycle.len(), 3);
                let names: Vec<_> = cycle.iter().map(|t| t.task_name.as_str()).collect();
                assert!(names.contains(&"a"));
                assert!(names.contains(&"b"));
                assert!(names.contains(&"c"));
            }
            _ => panic!("Expected CycleDetected error"),
        }
    }

    #[test]
    fn test_detect_self_referential_cycle() {
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("a"), make_simple_task(vec!["a"]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "a"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        );

        match result {
            Err(Error::CycleDetected(cycle)) => {
                assert_eq!(cycle.len(), 1);
                assert_eq!(cycle[0].task_name, "a");
            }
            _ => panic!("Expected CycleDetected error"),
        }
    }

    #[test]
    fn test_task_not_found() {
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("build"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "nonexistent"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        );

        assert!(matches!(result, Err(Error::TaskNotFound { .. })));
    }

    #[test]
    fn test_workspace_not_found() {
        let ws = Ident::new("my-package");
        let result = resolve(&task_id(&ws, "build"), |_, _| None, no_external_deps, no_includes);

        assert!(matches!(result, Err(Error::WorkspaceNotFound(_))));
    }

    #[test]
    fn test_diamond_dependency() {
        // build: lint typecheck (sequential)
        // lint: common
        // typecheck: common
        // Result: common: [], lint: [common], typecheck: [common, lint], build: [common, lint, typecheck]
        let mut tasks = BTreeMap::new();
        tasks.insert(task_name("build"), make_simple_task(vec!["lint", "typecheck"]));
        tasks.insert(task_name("lint"), make_simple_task(vec!["common"]));
        tasks.insert(task_name("typecheck"), make_simple_task(vec!["common"]));
        tasks.insert(task_name("common"), make_simple_task(vec![]));
        let task_file = make_task_file(tasks);

        let ws = Ident::new("my-package");
        let result = resolve(
            &task_id(&ws, "build"),
            single_workspace_getter(ws.clone(), task_file),
            no_external_deps,
            no_includes,
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(task_id(&ws, "common"), vec![]);
        expected.insert(task_id(&ws, "lint"), vec![task_id(&ws, "common")]);
        expected.insert(
            task_id(&ws, "typecheck"),
            vec![task_id(&ws, "common"), task_id(&ws, "lint")],
        );
        expected.insert(
            task_id(&ws, "build"),
            vec![
                task_id(&ws, "common"),
                task_id(&ws, "lint"),
                task_id(&ws, "typecheck"),
            ],
        );

        assert_eq!(result.tasks, expected);
    }

    #[test]
    fn test_dependency_phases_grouping() {
        // Test the build_dependency_phases function directly
        let ws = Ident::new("pkg");

        // a b& c& d -> phases [[a], [b, c], [d]]
        let deps = vec![
            (task_id(&ws, "a"), false),
            (task_id(&ws, "b"), true),
            (task_id(&ws, "c"), true),
            (task_id(&ws, "d"), false),
        ];

        let phases = build_dependency_phases(deps);

        assert_eq!(
            phases,
            vec![
                vec![task_id(&ws, "a")],
                vec![task_id(&ws, "b"), task_id(&ws, "c")],
                vec![task_id(&ws, "d")],
            ]
        );
    }

    #[test]
    fn test_dependency_phases_all_parallel() {
        let ws = Ident::new("pkg");

        // a& b& c& -> phases [[a, b, c]]
        let deps = vec![
            (task_id(&ws, "a"), true),
            (task_id(&ws, "b"), true),
            (task_id(&ws, "c"), true),
        ];

        let phases = build_dependency_phases(deps);

        assert_eq!(
            phases,
            vec![vec![task_id(&ws, "a"), task_id(&ws, "b"), task_id(&ws, "c")]]
        );
    }

    #[test]
    fn test_dependency_phases_all_sequential() {
        let ws = Ident::new("pkg");

        // a b c -> phases [[a], [b], [c]]
        let deps = vec![
            (task_id(&ws, "a"), false),
            (task_id(&ws, "b"), false),
            (task_id(&ws, "c"), false),
        ];

        let phases = build_dependency_phases(deps);

        assert_eq!(
            phases,
            vec![
                vec![task_id(&ws, "a")],
                vec![task_id(&ws, "b")],
                vec![task_id(&ws, "c")],
            ]
        );
    }
}
