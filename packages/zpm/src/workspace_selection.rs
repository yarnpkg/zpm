use std::collections::{BTreeMap, BTreeSet};

use zpm_macro_enum::zpm_enum;
use zpm_primitives::Ident;

use crate::{
    error::Error,
    git_utils,
    project::{Project, Workspace},
    workspace_glob::WorkspaceGlob,
};

#[zpm_enum]
#[derive(Debug)]
pub enum FollowedDependencies {
    #[literal("all")]
    All,

    #[literal("dev")]
    Dev,

    #[literal("prod")]
    Prod,
}

impl FollowedDependencies {
    /// The workspaces a workspace depends on (through `workspace:` ranges)
    /// for this kind of dependencies.
    pub fn of(&self, workspace: &Workspace) -> BTreeSet<Ident> {
        let mut dependencies
            = BTreeSet::new();

        if matches!(self, FollowedDependencies::All | FollowedDependencies::Prod) {
            for (ident, descriptor) in workspace.manifest.remote.dependencies.iter() {
                if descriptor.range.is_workspace() {
                    dependencies.insert(ident.clone());
                }
            }
        }

        if matches!(self, FollowedDependencies::All | FollowedDependencies::Dev) {
            for (ident, descriptor) in workspace.manifest.dev_dependencies.iter() {
                if descriptor.range.is_workspace() {
                    dependencies.insert(ident.clone());
                }
            }
        }

        dependencies
    }
}

/// Workspace selection options shared by `yarn workspaces foreach` and
/// `yarn tasks run`.
pub struct WorkspaceSelection<'a> {
    /// Select every workspace (`-A,--all`)
    pub all: bool,
    /// Select the workspaces matching these patterns (`--from`)
    pub from: &'a [WorkspaceGlob],
    /// Select the workspaces changed since a ref, or since the base refs when `Some(None)` (`--since`)
    pub since: &'a Option<Option<String>>,
    /// Compare `--since` against this ref rather than the working tree (`--head`)
    pub head: &'a Option<String>,
    /// Follow dependencies, or dependents when combined with `--since` (`--recursive`)
    pub recursive: bool,
    /// Add the transitive dependencies of the selected workspaces (`--follow-dependencies`)
    pub follow_dependencies: bool,
    /// Add the transitive dependents of the selected workspaces (`--follow-dependents`)
    pub follow_dependents: bool,
    /// Dependency kinds followed when traversing the graph (`--followed-dependencies`)
    pub followed_dependencies: &'a FollowedDependencies,
    /// Only keep workspaces matching these patterns (`--include`)
    pub include: &'a [WorkspaceGlob],
    /// Drop workspaces matching these patterns (`--exclude`)
    pub exclude: &'a [WorkspaceGlob],
    /// Keep private workspaces (`--private`, on by default)
    pub private: bool,
}

impl<'a> WorkspaceSelection<'a> {
    /// Whether the options select workspaces (as opposed to `--no-private`
    /// alone, which only filters).
    pub fn is_active(&self) -> bool {
        self.has_explicit_seeds()
            || self.follows_dependencies()
            || self.follows_dependents()
            || self.has_filters()
    }

    pub fn follows_dependencies(&self) -> bool {
        self.follow_dependencies || (self.recursive && self.since.is_none())
    }

    pub fn follows_dependents(&self) -> bool {
        self.follow_dependents || (self.recursive && self.since.is_some())
    }

    fn has_explicit_seeds(&self) -> bool {
        self.all || !self.from.is_empty() || self.since.is_some()
    }

    fn has_filters(&self) -> bool {
        !self.include.is_empty() || !self.exclude.is_empty()
    }

    pub async fn select(&self, project: &Project) -> Result<BTreeSet<Ident>, Error> {
        let mut seeds: BTreeSet<Ident>
            = BTreeSet::new();

        if self.all {
            seeds.extend(project.workspaces.iter().map(|w| w.name.clone()));
        }

        if !self.from.is_empty() {
            seeds.extend(project.workspaces.iter()
                .filter(|w| self.from.iter().any(|glob| glob.check(w)))
                .map(|w| w.name.clone()));
        }

        if let Some(since) = self.since {
            let range
                = git_utils::ChangesetRange {
                    base: since.clone(),
                    head: self.head.clone(),
                }.with_env_defaults();

            seeds.extend(git_utils::fetch_changed_workspaces_in_range(project, &range).await?);
        }

        if !self.has_explicit_seeds() {
            // The graph options alone apply to the active workspace; the
            // filters alone apply to the whole project.
            if self.follows_dependencies() || self.follows_dependents() {
                seeds.insert(project.active_workspace()?.name.clone());
            } else if self.has_filters() {
                seeds.extend(project.workspaces.iter().map(|w| w.name.clone()));
            } else {
                return Err(Error::MissingWorkspaceSelection);
            }
        }

        let mut selection
            = seeds.clone();

        if self.follows_dependencies() || self.follows_dependents() {
            let dependency_map: BTreeMap<Ident, BTreeSet<Ident>>
                = project.workspaces.iter()
                    .map(|w| (w.name.clone(), self.followed_dependencies.of(w)))
                    .collect();

            if self.follows_dependencies() {
                selection.extend(traverse(&seeds, &dependency_map));
            }

            if self.follows_dependents() {
                selection.extend(traverse(&seeds, &invert(&dependency_map)));
            }
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
        if !self.private && workspace.manifest.private == Some(true) {
            return false;
        }

        if !self.include.is_empty() && !self.include.iter().any(|glob| glob.check(workspace)) {
            return false;
        }

        !self.exclude.iter().any(|glob| glob.check(workspace))
    }
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

/// Every node reachable from the seeds (including the seeds themselves).
fn traverse(seeds: &BTreeSet<Ident>, edges: &BTreeMap<Ident, BTreeSet<Ident>>) -> BTreeSet<Ident> {
    let mut seen
        = seeds.clone();

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
