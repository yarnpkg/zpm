use std::collections::BTreeMap;

use clipanion::cli;
use zpm_primitives::{Descriptor, Reference};

use crate::{
    error::Error,
    project::{self, RunInstallOptions},
};

/// Pins the resolution of a dependency to a specific version
#[cli::command]
#[cli::path("set", "resolution")]
#[cli::category("Dependency management")]
pub struct SetResolution {
    descriptor: Descriptor,
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
