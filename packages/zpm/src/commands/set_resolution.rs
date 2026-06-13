use std::collections::BTreeMap;

use clipanion::cli;
use zpm_primitives::{Descriptor, Reference};

use crate::{
    error::Error,
    project::{self, RunInstallOptions},
};

/// Pin a descriptor to a specific resolution
///
/// This command updates the current lockfile resolution table so the given descriptor resolves to the given reference.
///
/// The override is stored in the lockfile, not the manifest. It disappears if the lockfile entry disappears. To make it persistent, edit the
/// top-level `resolutions` field instead.
///
/// Note that no attempt is made at validating that `resolution` is a valid resolution entry for `descriptor`.
///
#[cli::command]
#[cli::path("set", "resolution")]
#[cli::category("Dependency management")]
pub struct SetResolution {
    /// Descriptor whose resolution should be overridden
    descriptor: Descriptor,

    /// Reference to resolve the descriptor to
    reference: Reference,
}

impl SetResolution {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        let locator
            = self.descriptor.resolve_with(self.reference.clone());

        let enforced_resolutions
            = BTreeMap::from([(self.descriptor.clone(), Some(locator))]);

        project.run_install(RunInstallOptions{
            enforced_resolutions,
            ..Default::default()
        }).await?;

        Ok(())
    }
}
