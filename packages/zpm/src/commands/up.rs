use std::collections::BTreeSet;

use clipanion::cli;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_primitives::Ident;
use zpm_semver::RangeKind;
use zpm_utils::ToFileString;

use crate::{
    descriptor_loose::{self, LooseDescriptor},
    error::Error,
    install::InstallContext,
    project::{InstallMode, Project, RunInstallOptions, Workspace}
};

/// Update dependencies to the latest versions
///
/// This command upgrades the packages matching the list of specified patterns to their latest available version across the whole project (regardless
/// of whether they're part of `dependencies` or `devDependencies` - `peerDependencies` won't be affected). This is a project-wide command: all
/// workspaces will be upgraded in the process.
///
/// If `-R,--recursive` is set the command will change behavior and no other switch will be allowed. When operating under this mode yarn up will
/// collect package idents from both workspace manifests and the lockfile, expand patterns against all of them, then re-resolve matching packages
/// (often to their highest available versions). Both the lockfile and workspace manifests are updated with the new resolutions.
///
/// If `-i,--interactive` is set (or if the `preferInteractive` settings is toggled on) the command will offer various choices, depending on the
/// detected upgrade paths. Some upgrades require this flag in order to resolve ambiguities.
///
/// The, -C,--caret, -E,--exact and -T,--tilde options have the same meaning as in the add command (they change the modifier used when the range is
/// missing or a tag, and are ignored when the range is explicitly set).
///
/// If the --mode=<mode> option is set, Yarn will change which artifacts are generated. The modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting `enableScripts` to false because the latter will
///   disable build scripts, and thus affect the content of the artifacts generated on disk, whereas the former will just disable the build step but
///   not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are missing from the lockfile (or that have no associated
///   checksums). This mode is typically used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring the full install
///   cost.
///
/// Generally you can see `yarn up` as a counterpart to what was `yarn upgrade --latest` in Yarn 1 (ie it ignores the ranges previously listed in
/// your project's manifests), but unlike `yarn upgrade` which only upgraded dependencies in the current workspace, `yarn up` will upgrade all
/// workspaces at the same time.
///
/// This command accepts glob patterns as arguments (if valid Descriptors and supported by micromatch). Make sure to escape the patterns, to prevent
/// your own shell from trying to expand them.
///
/// **Note:** The ranges have to be static, only the package scopes and names can contain glob patterns.
///
#[cli::command]
#[cli::path("up")]
#[cli::category("Dependency management")]
pub struct Up {
    /// Store dependency tags as-is instead of resolving them
    #[cli::option("-F,--fixed", default = false)]
    fixed: bool,

    /// Don't use any semver modifier on the resolved range
    #[cli::option("-E,--exact", default = false)]
    exact: bool,

    /// Use the `~` semver modifier on the resolved range
    #[cli::option("-T,--tilde", default = false)]
    tilde: bool,

    /// Use the `^` semver modifier on the resolved range
    #[cli::option("-C,--caret", default = false)]
    caret: bool,

    /// Resolve again ALL resolutions for those packages
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    // ---

    /// Change what artifacts this install will generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    // ---

    /// The packages to update
    descriptors: Vec<LooseDescriptor>,
}

impl Up {
    pub async fn execute(&self) -> Result<(), Error> {
        if self.recursive && (self.exact || self.tilde || self.caret || self.fixed) {
            return Err(Error::ConflictingOptions(
                "The --recursive flag cannot be used together with --exact, --tilde, --caret, or --fixed".into()
            ));
        }

        if self.recursive {
            self.up_recursive().await
        } else {
            self.up_classic().await
        }
    }

    async fn up_classic(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let all_idents = project.workspaces.iter()
            .flat_map(|workspace| self.list_workspace_idents(workspace))
            .collect::<BTreeSet<_>>();

        let expanded_descriptors = self.descriptors.iter()
            .flat_map(|descriptor| descriptor.expand(&all_idents))
            .collect::<Vec<_>>();

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

        let loose_resolutions
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &expanded_descriptors).await?;

        for workspace in &project.workspaces {
            self.update_workspace_manifest(workspace, &loose_resolutions)?;
        }

        let mut project
            = Project::new(None).await?;

        let enforced_resolutions
            = loose_resolutions.into_iter()
                .filter_map(|resolution| resolution.locator.map(|locator| (resolution.descriptor, locator)))
                .collect();

        project.run_install(RunInstallOptions {
            mode: self.mode,
            enforced_resolutions,
            ..Default::default()
        }).await?;

        Ok(())
    }

    async fn up_recursive(&self) -> Result<(), Error> {
        let project = Project::new(None).await?;

        // Collect idents from both workspaces and lockfile for pattern matching
        let workspace_idents: BTreeSet<Ident> = project.workspaces.iter()
            .flat_map(|workspace| self.list_workspace_idents(workspace))
            .collect();

        let lockfile = project.lockfile()?;
        let lockfile_idents: BTreeSet<Ident> = lockfile.resolutions.keys()
            .map(|d| d.ident.clone())
            .collect();

        let all_idents: BTreeSet<Ident> = workspace_idents.union(&lockfile_idents).cloned().collect();

        // Expand patterns against all idents (workspaces + lockfile)
        let expanded_descriptors = self.descriptors.iter()
            .flat_map(|descriptor| descriptor.expand(&all_idents))
            .collect::<Vec<_>>();

        let range_kind = project.config.settings.default_semver_range_prefix.value;

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind,
            resolve_tags: true,
        };

        let package_cache = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let loose_resolutions = LooseDescriptor::resolve_all(&install_context, &resolve_options, &expanded_descriptors).await?;

        for workspace in &project.workspaces {
            self.update_workspace_manifest(workspace, &loose_resolutions)?;
        }

        let mut project = Project::new(None).await?;

        let enforced_resolutions = loose_resolutions.into_iter()
            .filter_map(|resolution| resolution.locator.map(|locator| (resolution.descriptor, locator)))
            .collect();

        project.run_install(RunInstallOptions {
            mode: self.mode,
            enforced_resolutions,
            ..Default::default()
        }).await?;

        Ok(())
    }

    fn update_workspace_manifest(&self, workspace: &Workspace, loose_resolutions: &[descriptor_loose::LooseResolution]) -> Result<(), Error> {
        let manifest_path = workspace.path
            .with_join_str("package.json");

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut document = JsonDocument::new(manifest_content)?;

        for resolution in loose_resolutions.iter() {
            document.update_path(
                &zpm_parsers::Path::from_segments(vec!["dependencies".to_string(), resolution.descriptor.ident.to_file_string()]),
                Value::String(resolution.descriptor.range.to_anonymous_range().to_file_string()),
            )?;

            document.update_path(
                &zpm_parsers::Path::from_segments(vec!["devDependencies".to_string(), resolution.descriptor.ident.to_file_string()]),
                Value::String(resolution.descriptor.range.to_anonymous_range().to_file_string()),
            )?;

            document.update_path(
                &zpm_parsers::Path::from_segments(vec!["optionalDependencies".to_string(), resolution.descriptor.ident.to_file_string()]),
                Value::String(resolution.descriptor.range.to_anonymous_range().to_file_string()),
            )?;
        }

        manifest_path
            .fs_change(&document.input, false)?;

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
