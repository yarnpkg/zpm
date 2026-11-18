use clipanion::cli;

use crate::{error::Error, project};

use super::immediate::{bump_versions, parse_prerelease_pattern, BumpOptions};

/// Apply deferred records to workspaces
///
/// This command will apply the deferred version changes to workspaces. The applied records are removed from the project, unless `--prerelease` is
/// set, in which case they are kept so that a later stable apply can still consume them.
///
/// It is equivalent to running `yarn version decline --immediate`: by default only the current workspace will be bumped, `--all` applies the
/// deferred records of all the workspaces in the project, and attempting to apply a record that would lower the version of a workspace will throw
/// an error unless `--force` is set. For more information on the options, see the `yarn version` command.
///
/// This command only updates the versions of the bumped workspaces; it doesn't update the ranges that other workspaces use to depend on them. Use
/// the `workspace:^` / `workspace:~` ranges if you need them to follow the version bumps.
///
/// ## Examples
///
/// Apply the deferred version change to the current workspace
///
/// `yarn version apply`
///
/// Apply the deferred version changes to all the workspaces of the project
///
/// `yarn version apply --all`
///
#[cli::command]
#[cli::path("version", "apply")]
#[cli::category("Project management")]
pub struct VersionApply {
    /// Apply the deferred records of all workspaces
    #[cli::option("--all", default = false)]
    all: bool,

    /// Not supported yet; consider using `--all` instead
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Specify a prerelease pattern to use when computing the new versions
    #[cli::option("--prerelease")]
    prerelease: Option<Option<String>>,

    /// Print the new versions without actually bumping them
    #[cli::option("--dry-run", default = false)]
    dry_run: bool,

    /// Bypass the check for bumping to a lower version than the current one
    #[cli::option("--force", default = false)]
    force: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,
}

impl VersionApply {
    pub async fn execute(&self) -> Result<(), Error> {
        if self.recursive && !self.all {
            return Err(Error::RecursiveVersionApplyNotImplemented);
        }

        let project
            = project::Project::new(None).await?;

        bump_versions(&project, BumpOptions {
            strategy: None,
            all: self.all,
            deferred: false,
            prerelease: parse_prerelease_pattern(&self.prerelease)?,
            dry_run: self.dry_run,
            force: self.force,
            json: self.json,
        }).await
    }
}
