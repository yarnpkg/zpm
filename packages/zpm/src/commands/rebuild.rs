use clipanion::cli;
use zpm_primitives::Ident;

use crate::{
    build::BuildState,
    error::Error,
    project::{self, RunInstallOptions},
};

/// Rebuild dependencies
///
/// This command will automatically cause Yarn to forget about previous compilations of the given packages and to run them again.
///
/// Note that while Yarn forgets the compilation, the previous artifacts aren't erased from the filesystem and may affect the next builds (in good
/// or bad). To avoid this, you may remove the `.yarn/unplugged` folder, or any other relevant location where packages might have been stored (Yarn
/// may offer a way to do that automatically in the future).
///
/// By default all packages will be rebuilt, but you can filter the list by specifying the names of the packages you want to clear from memory.
///
#[cli::command]
#[cli::path("rebuild")]
#[cli::category("Dependency management")]
pub struct Rebuild {
    /// The packages to rebuild
    identifiers: Vec<Ident>,
}

impl Rebuild {
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
