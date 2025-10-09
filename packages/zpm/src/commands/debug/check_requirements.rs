use clipanion::cli;

use crate::{error::Error, project::Project, system::{Requirements, System}};

#[cli::command]
#[cli::path("debug", "check-requirements")]
pub struct CheckRequirements {
    requirements: Requirements,
}

impl CheckRequirements {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let systems
            = System::from_supported_architectures(&project.config.settings.supported_architectures);

        println!("Systems: {:#?}", systems);
        println!();
        println!("Requirements: {:#?}", self.requirements);
        println!();
        println!("Is valid system? {}", self.requirements.validate_any(&systems));

        Ok(())
    }
}
