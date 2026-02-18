use clipanion::cli;
use zpm_primitives::IdentGlob;

use crate::{
    error::Error,
    project::{InstallMode, Project, RunInstallOptions},
};

/// Update dependencies to the latest versions by re-resolving them
///
/// This command forces all ranges matching the selected packages to be resolved again (often to
/// the highest available versions) before being stored in the lockfile. Unlike `yarn up`, it
/// won't touch your manifests, so depending on your needs you might want to run both `yarn up`
/// and `yarn up -R` to cover all bases.
///
/// This is useful when you want to update transitive dependencies without modifying your
/// package.json files. The matching descriptor entries are removed from the imported lockfile
/// before the install, causing the project to re-resolve them.
///
/// If the `--mode=<mode>` option is set, Yarn will change which artifacts are generated. The
/// modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting
///   `enableScripts` to false because the latter will disable build scripts, and thus affect the
///   content of the artifacts generated on disk, whereas the former will just disable the build
///   step but not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are
///   missing from the lockfile (or that have no associated checksums). This mode is typically
///   used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring
///   the full install cost.
///
/// This command accepts glob patterns as arguments. Make sure to escape the patterns, to prevent
/// your own shell from trying to expand them.
///
#[cli::command]
#[cli::path("up", "-R")]
#[cli::category("Dependency management")]
pub struct UpRecursive {
    /// Change what artifacts this install will generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    /// The packages patterns to re-resolve
    patterns: Vec<IdentGlob>,
}

impl UpRecursive {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project = Project::new(None).await?;

        let lockfile = project.lockfile()?;

        // Find all descriptors matching the patterns and mark them for removal (None)
        let enforced_resolutions = lockfile.resolutions.keys()
            .filter(|descriptor| {
                self.patterns.iter().any(|pattern| pattern.check(&descriptor.ident))
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
}
