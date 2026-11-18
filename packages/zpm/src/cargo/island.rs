//! Resolution of the islands using the `cargo` linker.
//!
//! Unlike the other islands, the dependencies aren't read from the
//! `package.json` of the island's workspaces but from the `Cargo.toml`
//! sitting next to them, which Cargo needs anyway to build the crates. A
//! workspace of the island is either a crate or the root of a Cargo
//! workspace; the members of the latter don't have to be Yarn workspaces.

use std::{cell::RefCell, collections::{BTreeMap, BTreeSet}, sync::Arc};

use futures::StreamExt;
use zpm_primitives::{CargoGitRange, CargoGitReference, CargoGitSource, CargoPathRange, CargoPathReference, CargoRange, CargoRegistryReference, CargoRequirement, CargoSemver, CargoVersion, Descriptor, Ident, Locator, Range, Reference};
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
    Dependency,
    DependencyKind,
    DependencySource,
    PatchTarget,
    Registries,
    RegistryIndex,
    RustVersion,
    Summary,
    git,
    index::{self, CrateVersions},
    lockfile::{LOCKFILE_NAME, parse_lockfile},
    manifest::{Manifest, ManifestContext, Replacement},
    solver::{CrateCatalog, GitRepository, LockedVersions, Solution, SolveOptions, SolverError, solve},
    toolchain,
};

/// How many index files are requested at once.
const INDEX_CONCURRENCY: usize
    = 48;

/// The crates of the island that live in the project, and what the
/// roots of its Cargo workspaces say about the resolution.
#[derive(Clone, Debug, Default)]
pub struct LocalCrates {
    /// Folders of the crates whose features are all enabled, and whose
    /// dev-dependencies are part of the resolution
    pub members: BTreeSet<Path>,

    pub summaries: BTreeMap<Path, Arc<Summary>>,

    /// The content of every manifest the resolution depends on
    pub manifests: BTreeMap<Path, String>,

    pub patches: Vec<(PatchTarget, Dependency)>,
    pub replacements: Vec<Replacement>,

    /// See `SolveOptions::rust_versions`
    pub rust_versions: Vec<RustVersion>,
}

pub fn load_local_summary(dir: &Path, registries: &Registries, manifests: &mut BTreeMap<Path, String>) -> Result<Summary, Error> {
    let manifest
        = Manifest::read(dir)?;

    let workspace
        = manifest.find_workspace_root(None)?;

    manifests.insert(dir.clone(), manifest.text.clone());

    if let Some(workspace) = &workspace {
        manifests.insert(workspace.dir.clone(), workspace.text.clone());
    }

    manifest.summarize(workspace.as_ref(), CrateSource::Path(dir.clone()), &ManifestContext::local(registries))
}

/// What `resolver.incompatible-rust-versions` is set to in the Cargo
/// configuration of a workspace, if anything.
fn configured_rust_version_preference(root: &Path) -> Option<bool> {
    let config
        = root.with_join_str(".cargo/config.toml").fs_read_text().ok()?.parse::<toml_edit::DocumentMut>().ok()?;

    match config.get("resolver")?.as_table_like()?.get("incompatible-rust-versions")?.as_str()? {
        "fallback" => Some(true),
        "allow" => Some(false),
        _ => None,
    }
}

/// Finds the crates the island is made of, starting from the folders of
/// its workspaces.
pub fn discover_local_crates(roots: &[Path], registries: &Registries) -> Result<LocalCrates, Error> {
    let mut local
        = LocalCrates::default();

    let mut workspace_dirs
        = Vec::new();

    let mut queue
        = Vec::new();

    let mut prefers_compatible_rust_versions
        = false;

    let context
        = ManifestContext::local(registries);

    for root in roots {
        let manifest
            = Manifest::read(root)?;

        local.manifests.insert(root.clone(), manifest.text.clone());

        if manifest.workspace().is_some() {
            workspace_dirs.push(root.clone());
        }

        for member in manifest.member_dirs()? {
            queue.push((member, true));
        }

        local.patches.extend(manifest.patches(&context)?);
        local.replacements.extend(manifest.replacements(&context)?);

        prefers_compatible_rust_versions |= configured_rust_version_preference(root)
            .unwrap_or_else(|| manifest.prefers_compatible_rust_versions());
    }

    // The forks patches and replacements point to are part of what the
    // resolution is computed from
    let overrides = local.patches.iter()
        .map(|(_, dependency)| dependency)
        .chain(local.replacements.iter().map(|replacement| &replacement.dependency));

    for dependency in overrides {
        if let DependencySource::Path(dir) = &dependency.source {
            queue.push((dir.clone(), false));
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
            None => Arc::new(load_local_summary(&dir, registries, &mut local.manifests)?),
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

    if prefers_compatible_rust_versions {
        let mut rust_versions = local.members.iter()
            .filter_map(|member| local.summaries.get(member)?.rust_version)
            .collect::<BTreeSet<_>>();

        // Without any `rust-version` Cargo goes by the toolchain's; we
        // can tell when the project pins it
        if rust_versions.is_empty() {
            rust_versions.extend(roots.iter().find_map(toolchain::pinned_rust_version));
        }

        local.rust_versions = rust_versions.into_iter().collect();
    }

    Ok(local)
}

/// Hash of everything the resolution of an island depends on, besides
/// the content of the registries.
fn input_hash(island_id: &str, context: &CargoContext, local: &LocalCrates) -> Hash64 {
    let mut parts
        = vec!["cargo-island-v2".to_string(), island_id.to_string(), context.registries.default_index.clone()];

    for (name, index) in &context.registries.names {
        parts.push(format!("registry:{}:{}", name, index));
    }

    for member in &local.members {
        parts.push(format!("member:{}", member.relative_to(&context.project_cwd).to_file_string()));
    }

    for (dir, text) in &local.manifests {
        parts.push(format!("manifest:{}:{}", dir.relative_to(&context.project_cwd).to_file_string(), Hash64::from_data(text.as_bytes()).to_file_string()));
    }

    for rust_version in &local.rust_versions {
        parts.push(format!("rust:{}.{}.{}", rust_version.0, rust_version.1, rust_version.2));
    }

    Hash64::from_data(parts.join("\n").as_bytes())
}

/// The catalog used by actual installs: the registry indexes, the
/// project's folders, and git checkouts.
struct ProjectCatalog {
    context: CargoContext,
    handle: tokio::runtime::Handle,
    local: RefCell<BTreeMap<Path, Arc<Summary>>>,
    locked_commits: BTreeMap<CargoGitSource, String>,
    repositories: RefCell<BTreeMap<CargoGitSource, Arc<GitRepository>>>,
}

impl CrateCatalog for ProjectCatalog {
    fn registry(&self, registry: &RegistryIndex, name: &str) -> Result<Arc<CrateVersions>, SolverError> {
        self.handle.block_on(index::fetch_crate(&self.context, registry, name, None))
            .map_err(|err| SolverError(err.to_string()))
    }

    fn prefetch(&self, crates: &[(RegistryIndex, String)]) {
        // Failures are reported when the solver asks for the crate
        self.handle.block_on(
            futures::stream::iter(crates)
                .map(|(registry, name)| index::fetch_crate(&self.context, registry, name, None))
                .buffer_unordered(INDEX_CONCURRENCY)
                .collect::<Vec<_>>(),
        );
    }

    fn path(&self, dir: &Path) -> Result<Arc<Summary>, SolverError> {
        if let Some(summary) = self.local.borrow().get(dir) {
            return Ok(summary.clone());
        }

        let summary
            = load_local_summary(dir, &self.context.registries, &mut BTreeMap::new())
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
                = git::read_packages(&checkout_dir, source, &commit, &self.context.registries)?;

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
        CrateSource::Registry(registry) => CargoRegistryReference {
            version: CargoVersion::from(&summary.version),
            registry: registry.clone(),
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
fn range_of(target: &Summary, alias: Option<Ident>, requirements: &BTreeSet<&str>, project_cwd: &Path) -> Result<Range, Error> {
    Ok(match &target.source {
        CrateSource::Registry(registry) => {
            let requirement
                = CargoRequirement::from_file_string(&requirements.iter().copied().collect::<Vec<_>>().join(","))
                    .map_err(|err| Error::CargoError(err.to_string()))?;

            CargoRange {
                ident: alias,
                requirement,
                registry: registry.clone(),
            }.into()
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

    let no_requirement
        = BTreeSet::new();

    for (index, package) in solution.packages.iter().enumerate() {
        let locator
            = &locators[index];

        let mut resolution
            = Resolution::new_empty(locator.clone(), CargoVersion::from(&package.summary.version).to_lossy_semver());

        // The entry points of the island
        if package.is_member {
            let descriptor
                = Descriptor::new(locator.ident.clone(), range_of(&package.summary, None, &no_requirement, project_cwd)?);

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
                let target_summary
                    = &solution.packages[target].summary;

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
                    = (key.as_str() != target_summary.name).then(|| Ident::new(&target_summary.name));

                let mut descriptor
                    = Descriptor::new(key.clone(), range_of(target_summary, alias.clone(), &requirements, project_cwd)?);

                // A requirement spanning several buckets can resolve to
                // different versions for different dependents; the lockfile
                // maps each descriptor to a single package, so those are
                // recorded with the version they got
                if descriptor_to_locator.get(&descriptor).is_some_and(|existing| existing != &locators[target]) {
                    let pinned
                        = format!("={}", target_summary.version);

                    descriptor = Descriptor::new(key.clone(), range_of(target_summary, alias, &BTreeSet::from([pinned.as_str()]), project_cwd)?);
                }

                descriptor_to_locator.insert(descriptor.clone(), locators[target].clone());
                resolution.dependencies.insert(key, descriptor);
            }
        }

        // A replaced crate is recorded as depending on its replacement,
        // which is the one holding the actual dependencies
        if let Some(replacement) = &package.summary.replaced_by {
            let replacement_locator
                = locator_of(replacement, project_cwd);

            let replacement_descriptor
                = Descriptor::new(locator.ident.clone(), range_of(replacement, None, &no_requirement, project_cwd)?);

            let mut replacement_resolution
                = Resolution::new_empty(replacement_locator.clone(), resolution.version.clone());

            replacement_resolution.dependencies
                = std::mem::take(&mut resolution.dependencies);

            resolution.dependencies.insert(locator.ident.clone(), replacement_descriptor.clone());

            descriptor_to_locator.insert(replacement_descriptor, replacement_locator.clone());
            normalized_resolutions.insert(replacement_locator, replacement_resolution);
        }

        normalized_resolutions.insert(locator.clone(), resolution);
    }

    // Cargo's lockfile lists the patches that ended up unused; they're
    // packages nothing depends on
    for patch in &solution.unused_patches {
        let locator
            = locator_of(patch, project_cwd);

        let descriptor
            = Descriptor::new(locator.ident.clone(), range_of(patch, None, &no_requirement, project_cwd)?);

        descriptor_to_locator.entry(descriptor).or_insert_with(|| locator.clone());

        normalized_resolutions.entry(locator.clone())
            .or_insert_with(|| Resolution::new_empty(locator, CargoVersion::from(&patch.version).to_lossy_semver()));
    }

    Ok(IslandResolutionResult {
        island_id: island_id.to_string(),
        input_hash: None,
        descriptor_to_locator,
        normalized_resolutions,
    })
}

/// The name of the crate a `cargo:` descriptor designates.
fn crate_name(descriptor: &Descriptor) -> Option<&str> {
    let alias = match &descriptor.range {
        Range::Cargo(params) => &params.ident,
        Range::CargoGit(params) => &params.ident,
        Range::CargoPath(params) => &params.ident,
        _ => return None,
    };

    Some(alias.as_ref().unwrap_or(&descriptor.ident).as_str())
}

/// The versions the resolution should stick to: the ones the island was
/// locked on, or, the first time, the ones of the `Cargo.lock` files found
/// next to the island's workspaces. The crates listed in `unlocked` are
/// free to move (`yarn up -R`).
fn locked_packages(island: &ResolvedIsland, lockfile: &Lockfile, roots: &[Path], unlocked: &BTreeSet<String>) -> (LockedVersions, BTreeMap<CargoGitSource, String>) {
    let mut versions
        = LockedVersions::default();

    let mut commits
        = BTreeMap::new();

    if let Some(locked_island) = lockfile.islands.get(&island.id) {
        for locator in locked_island.values() {
            let name
                = locator.ident.as_str();

            if unlocked.contains(name) {
                continue;
            }

            let version = match &locator.reference {
                Reference::CargoRegistry(params) => {
                    Some(params.version.to_semver())
                },

                Reference::CargoGit(params) => {
                    commits.insert(params.source.clone(), params.commit.clone());
                    lockfile.entries.get(locator).and_then(|entry| CargoSemver::parse(&entry.resolution.version.to_file_string()))
                },

                // Patches living on the disk are candidates like the
                // others; the one in use must stay preferred
                Reference::CargoPath(_) => {
                    lockfile.entries.get(locator).and_then(|entry| CargoSemver::parse(&entry.resolution.version.to_file_string()))
                },

                _ => None,
            };

            if let Some(version) = version {
                versions.registry.entry(name.to_string()).or_default().insert(version);
            }
        }

        return (versions, commits);
    }

    for root in roots {
        let Some(locked) = root.with_join_str(LOCKFILE_NAME).fs_read_text().ok().and_then(|text| parse_lockfile(&text)) else {
            continue;
        };

        for (name, locked_versions) in locked.versions {
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
    let context
        = CargoContext::new(project);

    let roots
        = island_roots(project, island_id, workspace_idents)?;

    let local
        = discover_local_crates(&roots, &context.registries)?;

    Ok(input_hash(island_id, &context, &local).to_file_string())
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
        = discover_local_crates(&roots, &cargo_context.registries)?;

    let input_hash
        = input_hash(&island.id, &cargo_context, &local);

    // What `yarn up -R` asks to resolve again
    let unlocked = context.enforced_resolutions.iter()
        .filter(|(_, locator)| locator.is_none())
        .filter_map(|(descriptor, _)| crate_name(descriptor))
        .map(|name| name.to_string())
        .collect::<BTreeSet<_>>();

    // As long as the manifests don't change the island is taken from the
    // lockfile, without looking at the registry
    if !context.refresh_lockfile && unlocked.is_empty() && lockfile.island_hashes.get(&island.id) == Some(&input_hash) {
        if let Some(locked_island) = lockfile.islands.get(&island.id) {
            if locked_island.values().all(|locator| lockfile.entries.contains_key(locator)) {
                let mut result
                    = island_result_from_lockfile(&island.id, locked_island, lockfile)?;

                result.input_hash = Some(input_hash);

                return Ok(result);
            }
        }
    }

    let (locked, locked_commits)
        = locked_packages(island, lockfile, &roots, &unlocked);

    let catalog = ProjectCatalog {
        context: cargo_context.clone(),
        handle: tokio::runtime::Handle::current(),
        local: RefCell::new(local.summaries.clone()),
        locked_commits,
        repositories: RefCell::new(BTreeMap::new()),
    };

    let options = SolveOptions {
        members: local.members,
        locked,
        rust_versions: local.rust_versions,
        patches: local.patches,
        replacements: local.replacements,
    };

    // The solver is synchronous, and blocks on the runtime whenever it
    // needs something from the network
    let solution = tokio::task::spawn_blocking(move || solve(&catalog, &options))
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
