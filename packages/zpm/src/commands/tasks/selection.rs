use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::Ident;
use zpm_tasks::TaskName;

use crate::{
    error::Error,
    git_utils,
    project::{Project, Workspace},
    workspace_glob::WorkspaceGlob,
};

/// Workspace selection for multi-workspace task runs. Mirrors turbo's
/// `--filter` / `--affected` semantics on top of the Yarn workspace graph
/// (`dependencies`, `devDependencies`, and `optionalDependencies` pointing
/// to other workspaces).
#[derive(Default)]
pub struct TaskSelection<'a> {
    /// Start from every workspace (`-A,--all`)
    pub all: bool,
    /// Start from the workspaces matching these globs (`--from`)
    pub from: &'a [WorkspaceGlob],
    /// Start from the workspaces changed since a ref (or the base refs when `Some(None)`), plus their dependents (`--since`, `--affected`)
    pub since: Option<Option<String>>,
    /// Add the transitive dependencies of the starting workspaces (turbo `pkg...`)
    pub with_dependencies: bool,
    /// Replace the starting workspaces by their transitive dependencies (turbo `pkg^...`)
    pub dependencies_only: bool,
    /// Add the transitive dependents of the starting workspaces (turbo `...pkg`)
    pub with_dependents: bool,
    /// Only keep workspaces matching these globs
    pub include: &'a [WorkspaceGlob],
    /// Drop workspaces matching these globs
    pub exclude: &'a [WorkspaceGlob],
}

impl<'a> TaskSelection<'a> {
    /// Whether the run targets an explicit set of workspaces rather than the
    /// active workspace only.
    pub fn is_active(&self) -> bool {
        self.all
            || !self.from.is_empty()
            || self.since.is_some()
            || self.with_dependencies
            || self.dependencies_only
            || self.with_dependents
            || !self.include.is_empty()
            || !self.exclude.is_empty()
    }

    pub async fn select_workspaces(&self, project: &Project) -> Result<BTreeSet<Ident>, Error> {
        let mut seeds: BTreeSet<Ident>
            = BTreeSet::new();

        let has_explicit_seeds
            = self.all || !self.from.is_empty() || self.since.is_some();

        if self.all {
            seeds.extend(project.workspaces.iter().map(|w| w.name.clone()));
        }

        if !self.from.is_empty() {
            seeds.extend(project.workspaces.iter()
                .filter(|w| self.from.iter().any(|glob| glob.check(w)))
                .map(|w| w.name.clone()));
        }

        if let Some(since) = &self.since {
            seeds.extend(git_utils::fetch_affected_workspaces(project, since.as_deref()).await?);
        }

        if !has_explicit_seeds {
            // `--include`/`--exclude` alone filter the whole project; the
            // graph flags alone apply to the active workspace.
            if self.with_dependencies || self.dependencies_only || self.with_dependents {
                seeds.insert(project.active_workspace()?.name.clone());
            } else {
                seeds.extend(project.workspaces.iter().map(|w| w.name.clone()));
            }
        }

        let dependency_map
            = workspace_dependency_map(project);

        let mut selection: BTreeSet<Ident>
            = match self.dependencies_only {
                true => BTreeSet::new(),
                false => seeds.clone(),
            };

        if self.with_dependencies || self.dependencies_only {
            let dependencies
                = traverse(&seeds, &dependency_map);

            selection.extend(dependencies.into_iter().filter(|ident| {
                // A workspace that's both a seed and a dependency of another
                // seed is still a dependency.
                !self.dependencies_only || !seeds.contains(ident) || is_reachable_from_others(ident, &seeds, &dependency_map)
            }));
        }

        if self.with_dependents {
            let dependent_map
                = invert(&dependency_map);

            selection.extend(traverse(&seeds, &dependent_map));
        }

        selection.retain(|ident| {
            let Ok(workspace) = project.workspace_by_ident(ident) else {
                return false;
            };

            self.matches_filters(workspace)
        });

        Ok(selection)
    }

    fn matches_filters(&self, workspace: &Workspace) -> bool {
        if !self.include.is_empty() && !self.include.iter().any(|glob| glob.check(workspace)) {
            return false;
        }

        !self.exclude.iter().any(|glob| glob.check(workspace))
    }

    /// Compute the `(workspace, task)` pairs to run: every selected
    /// workspace declaring one of the requested tasks (including through the
    /// root `@workspaces` defaults).
    pub async fn select_targets(&self, project: &Project, names: &[String]) -> Result<Vec<(Ident, String)>, Error> {
        let task_names: Vec<TaskName>
            = names.iter()
                .map(|name| TaskName::new(name).map_err(|_| Error::TaskNameParseError(name.clone())))
                .collect::<Result<_, _>>()?;

        let workspaces
            = self.select_workspaces(project).await?;

        let defaults
            = project.workspace_task_defaults();

        let mut targets
            = Vec::new();

        for ident in workspaces {
            let workspace
                = project.workspace_by_ident(&ident)?;

            for task_name in &task_names {
                if project.find_workspace_task(workspace, task_name, &defaults).is_some() {
                    targets.push((ident.clone(), task_name.as_str().to_string()));
                }
            }
        }

        Ok(targets)
    }
}

/// `workspace → workspaces it depends on` (hard dependencies only, by name).
pub fn workspace_dependency_map(project: &Project) -> BTreeMap<Ident, BTreeSet<Ident>> {
    let mut map
        = BTreeMap::new();

    for workspace in &project.workspaces {
        let dependencies: BTreeSet<Ident>
            = workspace.manifest.iter_hard_dependencies()
                .filter(|dependency| project.workspaces_by_ident.contains_key(dependency.ident))
                .filter(|dependency| *dependency.ident != workspace.name)
                .map(|dependency| dependency.ident.clone())
                .collect();

        map.insert(workspace.name.clone(), dependencies);
    }

    map
}

fn invert(map: &BTreeMap<Ident, BTreeSet<Ident>>) -> BTreeMap<Ident, BTreeSet<Ident>> {
    let mut inverted: BTreeMap<Ident, BTreeSet<Ident>>
        = BTreeMap::new();

    for (from, targets) in map {
        for to in targets {
            inverted.entry(to.clone())
                .or_default()
                .insert(from.clone());
        }
    }

    inverted
}

/// Every node reachable from the seeds through at least one edge (the seeds
/// themselves are only included if reachable from another seed).
fn traverse(seeds: &BTreeSet<Ident>, edges: &BTreeMap<Ident, BTreeSet<Ident>>) -> BTreeSet<Ident> {
    let mut seen
        = BTreeSet::new();

    let mut queue: Vec<&Ident>
        = seeds.iter().collect();

    while let Some(next) = queue.pop() {
        let Some(targets) = edges.get(next) else {
            continue;
        };

        for target in targets {
            if seen.insert(target.clone()) {
                queue.push(target);
            }
        }
    }

    seen
}

fn is_reachable_from_others(ident: &Ident, seeds: &BTreeSet<Ident>, edges: &BTreeMap<Ident, BTreeSet<Ident>>) -> bool {
    let others: BTreeSet<Ident>
        = seeds.iter()
            .filter(|seed| *seed != ident)
            .cloned()
            .collect();

    traverse(&others, edges).contains(ident)
}
