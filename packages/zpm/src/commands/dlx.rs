use std::{process::ExitStatus};

use zpm_parsers::{JsonFormatter, JsonValue};
use zpm_utils::{Path, ToFileString};
use clipanion::cli;
use zpm_semver::RangeKind;

use crate::{error::Error, install::InstallContext, primitives::{loose_descriptor, Descriptor, LooseDescriptor}, project::{self, Project}, script::{Binary, ScriptEnvironment}};

#[cli::command(proxy)]
#[cli::path("dlx")]
#[cli::category("Scripting commands")]
#[cli::description("Install a temporary package and run it")]
pub struct DlxWithPackages {
    #[cli::option("-p,--package", min_len = 1)]
    packages: Vec<LooseDescriptor>,

    name: String,
    args: Vec<String>,
}

impl DlxWithPackages {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let (project, current_cwd)
            = setup_project().await?;

        let package_cache
            = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let resolve_options = loose_descriptor::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind: RangeKind::Exact,
            resolve_tags: true,
        };

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &self.packages).await?;

        let project
            = install_dependencies(&project.project_cwd, descriptors).await?;

        let bin
            = find_binary(&project, self.name.as_str(), false)?;

        run_binary(&project, bin, self.args.clone(), current_cwd).await
    }
}

#[cli::command(proxy)]
#[cli::path("dlx")]
pub struct Dlx {
    package: LooseDescriptor,
    args: Vec<String>,
}

impl Dlx {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let (project, current_cwd)
            = setup_project().await?;

        let package_cache
            = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let resolve_options = loose_descriptor::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind: RangeKind::Exact,
            resolve_tags: true,
        };

        let descriptor
            = self.package.resolve(&install_context, &resolve_options).await?;

        let project
            = install_dependencies(&project.project_cwd, vec![descriptor.clone()]).await?;

        let bin
            = find_binary(&project, descriptor.ident.name(), true)?;

        run_binary(&project, bin, self.args.clone(), current_cwd).await
    }
}

async fn setup_project() -> Result<(Project, Path), Error> {
    let temp_dir
        = Path::temp_dir_pattern("dlx-<>")?;

    temp_dir.with_join_str("package.json")
        .fs_write_text("{}\n")?;
    temp_dir.with_join_str("yarn.lock")
        .fs_write_text("{}\n")?;

    let current_cwd
        = Path::current_dir()?;

    std::env::set_current_dir(temp_dir.to_path_buf())?;

    let project
        = project::Project::new(None).await?;

    Ok((project, current_cwd))
}

async fn install_dependencies(workspace_path: &Path, descriptors: Vec<Descriptor>) -> Result<Project, Error> {
    let manifest_path = workspace_path
        .with_join_str("package.json");

    let manifest_content = manifest_path
        .fs_read_text_prealloc()?;

    let mut formatter
        = JsonFormatter::from(&manifest_content)?;

    for descriptor in descriptors.into_iter() {
        formatter.update(
            vec!["dependencies".to_string(), descriptor.ident.to_file_string()], 
            JsonValue::String(descriptor.range.to_file_string()),
        )?;
    }

    let updated_content
        = formatter.to_string();

    manifest_path
        .fs_change(&updated_content, false)?;

    let mut project
        = project::Project::new(None).await?;

    project
        .run_install(project::RunInstallOptions {
            ..Default::default()
        }).await?;

    Ok(project)
}

fn find_binary(project: &Project, preferred_name: &str, fallback: bool) -> Result<Binary, Error> {
    let root_workspace
        = project.root_workspace();

    let visible_bins
        = project.package_visible_binaries(&root_workspace.locator())?;

    if visible_bins.is_empty() {
        return Err(Error::MissingBinariesDlxContent);
    }

    if let Some(bin) = visible_bins.get(preferred_name) {
        Ok(bin.clone())
    } else if fallback {
        if visible_bins.len() == 1 {
            Ok(visible_bins.into_iter().next().unwrap().1)
        } else {
            Err(Error::AmbiguousDlxContext)
        }
    } else {
        Err(Error::BinaryNotFound(preferred_name.to_string()))
    }
}

async fn run_binary(project: &Project, bin: Binary, args: Vec<String>, current_cwd: Path) -> Result<ExitStatus, Error> {
    Ok(ScriptEnvironment::new()?
        .with_project(&project)
        .with_package(&project, &project.active_package()?)?
        .with_cwd(current_cwd)
        .enable_shell_forwarding()
        .run_binary(&bin, &args)
        .await
        .into())
}
