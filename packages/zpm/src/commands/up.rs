use std::collections::{BTreeMap, BTreeSet};

use clipanion::cli;
use zpm_primitives::{Descriptor, Ident, IdentGlob};
use zpm_semver::RangeKind;

use crate::{
    descriptor_loose::{self, LooseDescriptor, LooseResolution},
    error::Error,
    install::InstallContext,
    manifest::dependencies::{ManifestScope, ManifestSet, upgrade_descriptor},
    project::{InstallMode, Project, RunInstallOptions, Workspace}
};

/// Update dependencies to the latest versions
///
/// This command upgrades packages matching the specified descriptors across the whole project. It updates `dependencies` and `devDependencies`;
/// `peerDependencies` are left unchanged.
///
/// If `-R,--recursive` is set, Yarn refreshes all lockfile resolutions matching the selected packages without editing manifests. Depending on the
/// goal, you may want to run both `yarn up` and `yarn up -R`.
///
/// The `-C,--caret`, `-E,--exact`, and `-T,--tilde` options have the same meaning as in `yarn add`: they choose the range modifier used when the
/// requested descriptor has no explicit range or uses a tag.
///
/// If the `--mode=<mode>` option is set, Yarn will change which artifacts are generated. The modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting `enableScripts` to false because the latter will
///   disable build scripts, and thus affect the content of the artifacts generated on disk, whereas the former will just disable the build step but
///   not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are missing from the lockfile (or that have no associated
///   checksums). This mode is typically used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring the full install
///   cost.
///
/// You can think of `yarn up` as the Yarn 6 counterpart to `yarn upgrade --latest` in Yarn 1: it ignores the ranges previously listed in your
/// manifests. Unlike `yarn upgrade`, which only upgraded dependencies in the current workspace, `yarn up` upgrades all workspaces at the same time.
///
/// This command accepts glob patterns as arguments (if valid Descriptors and supported by micromatch). Make sure to escape the patterns, to prevent
/// your own shell from trying to expand them.
///
/// **Note:** The ranges have to be static, only the package scopes and names can contain glob patterns.
///
/// ## Examples
///
/// Upgrade all instances of lodash to the latest release
///
/// `yarn up lodash`
///
/// Upgrade all instances of lodash to 4.17.21
///
/// `yarn up lodash@4.17.21`
///
/// Upgrade all instances of packages with the `@babel` scope to the latest release
///
/// `yarn up '@babel/*'`
///
/// Refresh the lockfile resolutions of lodash without changing the manifests
///
/// `yarn up -R lodash`
///
#[cli::command]
#[cli::path("up")]
#[cli::category("Dependency management")]
pub struct Up {
    /// Store dependency tags as-is instead of resolving them to versions
    #[cli::option("-F,--fixed", default = false)]
    fixed: bool,

    /// Use an exact semver range for resolved versions
    #[cli::option("-E,--exact", default = false)]
    exact: bool,

    /// Use the `~` semver modifier on the resolved range
    #[cli::option("-T,--tilde", default = false)]
    tilde: bool,

    /// Use the `^` semver modifier on the resolved range
    #[cli::option("-C,--caret", default = false)]
    caret: bool,

    /// Refresh matching lockfile resolutions without editing manifests
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    // ---

    /// Disable the minimum release age check for this command
    #[cli::option("--no-time-gate", default = false)]
    no_time_gate: bool,

    /// Select which install artifacts Yarn should generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    // ---

    /// Package descriptors or patterns to update
    descriptors: Vec<LooseDescriptor>,
}

impl Up {
    pub async fn execute(&self) -> Result<(), Error> {
        if self.recursive {
            return self.execute_recursive().await;
        }

        let mut project
            = Project::new(None).await?;

        if self.no_time_gate {
            project.config.settings.disable_age_gate();
        }

        let mut manifest_sets = project.workspaces.iter()
            .map(|workspace| ManifestSet::open(&project, workspace, ManifestScope::All))
            .collect::<Result<Vec<_>, Error>>()?;

        let package_idents = project.workspaces.iter()
            .flat_map(|workspace| self.list_workspace_idents(workspace))
            .collect::<BTreeSet<_>>();

        // The dependencies declared outside of the package.json files (in
        // Cargo.toml files, say) are upgraded within their own ecosystem
        let mut foreign_descriptors: BTreeMap<Ident, BTreeSet<Descriptor>>
            = BTreeMap::new();

        for dependency in manifest_sets.iter().flat_map(|set| set.companions()).flat_map(|manifest| manifest.dependencies()) {
            foreign_descriptors.entry(dependency.descriptor.ident.clone()).or_default().insert(dependency.descriptor);
        }

        let all_idents = package_idents.iter()
            .chain(foreign_descriptors.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        let expanded_descriptors = self.descriptors.iter()
            .flat_map(|descriptor| descriptor.expand(&all_idents))
            .collect::<Vec<_>>();

        let mut package_requests
            = Vec::new();
        let mut foreign_requests
            = BTreeSet::new();

        for descriptor in expanded_descriptors {
            match &descriptor {
                LooseDescriptor::Ident(params) if foreign_descriptors.contains_key(&params.ident) => {
                    foreign_requests.extend(foreign_descriptors[&params.ident].iter().cloned());

                    // Names aren't unique across ecosystems
                    if package_idents.contains(&params.ident) {
                        package_requests.push(descriptor);
                    }
                },

                _ => {
                    package_requests.push(descriptor);
                },
            }
        }

        let range_kind = if self.fixed {
            RangeKind::Exact
        } else if self.exact {
            RangeKind::Exact
        } else if self.tilde {
            RangeKind::Tilde
        } else if self.caret {
            RangeKind::Caret
        } else {
            project.config.settings.default_semver_range_prefix.value
        };

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind,
            resolve_tags: !self.fixed,
            allow_reuse: false,
        };

        let package_cache
            = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let mut loose_resolutions
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &package_requests).await?;

        for descriptor in &foreign_requests {
            if let Some(descriptor) = upgrade_descriptor(&install_context, range_kind, descriptor).await? {
                loose_resolutions.push(LooseResolution {descriptor, locator: None});
            }
        }

        for manifest_set in &mut manifest_sets {
            for resolution in loose_resolutions.iter() {
                for manifest in manifest_set.iter_mut() {
                    if manifest.accepts(&resolution.descriptor.range) {
                        manifest.update(&resolution.descriptor)?;
                    }
                }
            }

            manifest_set.save()?;
        }

        let mut project
            = Project::new(None).await?;

        if self.no_time_gate {
            project.config.settings.disable_age_gate();
        }

        let enforced_resolutions
            = loose_resolutions.into_iter()
                .filter_map(|resolution| resolution.locator.map(|locator| (resolution.descriptor, Some(locator))))
                .collect();

        project.run_install(RunInstallOptions {
            mode: self.mode,
            enforced_resolutions,
            ..Default::default()
        }).await?;

        Ok(())
    }

    async fn execute_recursive(&self) -> Result<(), Error> {
        use zpm_utils::ToFileString;

        let patterns = self.descriptors.iter()
            .map(|descriptor| IdentGlob::new(&descriptor.to_file_string()))
            .collect::<Result<Vec<_>, _>>()?;

        let mut project = Project::new(None).await?;

        if self.no_time_gate {
            project.config.settings.disable_age_gate();
        }

        let lockfile = project.lockfile()?;

        // Crates are locked by their islands rather than by the project
        let island_descriptors = lockfile.islands.values()
            .flat_map(|island| island.keys())
            .filter(|descriptor| crate::cargo::editor::is_cargo_range(&descriptor.range));

        let enforced_resolutions = lockfile.resolutions.keys()
            .chain(island_descriptors)
            .filter(|descriptor| {
                patterns.iter().any(|pattern| pattern.check(&descriptor.ident))
            })
            .map(|descriptor| (descriptor.clone(), None))
            .collect();

        project.run_install(RunInstallOptions {
            enforced_resolutions,
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }

    fn list_workspace_idents(&self, workspace: &Workspace) -> Vec<Ident> {
        let mut idents = Vec::new();

        for dependency in workspace.manifest.remote.dependencies.values() {
            idents.push(dependency.ident.clone());
        }

        for dependency in workspace.manifest.remote.optional_dependencies.values() {
            idents.push(dependency.ident.clone());
        }

        for dependency in workspace.manifest.dev_dependencies.values() {
            idents.push(dependency.ident.clone());
        }

        idents
    }
}
