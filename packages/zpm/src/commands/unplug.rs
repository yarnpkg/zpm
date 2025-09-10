use clipanion::cli;
use zpm_parsers::{JsonFormatter, Value};
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    project,
};

#[cli::command]
#[cli::path("unplug")]
#[cli::category("Dependency management")]
#[cli::description("Requests a package to be materialized on disk")]
pub struct Unplug {
    #[cli::option("--revert", default = false)]
    revert: bool,

    identifiers: Vec<Ident>,
}

impl Unplug {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let manifest_path = project.project_cwd
            .with_join_str(project::MANIFEST_NAME);

        let manifest_content = manifest_path
            .fs_read_text_prealloc()?;

        let mut formatter
            = JsonFormatter::from(&manifest_content)?;

        for identifier in &self.identifiers {
            formatter.set(
                vec!["dependenciesMeta".to_string(), identifier.to_file_string(), "unplugged".to_string()],
                if self.revert {Value::Undefined} else {Value::Bool(true)},
            )?;
        }

        let updated_content
            = formatter.to_string();

        manifest_path
            .fs_change(&updated_content, false)?;

        let mut project
            = project::Project::new(None).await?;

        project.run_install(project::RunInstallOptions {
            ..Default::default()
        }).await?;

        Ok(())
    }
}
