use clipanion::cli;
use zpm_config::Source;

use crate::{error::Error, project::{self, InstallMode, RunInstallOptions}};

/// Install dependencies
///
/// This command sets up your project if needed. The installation is split into four different steps that each have their own characteristics:
///
/// - **Resolution:** First the package manager will resolve your dependencies. The exact way a dependency version is privileged over another isn't standardized outside of the regular semver guarantees. If a package doesn't resolve to what you would expect, check that all dependencies are correctly declared (also check our website for more information: ).
///
/// - **Fetch:** Then we download all the dependencies if needed, and make sure that they're all stored within our cache (check the value of `cacheFolder` in `yarn config` to see where the cache files are stored).
///
/// - **Link:** Then we send the dependency tree information to internal plugins tasked with writing them on the disk in some form (for example by generating the `.pnp.cjs` file you might know).
///
/// - **Build:** Once the dependency tree has been written on the disk, the package manager will now be free to run the build scripts for all packages that might need it, in a topological order compatible with the way they depend on one another. See https://yarnpkg.com/advanced/lifecycle-scripts for detail.
///
/// Note that running this command is not part of the recommended workflow. Yarn supports zero-installs, which means that as long as you store your cache and your `.pnp.cjs` file inside your repository, everything will work without requiring any install right after cloning your repository or switching branches.
///
/// If the `--immutable` option is set (defaults to true on CI), Yarn will abort with an error exit code if the lockfile was to be modified (other paths can be added using the `immutablePatterns` configuration setting). For backward compatibility we offer an alias under the name of `--frozen-lockfile`, but it will be removed in a later release.
///
/// If the `--immutable-cache` option is set, Yarn will abort with an error exit code if the cache folder was to be modified (either because files would be added, or because they'd be removed).
///
/// If the `--refresh-lockfile` option is set, Yarn will keep the same resolution for the packages currently in the lockfile but will refresh their metadata. If used together with `--immutable`, it can validate that the lockfile information are consistent. This flag is enabled by default when Yarn detects it runs within a pull request context.
///
/// If the `--check-cache` option is set, Yarn will always refetch the packages and will ensure that their checksum matches what's 1/ described in the lockfile 2/ inside the existing cache files (if present). This is recommended as part of your CI workflow if you're both following the Zero-Installs model and accepting PRs from third-parties, as they'd otherwise have the ability to alter the checked-in packages before submitting them.
///
/// If the `--inline-builds` option is set, Yarn will verbosely print the output of the build steps of your dependencies (instead of writing them into individual files). This is likely useful mostly for debug purposes only when using Docker-like environments.
///
/// If the `--mode=<mode>` option is set, Yarn will change which artifacts are generated. The modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting `enableScripts` to false because the latter will disable build scripts, and thus affect the content of the artifacts generated on disk, whereas the former will just disable the build step - but not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are missing from the lockfile (or that have no associated checksums). This mode is typically used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring the full install cost.
///
#[cli::command(default)]
#[cli::path("install")]
#[cli::category("Dependency management")]
pub struct Install {
    /// Validates that the package resolutions are coherent
    #[cli::option("--check-resolutions", default = false)]
    check_resolutions: bool,

    /// Abort with an error exit code if the lockfile was to be modified
    #[cli::option("--immutable", default = false)]
    immutable: bool,

    /// Abort with an error exit code if the cache folder was to be modified
    #[cli::option("--immutable-cache", default = false)]
    immutable_cache: bool,

    #[cli::option("--check-checksums", default = false)]
    check_checksums: bool,

    /// Refresh the package metadata stored in the lockfile
    #[cli::option("--refresh-lockfile", default = false)]
    refresh_lockfile: bool,

    /// Select the artifacts this install will generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,
}

impl Install {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        if self.immutable {
            project.config.settings.enable_immutable_installs.value = true;
            project.config.settings.enable_immutable_installs.source = Source::Cli;
        }

        if self.immutable_cache {
            project.config.settings.enable_immutable_cache.value = true;
            project.config.settings.enable_immutable_cache.source = Source::Cli;
        }

        project.run_install(RunInstallOptions {
            check_checksums: self.check_checksums,
            check_resolutions: self.check_resolutions,
            refresh_lockfile: self.refresh_lockfile,
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }
}
