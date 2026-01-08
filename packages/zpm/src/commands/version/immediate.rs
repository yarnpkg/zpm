use clipanion::cli;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{ToHumanString, impl_file_string_from_str};

use crate::{commands::version::deferred::VersionDeferred, error::Error, project, versioning::{ReleaseStrategy, Versioning}};

#[zpm_enum(error = zpm_utils::EnumError, or_else = |s| Err(zpm_utils::EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[derive_variants(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionBump {
    #[literal("major")]
    Major,

    #[literal("minor")]
    Minor,

    #[literal("patch")]
    Patch,

    #[literal("premajor")]
    Premajor,

    #[literal("preminor")]
    Preminor,

    #[literal("prepatch")]
    Prepatch,

    #[literal("rc")]
    #[literal("pre")]
    #[literal("prerelease")]
    Pre,

    #[literal("decline")]
    Decline,

    #[pattern(spec = r"(?<version>.*)")]
    Exact {
        version: zpm_semver::Version,
    },
}

impl TryFrom<VersionBump> for Option<ReleaseStrategy> {
    type Error = Error;

    fn try_from(version_bump: VersionBump) -> Result<Self, Self::Error> {
        match version_bump {
            VersionBump::Major
                => Ok(Some(ReleaseStrategy::Major)),
            VersionBump::Minor
                => Ok(Some(ReleaseStrategy::Minor)),
            VersionBump::Patch
                => Ok(Some(ReleaseStrategy::Patch)),
            VersionBump::Decline
                => Ok(None),

            _ => {
                Err(Error::InvalidDeferredVersionBump(version_bump.to_print_string()))
            },
        }
    }
}

impl_file_string_from_str!(VersionBump);

#[cli::command]
#[cli::path("version")]
#[cli::category("Project management")]
pub struct Version {
    #[cli::option("-d,--deferred", default = false)]
    deferred: bool,

    #[cli::option("-i,--immediate", default = false)]
    immediate: bool,

    version_bump: VersionBump,
}

impl Version {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let deferred
            = !self.immediate && (self.deferred || project.config.settings.prefer_deferred_versions.value);

        let versioning
            = Versioning::new(&project);

        let active_workspace
            = project.active_workspace()?;

        if deferred {
            versioning.set_workspace_release_strategy(
                &active_workspace.name,
                self.strategy.clone().into(),
            ).await?;

            return Ok(());
        }

        let current_version
            = active_workspace.manifest.remote.version.as_ref()
                .ok_or(Error::NoVersionFoundForActiveWorkspace)?;

        let new_version = match &self.version_bump {
            VersionBump::Major => current_version.next_major(),
            VersionBump::Minor => current_version.next_minor(),
            VersionBump::Patch => current_version.next_patch(),

            VersionBump::Premajor => current_version.next_major_rc(),
            VersionBump::Preminor => current_version.next_minor_rc(),
            VersionBump::Prepatch => current_version.next_patch_rc(),

            VersionBump::Pre => current_version.next_rc(),

            VersionBump::Exact(params) => {
                params.version.clone()
            },

            VersionBump::Decline => {
                return Err(Error::VersionDeclineNotAllowed);
            },
        };

        versioning.set_immediate_version(
            &active_workspace.name,
            &new_version,
        )?;

        println!("Bumped from {} to {}", current_version.to_print_string(), new_version.to_print_string());

        Ok(())
    }
}
