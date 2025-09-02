use std::collections::BTreeSet;

use zpm_primitives::Locator;
use zpm_utils::Path;
use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    manifest::helpers::parse_manifest,
    pack::{pack_list, pack_manifest},
    project::{Project, RunInstallOptions, Workspace},
    script::ScriptEnvironment,
};

#[cli::command(proxy)]
#[cli::path("pack")]
#[cli::category("Release commands")]
#[cli::description("Pack the project into a distributable archive")]
pub struct Pack {
    #[cli::option("-n,--dry-run", default = false)]
    dry_run: bool,

    #[cli::option("--install-if-needed", default = false)]
    install_if_needed: bool,

    #[cli::option("--json", default = false)]
    json: bool,

    #[cli::option("--out")]
    out: Option<Path>,
}

impl Pack {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let active_workspace
            = project.active_workspace()?;

        let prepack_script
            = project.find_package_script(&active_workspace.locator(), "prepack")
                .map(Some).or_else(|e| e.ignore(|e| matches!(e, Error::ScriptNotFound(_))))?;

        let postpack_script
            = project.find_package_script(&active_workspace.locator(), "postpack")
                .map(Some).or_else(|e| e.ignore(|e| matches!(e, Error::ScriptNotFound(_))))?;

        if prepack_script.is_some() || postpack_script.is_some() {
            if self.install_if_needed {
                project.run_install(RunInstallOptions {
                    ..Default::default()
                }).await?;
            } else {
                project.import_install_state()?;
            }
        }

        self.maybe_run_script(&project, prepack_script).await?;
        let result = self.run_command(&project).await;
        self.maybe_run_script(&project, postpack_script).await?;

        result
    }

    async fn maybe_run_script(&self, project: &Project, script: Option<(Locator, String)>) -> Result<(), Error> {
        if let Some((locator, script)) = script {
            ScriptEnvironment::new()?
                .with_project(&project)
                .with_package(&project, &locator)?
                .run_script(&script, &Vec::<&str>::new())
                .await?
                .ok()?;
        }

        Ok(())
    }

    async fn run_command(&self, project: &Project) -> Result<(), Error> {
        let active_workspace
            = project.active_workspace()?;

        if self.dry_run {
            self.dry_run(&project, active_workspace).await
        } else {
            self.gen_archive(&project, active_workspace).await
        }
    }

    async fn dry_run(&self, project: &Project, active_workspace: &Workspace) -> Result<(), Error> {
        let pack_manifest_content
            = pack_manifest(project, active_workspace)?;

        let pack_manifest
            = parse_manifest(&pack_manifest_content)?;

        let pack_list
            = pack_list(&project, active_workspace, &pack_manifest)?;

        if self.json {
            for path in pack_list {
                sonic_rs::json!({"location": path}).to_string();
            }
        } else {
            for path in pack_list {
                path.to_file_string();
            }
        }

        Ok(())
    }

    async fn gen_archive(&self, project: &Project, active_workspace: &Workspace) -> Result<(), Error> {
        let pack_manifest_content
            = pack_manifest(project, active_workspace)?;

        let pack_manifest
            = parse_manifest(&pack_manifest_content)?;

        let pack_list
            = pack_list(&project, active_workspace, &pack_manifest)?;

        let mut entries
            = zpm_formats::entries_from_files(&active_workspace.path, &pack_list)?;

        let mut executable_files
            = pack_manifest.publish_config.executable_files
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect::<BTreeSet<_>>();

        if let Some(bin) = &pack_manifest.bin {
            executable_files.extend(bin.paths().cloned());
        }

        for entry in entries.iter_mut() {
            if executable_files.contains(&Path::try_from(&entry.name)?) {
                entry.mode = 0o755;
            } else {
                entry.mode = 0o644;
            }
        }

        let manifest_entry = entries
            .iter_mut()
            .find(|entry| entry.name == "package.json");

        if let Some(manifest_entry) = manifest_entry {
            manifest_entry.data = pack_manifest_content.into_bytes().into();
        }

        let entries
            = zpm_formats::prefix_entries(entries, "package");

        let packed_file
            = zpm_formats::tar::craft_tgz(&entries)?;


        let package_name
            = pack_manifest.name.map_or_else(
                || "package".to_string(),
                |name| name.slug());

        let package_version
            = pack_manifest.remote.version.as_ref().map_or_else(
                || "0.0.0".to_string(),
                |v| v.to_file_string());

        self.get_out_path(&active_workspace.path, &package_name, &package_version)?
            .fs_create_parent()?
            .fs_write(&packed_file)?;

        Ok(())
    }

    fn get_out_path(&self, workspace_abs_path: &Path, package_name: &str, package_version: &str) -> Result<Path, Error> {
        let Some(out) = &self.out else {
            return Ok(workspace_abs_path.with_join_str("package.tgz"));
        };

        let out_str = out.to_file_string();

        let out_str = out_str.replace("%s", package_name);
        let out_str = out_str.replace("%v", package_version);

        Ok(Path::current_dir()?.with_join_str(&out_str))
    }
}
