use clipanion::cli;
use zpm_macro_enum::zpm_enum;
use zpm_utils::ToHumanString;

use crate::{error::Error, project, versioning::{ExactReleaseStrategy, ReleaseStrategy, Versioning}};

#[zpm_enum(error = zpm_utils::EnumError, or_else = |s| Err(zpm_utils::EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[derive_variants(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeferredStrategy {
    #[literal("major")]
    Major,

    #[literal("minor")]
    Minor,

    #[literal("patch")]
    Patch,

    #[literal("decline")]
    Decline,

    #[pattern(r"(?<version>.*)")]
    #[to_file_string(|params| params.version.to_file_string())]
    #[to_print_string(|params| params.version.to_print_string())]
    Exact {
        version: zpm_semver::Version,
    },
}

impl From<DeferredStrategy> for Option<ReleaseStrategy> {
    fn from(strategy: DeferredStrategy) -> Self {
        match strategy {
            DeferredStrategy::Major
                => Some(ReleaseStrategy::Major),
            DeferredStrategy::Minor
                => Some(ReleaseStrategy::Minor),
            DeferredStrategy::Patch
                => Some(ReleaseStrategy::Patch),
            DeferredStrategy::Exact(params)
                => Some(ExactReleaseStrategy { version: params.version.clone() }.into()),
            DeferredStrategy::Decline
                => None,
        }
    }
}

#[cli::command]
#[cli::path("version")]
#[cli::category("Project management")]
pub struct VersionDeferred {
    #[cli::option("-d,--deferred")]
    pub _deferred: bool,

    pub strategy: DeferredStrategy,
}

impl VersionDeferred {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let active_workspace
            = project.active_workspace()?;

        let versioning
            = Versioning::new(&project);

        versioning.set_workspace_release_strategy(
            &active_workspace.name,
            self.strategy.clone().into(),
        ).await?;

        println!("Marked {} has requiring a {} release", active_workspace.name.to_print_string(), self.strategy.to_print_string());

        Ok(())
    }
}
