//! Resolution of the islands using the `cargo` linker.
//!
//! Unlike the other islands, the dependencies aren't read from the
//! `package.json` of the island's workspaces but from the `Cargo.toml`
//! sitting next to them, which Cargo needs anyway to build the crates. A
//! workspace of the island is either a crate or the root of a Cargo
//! workspace; the members of the latter don't have to be Yarn workspaces.

use std::{cell::RefCell, collections::{BTreeMap, BTreeSet}, sync::Arc};

use futures::StreamExt;
use zpm_primitives::{CargoGitRange, CargoGitReference, CargoGitSource, CargoPathRange, CargoPathReference, CargoRange, CargoRegistryReference, CargoRequirement, CargoVersion, Descriptor, Ident, Locator, Range, Reference};
use zpm_utils::{FromFileString, Hash64, Path, ToFileString};

use crate::{
    error::Error,
    install::InstallContext,
    island::{IslandResolutionResult, ResolvedIsland, island_result_from_lockfile},
    lockfile::Lockfile,
    project::Project,
    resolvers::Resolution,
};

use super::{
    CargoContext,
    CrateSource,
    DependencyKind,
    DependencySource,
    Summary,
    git,
    index::{self, CrateVersions},
    lockfile::{LOCKFILE_NAME, parse_lockfile},
    manifest::{Manifest, PathDependencies},
    solver::{CrateCatalog, GitRepository, LockedVersions, Solution, SolvedPackage, SolverError, solve},
};

/// How many index files are requested at once.
const INDEX_CONCURRENCY: usize
    = 48;

/// The crates of the island that live in the project.
#[derive(Clone, Debug, Default)]
pub struct LocalCrates {
    /// Folders of the crates whose features are all enabled, and whose
    /// dev-dependencies are part of the resolution
    pub members: BTreeSet<Path>,

    pub summaries: BTreeMap<Path, Arc<Summary>>,

    /// The content of every manifest the resolution depends on
    pub manifests: BTreeMap<Path, String>,
}

pub fn load_local_summary(dir: &Path, manifests: &mut BTreeMap<Path, String>) -> Result<Summary, Error> {
    let manifest
        = Manifest::read(dir)?;

    let workspace
        = manifest.find_workspace_root(None)?;

    manifests.insert(dir.clone(), manifest.text.clone());

    if let Some(workspace) = &workspace {
        manifests.insert(workspace.dir.clone(), workspace.text.clone());
    }

    manifest.summarize(workspace.as_ref(), CrateSource::Path(dir.clone()), &PathDependencies::Local)
}

/// Finds the crates the island is made of, starting from the folders of
/// its workspaces.
pub fn discover_local_crates(roots: &[Path]) -> Result<LocalCrates, Error> {
    let mut local
        = LocalCrates::default();

    let mut workspace_dirs
        = Vec::new();

    let mut queue
        = Vec::new();

    for root in roots {
        let manifest
            = Manifest::read(root)?;

        // Ignoring them would silently install something else than what
        // Cargo builds
        if let Some(table) = manifest.source_overrides().first() {
            return Err(Error::CargoError(format!(
                "{} has a [{}] table, which the cargo linker doesn't support yet",
                root.with_join_str(super::manifest::MANIFEST_NAME).to_file_string(),
                table,
            )));
        }

        local.manifests.insert(root.clone(), manifest.text.clone());

        if manifest.workspace().is_some() {
            workspace_dirs.push(root.clone());
        }

        for member in manifest.member_dirs()? {
            queue.push((member, true));
        }
    }

    while let Some((dir, is_member)) = queue.pop() {
        let is_new_member
            = is_member && local.members.insert(dir.clone());

        // A crate first met as a regular dependency is visited again
        // once it turns out to be a member, for its dev-dependencies
        let summary = match local.summaries.get(&dir) {
            Some(_) if !is_new_member => continue,
            Some(summary) => summary.clone(),
            None => Arc::new(load_local_summary(&dir, &mut local.manifests)?),
        };

        for dependency in &summary.dependencies {
            let DependencySource::Path(dependency_dir) = &dependency.source else {
                continue;
            };

            // Path dependencies located inside a workspace are members
            // of it, even when `workspace.members` doesn't list them
            let is_dependency_member
                = workspace_dirs.iter().any(|workspace_dir| workspace_dir.contains(dependency_dir));

            if is_dependency_member || is_member || dependency.kind != DependencyKind::Development {
                queue.push((dependency_dir.clone(), is_dependency_member));
            }
        }

        local.summaries.insert(dir, summary);
    }

    Ok(local)
}

/// Hash of everything the resolution of an island depends on, besides
/// the content of the registry.
fn input_hash(island_id: &str, context: &CargoContext, local: &LocalCrates) -> Hash64 {
    let mut parts
        = vec!["cargo-island-v1".to_string(), island_id.to_string(), context.index_url.clone()];

    for member in &local.members {
        parts.push(format!("member:{}", member.relative_to(&context.project_cwd).to_file_string()));
    }

    for (dir, text) in &local.manifests {
        parts.push(format!("manifest:{}:{}", dir.relative_to(&context.project_cwd).to_file_string(), Hash64::from_data(text.as_bytes()).to_file_string()));
    }

    Hash64::from_data(parts.join("\n").as_bytes())
}

/// The catalog used by actual installs: the registry index, the project's
/// folders, and git checkouts.
struct ProjectCatalog {
    context: CargoContext,
    handle: tokio::runtime::Handle,
    local: RefCell<BTreeMap<Path, Arc<Summary>>>,
    locked_commits: BTreeMap<CargoGitSource, String>,
    repositories: RefCell<BTreeMap<CargoGitSource, Arc<GitRepository>>>,
}

impl CrateCatalog for ProjectCatalog {
    fn registry(&self, name: &str) -> Result<Arc<CrateVersions>, SolverError> {
        self.handle.block_on(index::fetch_crate(&self.context, name, None))
            .map_err(|err| SolverError(err.to_string()))
    }

    fn prefetch(&self, names: &[String]) {
        // Failures are reported when the solver asks for the crate
        self.handle.block_on(
            futures::stream::iter(names)
                .map(|name| index::fetch_crate(&self.context, name, None))
                .buffer_unordered(INDEX_CONCURRENCY)
                .collect::<Vec<_>>(),
        );
    }

    fn path(&self, dir: &Path) -> Result<Arc<Summary>, SolverError> {
        if let Some(summary) = self.local.borrow().get(dir) {
            return Ok(summary.clone());
        }

        let summary
            = load_local_summary(dir, &mut BTreeMap::new())
                .map(Arc::new)
                .map_err(|err| SolverError(err.to_string()))?;

        self.local.borrow_mut().insert(dir.clone(), summary.clone());

        Ok(summary)
    }

    fn git(&self, source: &CargoGitSource) -> Result<Arc<GitRepository>, SolverError> {
        if let Some(repository) = self.repositories.borrow().get(source) {
            return Ok(repository.clone());
        }

        let repository = self.handle.block_on(async {
            // Branches move; the commit the island was locked on is kept
            let commit = match self.locked_commits.get(source) {
                Some(commit) => commit.clone(),
                None => git::resolve_commit(&self.context, source).await?,
            };

            let checkout_dir
                = git::ensure_checkout(&self.context, source, &commit).await?;

            let packages
                = git::read_packages(&checkout_dir, source, &commit)?;

            Ok::<_, Error>(GitRepository {commit, packages})
        }).map_err(|err| SolverError(format!("Failed to fetch {}: {}", source.url(), err)))?;

        let repository
            = Arc::new(repository);

        self.repositories.borrow_mut().insert(source.clone(), repository.clone());

        Ok(repository)
    }
}

fn relative_path(dir: &Path, project_cwd: &Path) -> String {
    let relative
        = dir.relative_to(project_cwd).to_file_string();

    match relative.is_empty() {
        true => ".".to_string(),
        false => relative,
    }
}

pub fn locator_of(summary: &Summary, project_cwd: &Path) -> Locator {
    let reference: Reference = match &summary.source {
        CrateSource::Registry => CargoRegistryReference {
            version: CargoVersion::from(&summary.version),
        }.into(),

        CrateSource::Git {source, commit} => CargoGitReference {
            source: source.clone(),
            commit: commit.clone(),
        }.into(),

        CrateSource::Path(dir) => CargoPathReference {
            path: relative_path(dir, project_cwd),
        }.into(),
    };

    Locator::new(Ident::new(&summary.name), reference)
}

/// The range a dependency is recorded with in the lockfile. Only what
/// identifies the edge is kept; features, targets and kinds are read back
/// from the manifests whenever they're needed.
fn range_of(target: &SolvedPackage, alias: Option<Ident>, requirements: &BTreeSet<&str>, project_cwd: &Path) -> Result<Range, Error> {
    Ok(match &target.summary.source {
        CrateSource::Registry => {
            let requirement
                = CargoRequirement::from_file_string(&requirements.iter().copied().collect::<Vec<_>>().join(","))
                    .map_err(|err| Error::CargoError(err.to_string()))?;

            CargoRange {ident: alias, requirement}.into()
        },

        CrateSource::Git {source, ..} => CargoGitRange {
            ident: alias,
            source: source.clone(),
        }.into(),

        CrateSource::Path(dir) => CargoPathRange {
            ident: alias,
            path: relative_path(dir, project_cwd),
        }.into(),
    })
}

/// Converts what the solver found into Yarn's own resolution structures.
pub fn convert_solution(island_id: &str, solution: &Solution, project_cwd: &Path) -> Result<IslandResolutionResult, Error> {
    let locators = solution.packages.iter()
        .map(|package| locator_of(&package.summary, project_cwd))
        .collect::<Vec<_>>();

    let mut descriptor_to_locator: BTreeMap<Descriptor, Locator>
        = BTreeMap::new();

    let mut normalized_resolutions
        = BTreeMap::new();

    for (index, package) in solution.packages.iter().enumerate() {
        let locator
            = &locators[index];

        let mut resolution
            = Resolution::new_empty(locator.clone(), CargoVersion::from(&package.summary.version).to_lossy_semver());

        // The entry points of the island
        if package.is_member {
            let descriptor
                = Descriptor::new(locator.ident.clone(), range_of(package, None, &BTreeSet::new(), project_cwd)?);

            descriptor_to_locator.insert(descriptor, locator.clone());
        }

        // A crate may list the same dependency several times (once per
        // table, once per target), usually ending on the same package
        let mut by_name: BTreeMap<&str, BTreeMap<usize, (DependencyKind, BTreeSet<&str>)>>
            = BTreeMap::new();

        for edge in &package.edges {
            let targets
                = by_name.entry(edge.dependency.name_in_toml.as_str()).or_default();

            let (kind, requirements)
                = targets.entry(edge.target).or_insert((edge.dependency.kind, BTreeSet::new()));

            *kind = (*kind).min(edge.dependency.kind);
            requirements.insert(edge.dependency.requirement.as_str());
        }

        for (name, targets) in by_name {
            let mut targets
                = targets.into_iter().collect::<Vec<_>>();

            targets.sort_by_key(|(target, (kind, _))| (*kind, *target));

            for (position, (target, (kind, requirements))) in targets.into_iter().enumerate() {
                let target_package
                    = &solution.packages[target];

                // The rare dependencies resolving to different packages
                // depending on the table they're listed in (a build script
                // using another major than the library, say) get a key of
                // their own
                let mut key = match position {
                    0 => Ident::new(name),
                    _ => Ident::new(format!("@{}/{}", kind.as_str(), name)),
                };

                let mut suffix
                    = 1;

                while resolution.dependencies.contains_key(&key) {
                    suffix += 1;
                    key = Ident::new(format!("@{}{}/{}", kind.as_str(), suffix, name));
                }

                let alias
                    = (key.as_str() != target_package.summary.name).then(|| Ident::new(&target_package.summary.name));

                let mut descriptor
                    = Descriptor::new(key.clone(), range_of(target_package, alias.clone(), &requirements, project_cwd)?);

                // A requirement spanning several buckets can resolve to
                // different versions for different dependents; the lockfile
                // maps each descriptor to a single package, so those are
                // recorded with the version they got
                if descriptor_to_locator.get(&descriptor).is_some_and(|existing| existing != &locators[target]) {
                    let pinned
                        = format!("={}", target_package.summary.version);

                    descriptor = Descriptor::new(key.clone(), range_of(target_package, alias, &BTreeSet::from([pinned.as_str()]), project_cwd)?);
                }

                descriptor_to_locator.insert(descriptor.clone(), locators[target].clone());
                resolution.dependencies.insert(key, descriptor);
            }
        }

        normalized_resolutions.insert(locator.clone(), resolution);
    }

    Ok(IslandResolutionResult {
        island_id: island_id.to_string(),
        input_hash: None,
        descriptor_to_locator,
        normalized_resolutions,
    })
}

/// The versions the resolution should stick to: the ones the island was
/// locked on, or, the first time, the ones of the `Cargo.lock` files found
/// next to the island's workspaces.
fn locked_packages(island: &ResolvedIsland, lockfile: &Lockfile, roots: &[Path]) -> (LockedVersions, BTreeMap<CargoGitSource, String>) {
    let mut versions
        = LockedVersions::default();

    let mut commits
        = BTreeMap::new();

    if let Some(locked_island) = lockfile.islands.get(&island.id) {
        for locator in locked_island.values() {
            match &locator.reference {
                Reference::CargoRegistry(params) => {
                    versions.registry.entry(locator.ident.as_str().to_string()).or_default().insert(params.version.to_semver());
                },

                Reference::CargoGit(params) => {
                    commits.insert(params.source.clone(), params.commit.clone());
                },

                _ => {},
            }
        }

        return (versions, commits);
    }

    for root in roots {
        let Some(locked) = root.with_join_str(LOCKFILE_NAME).fs_read_text().ok().and_then(|text| parse_lockfile(&text)) else {
            continue;
        };

        for (name, locked_versions) in locked.registry {
            versions.registry.entry(name).or_default().extend(locked_versions);
        }

        commits.extend(locked.git);
    }

    (versions, commits)
}

pub fn island_roots(project: &Project, island_id: &str, workspace_idents: &BTreeSet<Ident>) -> Result<Vec<Path>, Error> {
    workspace_idents.iter().map(|ident| {
        let workspace
            = project.workspace_by_ident(ident)?;

        if !Manifest::exists(&workspace.path) {
            return Err(Error::CargoError(format!(
                "The workspace {} belongs to the island '{}', which uses the cargo linker, but it doesn't have a Cargo.toml",
                ident.as_str(),
                island_id,
            )));
        }

        Ok(workspace.path.clone())
    }).collect()
}

/// A digest of the manifests an island is resolved from. The linker stores
/// it next to what it generates, so that Yarn can tell whether an install
/// would change anything (Cargo manifests aren't tracked like the
/// `package.json` files are).
pub fn island_state(project: &Project, island_id: &str, workspace_idents: &BTreeSet<Ident>) -> Result<String, Error> {
    let roots
        = island_roots(project, island_id, workspace_idents)?;

    let local
        = discover_local_crates(&roots)?;

    Ok(input_hash(island_id, &CargoContext::new(project), &local).to_file_string())
}

pub async fn resolve_cargo_island(island: &ResolvedIsland, context: &InstallContext<'_>, lockfile: &Lockfile) -> Result<IslandResolutionResult, Error> {
    let project
        = context.project
            .expect("The project is required for resolving cargo islands");

    let cargo_context
        = CargoContext::new(project);

    let roots
        = island_roots(project, &island.id, &island.workspace_idents)?;

    let local
        = discover_local_crates(&roots)?;

    let input_hash
        = input_hash(&island.id, &cargo_context, &local);

    // As long as the manifests don't change the island is taken from the
    // lockfile, without looking at the registry
    if !context.refresh_lockfile && lockfile.island_hashes.get(&island.id) == Some(&input_hash) {
        if let Some(locked_island) = lockfile.islands.get(&island.id) {
            if locked_island.values().all(|locator| lockfile.entries.contains_key(locator)) {
                let mut result
                    = island_result_from_lockfile(&island.id, locked_island, lockfile)?;

                result.input_hash = Some(input_hash);

                return Ok(result);
            }
        }
    }

    let (locked_versions, locked_commits)
        = locked_packages(island, lockfile, &roots);

    let catalog = ProjectCatalog {
        context: cargo_context.clone(),
        handle: tokio::runtime::Handle::current(),
        local: RefCell::new(local.summaries.clone()),
        locked_commits,
        repositories: RefCell::new(BTreeMap::new()),
    };

    let members
        = local.members.clone();

    // The solver is synchronous, and blocks on the runtime whenever it
    // needs something from the network
    let solution = tokio::task::spawn_blocking(move || solve(&catalog, &members, &locked_versions))
        .await
        .map_err(|err| Error::IslandResolutionFailed {
            island_id: island.id.clone(),
            message: format!("Join error: {}", err),
        })?
        .map_err(|err| Error::IslandResolutionFailed {
            island_id: island.id.clone(),
            message: err.0,
        })?;

    let mut result
        = convert_solution(&island.id, &solution, &cargo_context.project_cwd)?;

    result.input_hash = Some(input_hash);

    Ok(result)
}
