use zpm_utils::Path;
use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    pack::{pack_workspace, PackOptions},
    project::Project,
};

/// Pack the project into a distributable archive
///
/// This command turns the active workspace into a compressed archive suitable for publishing. By default the archive is written to `package.tgz` at
/// the workspace root.
///
/// If `--out` is set, the archive is written to the specified path. The `%s` and `%v` placeholders are replaced by the package name and version.
///
#[cli::command]
#[cli::path("pack")]
#[cli::category("Release commands")]
pub struct Pack {
    /// Print the files that would be included without generating the archive
    #[cli::option("-n,--dry-run", default = false)]
    dry_run: bool,

    /// Run `yarn install` first when the package needs build scripts
    #[cli::option("--install-if-needed", default = false)]
    install_if_needed: bool,

    /// Keep `workspace:` ranges unchanged in the generated package manifest
    #[cli::option("--preserve-workspaces", default = false)]
    preserve_workspaces: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Path where the archive should be written
    #[cli::option("--out")]
    out: Option<Path>,
}

impl Pack {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let pack_locator
            = project.active_workspace()?.locator();

        let pack_result
            = pack_workspace(&mut project, &pack_locator, &PackOptions {
                preserve_workspaces: self.preserve_workspaces,
            }).await?;

        if self.dry_run {
            if self.json {
                for path in pack_result.pack_list {
                    println!("{}", zpm_parsers::Value::Object(vec![(
                        "location".to_string(),
                        zpm_parsers::Value::String(path.to_file_string()),
                    )]).to_json_string());
                }
            } else {
                for path in pack_result.pack_list {
                    println!("{}", path.to_file_string());
                }
            }
        } else {
            let active_workspace
                = project.workspace_by_locator(&pack_locator)?;

            let out_path
                = self.out.as_ref().map_or_else(
                    || Ok(active_workspace.path.with_join_str("package.tgz")),
                    |out| pack_result.resolve_out_path(out),
                )?;

            out_path
                .fs_create_parent()?
                .fs_write(&pack_result.pack_file)?;
        }

        Ok(())
    }
}
