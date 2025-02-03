use std::process::ExitCode;

use clipanion::cli;

use crate::{error::Error, pack::pack_list, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("pack")]
pub struct Pack {
    #[cli::option("-n,--dry-run")]
    dry_run: bool,

    #[cli::option("--install-if-needed")]
    install_if_needed: bool,
}

impl Pack {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .import_install_state()?;

        let active_workspace
            = project.active_workspace()?;

        let pack_list
            = pack_list(&project, active_workspace)?;

        if self.dry_run {
            for pack in pack_list {
                println!("{}", pack);
            }

            return Ok(ExitCode::SUCCESS);
        }

        let prepack_script
            = project.find_script("prepack")
                .map(Some).or_else(|e| e.ignore(|e| matches!(e, Error::ScriptNotFound(_))))?;

        let postpack_script
            = project.find_script("postpack")
                .map(Some).or_else(|e| e.ignore(|e| matches!(e, Error::ScriptNotFound(_))))?;

        if let Some((locator, script)) = prepack_script {
            ScriptEnvironment::new()
                .with_project(&project)
                .with_package(&project, &locator)?
                .run_script(&script, &Vec::<&str>::new())
                .await
                .ok()?;
        }

        let entries
            = zpm_formats::entries_from_files(&active_workspace.path, &pack_list)?;

        let entries
            = zpm_formats::prefix_entries(entries, active_workspace.name.nm_subdir());

        let packed_file
            = zpm_formats::tar::craft_tgz(&entries)?;

        if let Some((locator, script)) = postpack_script {
            ScriptEnvironment::new()
                .with_project(&project)
                .with_package(&project, &locator)?
                .run_script(&script, &Vec::<&str>::new())
                .await
                .ok()?;
        }

        active_workspace.path
            .with_join_str("package.tgz")
            .fs_write(&packed_file)?;

        Ok(ExitCode::SUCCESS)
    }
}
