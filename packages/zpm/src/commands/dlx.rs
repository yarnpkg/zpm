use std::process::ExitStatus;

use arca::Path;
use clipanion::cli;

use crate::{error::Error, primitives::{descriptor::LooseDescriptor, Descriptor}, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("dlx")]
pub struct DlxWithPackages {
    #[cli::option("-p,--package", required)]
    packages: Vec<LooseDescriptor>,

    name: String,
    args: Vec<String>,
}

impl DlxWithPackages {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let packages = self.packages.iter()
            .map(|p| p.descriptor.clone())
            .collect();

        run_dlx(packages, self.name.clone(), self.args.clone()).await
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
        let name
            = self.package.descriptor.ident.name().to_string();
        let packages
            = vec![self.package.descriptor.clone()];

        run_dlx(packages, name, self.args.clone()).await
    }
}

async fn run_dlx(packages: Vec<Descriptor>, name: String, args: Vec<String>) -> Result<ExitStatus, Error> {
    let temp_dir
        = Path::temp_dir_pattern("dlx-<>")?;

    temp_dir.with_join_str("package.json")
        .fs_write_text("{}\n")?;
    temp_dir.with_join_str("yarn.lock")
        .fs_write_text("{}\n")?;

    let current_cwd
        = Path::current_dir()?;

    std::env::set_current_dir(temp_dir.to_path_buf())?;

    let mut project
        = project::Project::new(None).await?;

    let root_workspace
        = project.root_workspace_mut();

    for package in packages.into_iter() {
        root_workspace.manifest.remote.dependencies.insert(package.ident.clone(), package);
    }

    root_workspace.write_manifest()?;

    project
        .run_install().await?;

    let root_workspace
        = project.root_workspace();

    let visible_bins
        = project.package_visible_binaries(&root_workspace.locator())?;

    if visible_bins.is_empty() {
        return Err(Error::MissingBinariesDlxContent);
    }

    let bin = if let Some(bin) = visible_bins.get(name.as_str()) {
        bin.clone()
    } else if visible_bins.len() == 1 {
        visible_bins.into_iter().next().unwrap().1
    } else {
        return Err(Error::AmbiguousDlxContext);
    };

    Ok(ScriptEnvironment::new()
        .with_project(&project)
        .with_package(&project, &project.active_package()?)?
        .with_cwd(current_cwd)
        .enable_shell_forwarding()
        .run_binary(&bin, &args)
        .await
        .into())
}
