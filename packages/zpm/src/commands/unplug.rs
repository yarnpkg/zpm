use clipanion::cli;
use zpm_parsers::{JsonDocument, Value};
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    project,
};

/// Requests a package to be materialized on disk
#[cli::command]
#[cli::path("unplug")]
#[cli::category("Dependency management")]
pub struct Unplug {
    #[cli::option("--revert", default = false)]
    revert: bool,

    identifiers: Vec<Ident>,
}

impl Unplug {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let manifest_path = project.project_cwd
            .with_join_str(project::MANIFEST_NAME);

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut formatter
            = JsonDocument::new(manifest_content)?;

        for identifier in &self.identifiers {
            formatter.set_path(
                &zpm_parsers::Path::from_segments(vec!["dependenciesMeta".to_string(), identifier.to_file_string(), "unplugged".to_string()]),
                if self.revert {Value::Undefined} else {Value::Bool(true)},
            )?;
        }

        manifest_path
            .fs_change(&formatter.input, false)?;

        let mut project
            = project::Project::new(None).await?;

        project.run_install(project::RunInstallOptions {
            ..Default::default()
        }).await?;

        Ok(())
    }
}
