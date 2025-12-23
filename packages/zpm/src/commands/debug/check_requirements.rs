use clipanion::cli;
use zpm_parsers::JsonSource;
use zpm_utils::Requirements;

use crate::{error::Error, project::Project};

#[cli::command]
#[cli::path("debug", "check-requirements")]
pub struct CheckRequirements {
    requirements: JsonSource<Requirements>,
}

impl CheckRequirements {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let systems
            = project.config.settings.supported_architectures.to_systems();

        println!("Systems: {:#?}", systems);
        println!();
        println!("Requirements: {:#?}", self.requirements.value);
        println!();
        println!("Is valid system? {}", self.requirements.value.validate_any(&systems));

        Ok(())
    }
}
