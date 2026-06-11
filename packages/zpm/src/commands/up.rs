use std::collections::BTreeSet;

use clipanion::cli;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_primitives::{Ident, IdentGlob};
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
/// This command upgrades packages matching the specified descriptors across the whole project. It updates `dependencies` and `devDependencies`;
/// `peerDependencies` are left unchanged.
///
/// If `-R,--recursive` is set, Yarn refreshes all lockfile resolutions matching the selected packages without editing manifests. Depending on the
/// goal, you may want to run both `yarn up` and `yarn up -R`.
///
/// If `-i,--interactive` is set, or if the `preferInteractive` setting is enabled, the command offers various choices depending on the
/// detected upgrade paths. Some upgrades require this flag in order to resolve ambiguities.
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
            let manifest_path = workspace.path
                .with_join_str("package.json");

            let manifest_content = manifest_path
                .fs_read_prealloc()?;

            let mut document
                = JsonDocument::new(manifest_content)?;

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

        let enforced_resolutions = lockfile.resolutions.keys()
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
