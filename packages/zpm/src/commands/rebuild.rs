use clipanion::cli;

use crate::{build::BuildState, error::Error, primitives::Ident, project::{self, RunInstallOptions}};

#[cli::command]
#[cli::path("rebuild")]
#[cli::category("Dependency management")]
#[cli::description("Rebuild dependencies")]
pub struct Rebuild {
    identifiers: Vec<Ident>,
}

impl Rebuild {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        let mut build_state
            = BuildState::load(&project).await;

        if self.identifiers.is_empty() {
            build_state.entries.clear();
        } else {
            for ident in &self.identifiers {
                build_state.entries.retain(|locator, _| {
                    locator.ident != *ident
                });
            }
        }

        build_state.save(&project)?;

        project.run_install(RunInstallOptions::default()).await?;

        Ok(())
    }
}
