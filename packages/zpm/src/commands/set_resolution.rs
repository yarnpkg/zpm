use std::collections::BTreeMap;

use clipanion::cli;
use zpm_primitives::{Descriptor, Reference};

use crate::{
    error::Error,
    project::{self, RunInstallOptions},
};

/// Pins the resolution of a dependency to a specific version
///
/// This command updates the resolution table so that descriptor is resolved by resolution.
///
/// Note that by default this command only affect the current resolution table - meaning that this "manual override" will disappear if you remove the
/// lockfile, or if the package disappear from the table. If you wish to make the enforced resolution persist whatever happens, edit the `resolutions`
/// field in your top-level manifest.
///
/// Note that no attempt is made at validating that `resolution` is a valid resolution entry for `descriptor`.
///
#[cli::command]
#[cli::path("set", "resolution")]
#[cli::category("Dependency management")]
pub struct SetResolution {
    /// The descriptor to set the resolution for
    descriptor: Descriptor,

    /// The reference to set the resolution for
    reference: Reference,
}

impl SetResolution {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        let locator
            = self.descriptor.resolve_with(self.reference.clone());

        let enforced_resolutions
            = BTreeMap::from([(self.descriptor.clone(), locator)]);

        project.run_install(RunInstallOptions{
            enforced_resolutions,
            ..Default::default()
        }).await?;

        Ok(())
    }
}
