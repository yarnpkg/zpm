use std::process::ExitStatus;

use zpm_parsers::{Document, JsonDocument, Value};
use zpm_utils::{Path, ToFileString};
use clipanion::cli;
use zpm_semver::RangeKind;

use crate::{
    descriptor_loose::{self, LooseDescriptor, LooseResolution},
    error::Error,
    install::InstallContext,
    project::{Project, RunInstallOptions},
    script::{Binary, ScriptEnvironment},
};

/// Install a temporary package and run it
///
/// This command will install a package within a temporary environment, and run its binary script if it contains any. The binary will run within the
/// current cwd.
///
/// By default Yarn will download the package named command, but this can be changed through the use of the `-p,--package` flag which will instruct
/// Yarn to still run the same command but from a different package.
///
/// Using `yarn dlx` as a replacement of `yarn add` isn't recommended as it makes your project non-deterministic. Yarn doesn't keep track of the
/// packages installed through dlx - neither their name, nor their version).
///
#[cli::command(proxy)]
#[cli::path("dlx")]
#[cli::category("Scripting commands")]
pub struct DlxWithPackages {
    /// Suppress the install unless it errors
    #[cli::option("-q,--quiet", default = false)]
    quiet: bool,

    /// The package(s) to install before running the command
    #[cli::option("-p,--package", min_len = 1)]
    packages: Vec<LooseDescriptor>,

    /// The name of the binary to run
    name: String,

    /// The arguments to pass to the binary
    args: Vec<String>,
}

impl DlxWithPackages {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let dlx_project
            = setup_project().await?;

        let package_cache
            = dlx_project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&dlx_project));

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: dlx_project.active_workspace()?.name.clone(),
            range_kind: RangeKind::Exact,
            resolve_tags: true,
            allow_reuse: true,
        };

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &self.packages).await?;

        if !self.quiet {
            for locator in descriptors.iter().filter_map(|resolution| resolution.locator.as_ref()) {
                println!("➤ · {}", locator.to_file_string());
            }
        }

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
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        install_and_run_single(self.package.clone(), InstallAndRunOptions {
            args: self.args.clone(),
            quiet: self.quiet,
            fallback_binary: true,
            ..Default::default()
        }).await
    }
}

pub async fn setup_project() -> Result<Project, Error> {
    let temp_dir
        = Path::temp_dir_pattern("dlx-<>")?;

    temp_dir.with_join_str("package.json")
        .fs_write_text("{}\n")?;
    temp_dir.with_join_str("yarn.lock")
        .fs_write_text("{}\n")?;

    let calling_cwd = Path::current_dir()?;
    let rc_body = crate::commands::rc_helpers::build_inherited_rc(&calling_cwd);

    temp_dir.with_join_str(".yarnrc.yml")
        .fs_write_text(&rc_body)?;

    let project
        = Project::new(Some(temp_dir)).await?;

    Ok(project)
}

pub async fn install_dependencies(workspace_path: &Path, loose_resolutions: Vec<LooseResolution>, quiet: bool) -> Result<Project, Error> {
    let manifest_path = workspace_path
        .with_join_str("package.json");

    let manifest_content = manifest_path
        .fs_read_prealloc()?;

    let mut formatter
        = JsonDocument::new(manifest_content)?;

    for resolution in &loose_resolutions {
        formatter.set_path(
            &zpm_parsers::Path::from_segments(vec!["dependencies".to_string(), resolution.descriptor.ident.to_file_string()]),
            Value::String(resolution.descriptor.range.to_anonymous_range().to_file_string()),
        )?;
    }

    manifest_path
        .fs_change(&formatter.input, false)?;

    let mut project
        = Project::new(Some(workspace_path.clone())).await?;

    let enforced_resolutions
        = loose_resolutions.into_iter()
            .filter_map(|resolution| resolution.locator.map(|locator| (resolution.descriptor, Some(locator))))
            .collect();

    project
        .run_install(RunInstallOptions {
            silent_or_error: quiet,
            enforced_resolutions,
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
        .enable_signal_delegation()
        .run_binary(&bin, &args)
        .await?
        .into())
}

#[derive(Default)]
pub struct InstallAndRunOptions {
    /// Arguments passed to the binary.
    pub args: Vec<String>,
    /// Suppress the install report unless it errors.
    pub quiet: bool,
    /// Optional banner to emit before the install.
    pub banner: Option<String>,
    /// Override the preferred binary name (defaults to the package name).
    pub binary_name: Option<String>,
    /// Fall back to the sole available binary when the preferred name
    /// isn't found (matches `yarn create` / `yarn dlx -p`).
    pub fallback_binary: bool,
    /// Override the run cwd (defaults to the caller's current dir).
    pub run_cwd: Option<Path>,
}

/// Installs `descriptor` into a temp dlx project and runs its binary —
/// shared backbone for `yarn dlx`, `yarn create`, and `yarn init`.
pub async fn install_and_run_single(
    descriptor: LooseDescriptor,
    options: InstallAndRunOptions,
) -> Result<ExitStatus, Error> {
    if let Some(banner) = options.banner.as_ref() {
        // Direct println so the banner lands before the report opens.
        println!("➤ {}", banner);
    }

    let dlx_project
        = setup_project().await?;

    let package_cache
        = dlx_project.package_cache()?;

    let install_context = InstallContext::default()
        .with_package_cache(Some(&package_cache))
        .with_project(Some(&dlx_project));

    let resolve_options = descriptor_loose::ResolveOptions {
        active_workspace_ident: dlx_project.active_workspace()?.name.clone(),
        range_kind: RangeKind::Exact,
        resolve_tags: true,
        allow_reuse: true,
    };

    let resolution
        = descriptor.resolve(&install_context, &resolve_options).await?;

    let binary_name = options.binary_name
        .unwrap_or_else(|| resolution.descriptor.ident.name().to_string());

    let dlx_project
        = install_dependencies(&dlx_project.project_cwd, vec![resolution], options.quiet).await?;

    let bin
        = find_binary(&dlx_project, &binary_name, options.fallback_binary)?;

    let run_cwd = match options.run_cwd {
        Some(cwd) => cwd,
        None => Path::current_dir()?,
    };

    run_binary(&dlx_project, bin, options.args, run_cwd).await
}
