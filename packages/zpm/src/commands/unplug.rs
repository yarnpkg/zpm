use std::collections::HashSet;

use clipanion::cli;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_primitives::{VersionFilter, Locator, Reference};
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    install::InstallState,
    project,
};

/// Mark packages to be unpacked on disk
///
/// This command stores `dependenciesMeta` entries that force matching packages to be unplugged when installed.
///
/// An unplugged package is extracted into `pnpUnpluggedFolder` instead of being loaded directly from its archive. This is useful for debugging or for
/// packages that need real files on disk, such as packages with native sources or shell scripts.
///
/// The setting is persistent. Use `--revert` or edit the top-level manifest, then run `yarn install` to apply the change.
///
/// By default, only direct dependencies from the current workspace are affected. If `-A,--all` is set, direct dependencies from the entire project are
/// affected. Using the `-R,--recursive` flag will affect transitive dependencies as well as direct ones.
///
/// This command accepts glob patterns inside the scope and name components (not the range). Make sure to escape the patterns to prevent your own
/// shell from trying to expand them.
///
#[cli::command]
#[cli::path("unplug")]
#[cli::category("Dependency management")]
pub struct Unplug {
    /// Remove the unplugged flag for the selected packages
    #[cli::option("--revert", default = false)]
    revert: bool,

    /// Unplug direct dependencies from the entire project
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Unplug both direct and transitive dependencies
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Package patterns to unplug
    patterns: Vec<VersionFilter>,
}

fn package_ident(locator: &Locator) -> &zpm_primitives::Ident {
    match &locator.reference {
        Reference::Registry(params) => &params.ident,
        _ => &locator.ident,
    }
}

impl Unplug {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project.lazy_install().await?;

        let install_state
            = project.install_state
                .as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let matches_any = |ident: &zpm_primitives::Ident, version: &zpm_semver::Version| -> bool {
            self.patterns.iter().any(|p| p.check(ident, version))
        };

        let selected = if self.all && self.recursive {
            self.get_all_matching_packages(install_state, &matches_any)
        } else {
            let roots: Vec<Locator> = if self.all {
                project.workspaces.iter().map(|w| w.locator()).collect()
            } else {
                vec![project.active_workspace()?.locator()]
            };

            self.get_selected_packages(&roots, install_state, &matches_any)
        };

        let manifest_path
            = project.project_cwd
                .with_join_str(project::MANIFEST_NAME);

        let manifest_content
            = manifest_path
                .fs_read_prealloc()?;

        let mut document
            = JsonDocument::new(manifest_content)?;

        let mut output
            = Vec::new();

        for (locator, version) in &selected {
            let ident
                = package_ident(locator);

            let key
                = format!("{}@{}", ident.to_file_string(), version.to_file_string());

            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["dependenciesMeta".to_string(), key, "unplugged".to_string()]),
                if self.revert {Value::Undefined} else {Value::Bool(true)},
            )?;

            if self.json {
                output.push(serde_json::json!({
                    "locator": locator.to_file_string(),
                    "version": version.to_file_string(),
                }));
            }
        }

        manifest_path
            .fs_change(&document.input, false)?;

        let mut project
            = project::Project::new(None).await?;

        project.run_install(project::RunInstallOptions {
            silent_or_error: self.json,
            ..Default::default()
        }).await?;

        for item in output {
            println!("{}", serde_json::to_string(&item).unwrap());
        }

        Ok(())
    }

    fn get_all_matching_packages(
        &self,
        install_state: &InstallState,
        matches_any: &dyn Fn(&zpm_primitives::Ident, &zpm_semver::Version) -> bool,
    ) -> Vec<(Locator, zpm_semver::Version)> {
        let mut selected
            = Vec::new();

        for (locator, resolution) in &install_state.resolution_tree.locator_resolutions {
            if locator.reference.is_workspace_reference() {
                continue;
            }

            if locator.reference.is_virtual_reference() {
                continue;
            }

            let ident
                = package_ident(locator);

            if matches_any(ident, &resolution.version) {
                selected.push((locator.clone(), resolution.version.clone()));
            }
        }

        selected.sort();
        selected.dedup();
        selected
    }

    fn get_selected_packages(
        &self,
        roots: &[Locator],
        install_state: &InstallState,
        matches_any: &dyn Fn(&zpm_primitives::Ident, &zpm_semver::Version) -> bool,
    ) -> Vec<(Locator, zpm_semver::Version)> {
        let mut seen
            = HashSet::new();

        let mut selected
            = Vec::new();

        for root in roots {
            self.traverse(root, 0, &mut seen, install_state, matches_any, &mut selected);
        }

        selected.sort();
        selected.dedup();
        selected
    }

    fn traverse(
        &self,
        locator: &Locator,
        depth: usize,
        seen: &mut HashSet<Locator>,
        install_state: &InstallState,
        matches_any: &dyn Fn(&zpm_primitives::Ident, &zpm_semver::Version) -> bool,
        selected: &mut Vec<(Locator, zpm_semver::Version)>,
    ) {
        if seen.contains(locator) {
            return;
        }

        let is_workspace
            = locator.reference.is_workspace_reference();

        // Don't mark workspace deps as "seen" when not recursive,
        // so they can still be visited as traversal roots in --all mode.
        if depth > 0 && !self.recursive && is_workspace {
            return;
        }

        seen.insert(locator.clone());

        if !is_workspace {
            if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
                let ident
                    = package_ident(locator);

                if matches_any(ident, &resolution.version) {
                    selected.push((locator.physical_locator(), resolution.version.clone()));
                }
            }
        }

        if depth > 0 && !self.recursive {
            return;
        }

        if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
            for descriptor in resolution.dependencies.values() {
                if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                    self.traverse(dep_locator, depth + 1, seen, install_state, matches_any, selected);
                }
            }
        }
    }
}
