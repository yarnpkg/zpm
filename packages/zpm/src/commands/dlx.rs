use std::{process::ExitStatus};

use zpm_parsers::{JsonFormatter, Value};
use zpm_utils::{Path, ToFileString};
use clipanion::cli;
use zpm_semver::RangeKind;

use crate::{error::Error, install::InstallContext, primitives::{loose_descriptor, Descriptor, LooseDescriptor}, project::{self, Project}, script::{Binary, ScriptEnvironment}};

#[cli::command(proxy)]
#[cli::path("dlx")]
#[cli::category("Scripting commands")]
#[cli::description("Install a temporary package and run it")]
pub struct DlxWithPackages {
    #[cli::option("-q,--quiet", default = false)]
    quiet: bool,

    #[cli::option("-p,--package", min_len = 1)]
    packages: Vec<LooseDescriptor>,

    name: String,
    args: Vec<String>,
}

impl DlxWithPackages {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let dlx_project
            = setup_project().await?;

        let package_cache
            = dlx_project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&dlx_project));

        let resolve_options = loose_descriptor::ResolveOptions {
            active_workspace_ident: dlx_project.active_workspace()?.name.clone(),
            range_kind: RangeKind::Exact,
            resolve_tags: true,
        };

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &self.packages).await?;

        let dlx_project
            = install_dependencies(&dlx_project.project_cwd, descriptors, self.quiet).await?;
        let bin
            = find_binary(&dlx_project, self.name.as_str(), false)?;

        let current_cwd
            = Path::current_dir()?;

        run_binary(&dlx_project, bin, self.args.clone(), current_cwd).await
    }
}

#[cli::command(proxy)]
#[cli::path("dlx")]
pub struct Dlx {
    #[cli::option("-q,--quiet", default = false)]
    quiet: bool,

    package: LooseDescriptor,
    args: Vec<String>,
}

impl Dlx {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let dlx_project
            = setup_project().await?;

        let package_cache
            = dlx_project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&dlx_project));

        let resolve_options = loose_descriptor::ResolveOptions {
            active_workspace_ident: dlx_project.active_workspace()?.name.clone(),
            range_kind: RangeKind::Exact,
            resolve_tags: true,
        };

        let descriptor
            = self.package.resolve(&install_context, &resolve_options).await?;

        let dlx_project
            = install_dependencies(&dlx_project.project_cwd, vec![descriptor.clone()], self.quiet).await?;
        let bin
            = find_binary(&dlx_project, descriptor.ident.name(), true)?;

        let run_cwd
            = Path::current_dir()?;

        run_binary(&dlx_project, bin, self.args.clone(), run_cwd).await
    }
}

pub async fn setup_project() -> Result<Project, Error> {
    let temp_dir
        = Path::temp_dir_pattern("dlx-<>")?;

    temp_dir.with_join_str("package.json")
        .fs_write_text("{}\n")?;
    temp_dir.with_join_str("yarn.lock")
        .fs_write_text("{}\n")?;
    temp_dir.with_join_str(".yarnrc.yml")
        .fs_write_text("enableGlobalCache: false\n")?;

    let project
        = project::Project::new(Some(temp_dir)).await?;

    Ok(project)
}

pub async fn install_dependencies(workspace_path: &Path, descriptors: Vec<Descriptor>, quiet: bool) -> Result<Project, Error> {
    let manifest_path = workspace_path
        .with_join_str("package.json");

    let manifest_content = manifest_path
        .fs_read_text_prealloc()?;

    let mut formatter
        = JsonFormatter::from(&manifest_content)?;

    for descriptor in descriptors.into_iter() {
        formatter.set(
            vec!["dependencies".to_string(), descriptor.ident.to_file_string()],
            Value::String(descriptor.range.to_file_string()),
        )?;
    }

    let updated_content
        = formatter.to_string();

    manifest_path
        .fs_change(&updated_content, false)?;

    let mut project
        = project::Project::new(Some(workspace_path.clone())).await?;

    project
        .run_install(project::RunInstallOptions {
            silent_or_error: quiet,
            ..Default::default()
        }).await?;

    Ok(project)
}

pub fn find_binary(project: &Project, preferred_name: &str, fallback: bool) -> Result<Binary, Error> {
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

pub async fn run_binary(project: &Project, bin: Binary, args: Vec<String>, current_cwd: Path) -> Result<ExitStatus, Error> {
    Ok(ScriptEnvironment::new()?
        .with_project(&project)
        .with_package(&project, &project.root_workspace().locator())?
        .with_cwd(current_cwd)
        .enable_shell_forwarding()
        .run_binary(&bin, &args)
        .await?
        .into())
}
