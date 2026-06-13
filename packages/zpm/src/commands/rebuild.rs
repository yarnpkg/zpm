use clipanion::cli;
use zpm_primitives::Ident;

use crate::{
    build::BuildState,
    error::Error,
    project::{self, RunInstallOptions},
};

/// Rebuild dependencies
///
/// This command makes Yarn forget previous build results for selected packages and runs the install pipeline again so they can be rebuilt.
///
/// Note that while Yarn forgets the compilation, the previous artifacts aren't erased from the filesystem and may affect the next builds (in good
/// or bad). To avoid this, you may remove the `.yarn/unplugged` folder, or any other relevant location where packages might have been stored (Yarn
/// may offer a way to do that automatically in the future).
///
/// By default all packages are rebuilt. Pass package names to rebuild only specific packages.
///
#[cli::command]
#[cli::path("rebuild")]
#[cli::category("Dependency management")]
pub struct Rebuild {
    /// Package names to rebuild
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
