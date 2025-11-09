use clipanion::cli;
use zpm_parsers::{document::Document, JsonDocument, Value};
use zpm_primitives::Ident;
use zpm_utils::{IoResultExt, Path, ToFileString};

use crate::{
    commands::dlx,
    descriptor_loose::{self, LooseDescriptor},
    error::Error,
    install::InstallContext,
    manifest::Manifest,
    project::{Project, RunInstallOptions},
    script::ScriptEnvironment,
};

/// This command will setup a new package in your local directory.
///
/// If the `-p,--private` or `-w,--workspace` options are set, the package will be private by default.
///
/// If the `-w,--workspace` option is set, the package will be configured to accept a set of workspaces in the `packages/` directory.
///
/// If the `-i,--install` option is given a value, Yarn will first download it using `yarn set version` and only then forward the init call to the
/// newly downloaded bundle. Without arguments, the downloaded bundle will be latest.
///
/// The initial settings of the manifest can be changed by using the `initScope` and `initFields` configuration values. Additionally, Yarn will
/// generate an `.editorconfig` file whose rules can be altered via `initEditorConfig`, and will initialize a Git repository in the current directory.
///
#[cli::command(proxy)]
#[cli::path("init")]
#[cli::category("Project management")]
pub struct InitWithTemplate {
    /// Set the package to be private
    #[cli::option("-p,--private")]
    private: Option<bool>,

    /// Set the package to be a workspace
    #[cli::option("-w,--workspace", default = false)]
    workspace: bool,

    /// Set the name of the package
    #[cli::option("-n,--name")]
    name: Option<String>,

    /// The template to use for the package
    template: LooseDescriptor,

    /// The arguments to pass to the template
    args: Vec<String>,

    #[cli::option("-2", default = false)]
    usev2: bool,

    #[cli::option("-y,--yes", default = false)]
    yes: bool,
}

impl InitWithTemplate {
    pub async fn execute(&self) -> Result<(), Error> {
        let init_cwd
            = Path::current_dir()?;

        let params = InitParams {
            private: self.private,
            workspace: self.workspace,
            name: self.name.clone(),
        };

        let mut project
            = init_project(&init_cwd, params).await?;

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind: zpm_semver::RangeKind::Exact,
            resolve_tags: true,
        };

        let package_cache
            = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let template
            = self.template.resolve(&install_context, &resolve_options).await?;

        project.run_install(RunInstallOptions {
            ..Default::default()
        }).await?;

        println!();

        let dlx_project
            = dlx::setup_project().await?;
        let dlx_project
            = dlx::install_dependencies(&dlx_project.project_cwd, vec![template.clone()], false).await?;
        let bin
            = dlx::find_binary(&dlx_project, template.ident.name(), true)?;

        println!();
        dlx::run_binary(&dlx_project, bin, self.args.clone(), init_cwd.clone()).await?;

        Ok(())
    }
}

#[cli::command]
#[cli::path("init")]
#[derive(Debug)]
pub struct Init {
    #[cli::option("-p,--private")]
    private: Option<bool>,

    #[cli::option("-w,--workspace", default = false)]
    workspace: bool,

    #[cli::option("-n,--name")]
    name: Option<String>,

    // Hidden legacy options
    #[cli::option("-2", default = false)]
    usev2: bool,

    #[cli::option("-y,--yes", default = false)]
    yes: bool,
}

impl Init {
    pub async fn execute(&self) -> Result<(), Error> {
        let init_cwd
            = Path::current_dir()?;

        let params = InitParams {
            private: self.private,
            workspace: self.workspace,
            name: self.name.clone(),
        };

        let mut project
            = init_project(&init_cwd, params).await?;

        project.run_install(RunInstallOptions {
            ..Default::default()
        }).await?;

        Ok(())
    }
}

pub struct InitParams {
    private: Option<bool>,
    workspace: bool,
    name: Option<String>,
}

pub async fn init_project(init_cwd: &Path, params: InitParams) -> Result<Project, Error> {
    let existing_project
        = Project::find_closest_project(init_cwd.clone()).ok();

    let manifest_path
        = init_cwd.with_join_str("package.json");
    let manifest_content
        = manifest_path.fs_read_prealloc()
            .ok_missing()?
            .unwrap_or_else(|| b"{}".to_vec());

    let mut document
        = JsonDocument::new(manifest_content)?;

    if !manifest_path.fs_exists() {
        let init_name = params.name.as_ref()
            .map(|n| Ident::new(n))
            .unwrap_or_else(|| Ident::new(init_cwd.basename().unwrap_or("package")));

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["name".to_string()]),
            Value::String(init_name.to_file_string()),
        )?;
    }

    if let Some(version) = option_env!("INFRA_VERSION") {
        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["packageManager".to_string()]),
            Value::String(format!("yarn@{version}")),
        )?;
    }

    if let Some(private) = params.private {
        let private_field = match private {
            true => Value::Bool(true),
            false => Value::Undefined,
        };

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["private".to_string()]),
            private_field,
        )?;
    }

    // TODO: --workspace should create a workspace child, not
    // define a workspace root (we should have a different flag
    // for that).
    if params.workspace {
        let packages_dir = init_cwd
            .with_join_str("packages");

        packages_dir
            .fs_create_dir_all()?;

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["workspaces".to_string()]),
            Value::Array(vec![
                Value::String("packages/*".to_string()),
            ]),
        )?;
    }

    manifest_path
        .fs_change(&document.input, false)?;

    let manifest_json
        = String::from_utf8_lossy(&document.input);
    let manifest: Manifest
        = JsonDocument::hydrate_from_str(&manifest_json)?;

    let mut changed_paths = vec![
        manifest_path.clone(),
    ];

    let readme_path
        = init_cwd.with_join_str("README.md");

    if !readme_path.fs_exists() {
        if let Some(name) = manifest.name.as_ref() {
            let readme_content
                = format!("# {}\n", name.as_str());

            readme_path
                .fs_write_text(&readme_content)?;

            changed_paths.push(readme_path.clone());
        }
    }

    // Only create lockfile and other files if we're in the project root
    let is_project_root = existing_project
        .as_ref()
        .map(|(project_cwd, _)| project_cwd == init_cwd)
        .unwrap_or(true);

    if is_project_root {
        let lockfile_path = init_cwd
            .with_join_str("yarn.lock");

        if !lockfile_path.fs_exists() {
            lockfile_path
                .fs_write_text("")?;

            changed_paths.push(
                lockfile_path.clone(),
            );
        }

        let gitignore_path = init_cwd
            .with_join_str(".gitignore");

        if !gitignore_path.fs_exists() {
            let gitignore_content = vec![
                "node_modules\n",
            ];

            gitignore_path
                .fs_write_text(&gitignore_content.join(""))?;

            changed_paths.push(
                gitignore_path.clone(),
            );
        }

        let gitattributes_path = init_cwd
            .with_join_str(".gitattributes");

        if !gitattributes_path.fs_exists() {
            let gitattributes_content = vec![
                "/.yarn/**         linguist-vendored\n",
                "/.pnp.*           linguist-generated binary\n",
            ];

            gitattributes_path
                .fs_write_text(&gitattributes_content.join(""))?;

            changed_paths.push(
                gitattributes_path.clone(),
            );
        }

        let editorconfig_path = init_cwd
            .with_join_str(".editorconfig");

        if !editorconfig_path.fs_exists() {
            let editorconfig_content = vec![
                "root = true\n",
                "\n",
                "[*]\n",
                "charset = utf-8\n",
                "end_of_line = lf\n",
                "indent_size = 2\n",
                "indent_style = space\n",
                "insert_final_newline = true\n",
                "\n",
                "[*.rs]\n",
                "indent_size = 4\n",
            ];

            editorconfig_path
                .fs_write_text(&editorconfig_content.join(""))?;

            changed_paths.push(
                editorconfig_path.clone(),
            );
        }

        let git_path = init_cwd
            .with_join_str(".git");

        if !git_path.fs_exists() {
            let init = ScriptEnvironment::new()?
                .run_exec("git", ["init"])
                .await?
                .ok();

            if init.is_ok() {
                let mut add_args = vec!["add", "--"];

                let changed_path_strings = changed_paths.iter()
                    .map(|path| path.to_file_string())
                    .collect::<Vec<_>>();

                add_args.extend(changed_path_strings.iter().map(|s| s.as_str()));

                ScriptEnvironment::new()?
                    .run_exec("git", add_args)
                    .await?
                    .ok()?;

                ScriptEnvironment::new()?
                    .run_exec("git", ["commit", "--allow-empty", "-m", "First commit"])
                    .await?
                    .ok()?;
            }
        }
    }

    let project
        = Project::new(Some(init_cwd.clone())).await?;

    Ok(project)
}
