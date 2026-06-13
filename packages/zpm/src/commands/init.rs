use clipanion::cli;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_primitives::Ident;
use zpm_utils::{IoResultExt, Path, ToFileString};
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::{
    commands::dlx::{install_and_run_single, InstallAndRunOptions},
    descriptor_loose::{self, LooseDescriptor},
    error::Error,
    install::InstallContext,
    manifest::Manifest,
    project::{Project, RunInstallOptions},
    script::ScriptEnvironment,
};

/// Initialize a package in the current directory
///
/// This command creates a manifest and supporting project files in the current directory.
///
/// If the `-p,--private` or `-w,--workspace` options are set, the package will be private by default.
///
/// If the `-w,--workspace` option is set, the package will be configured to accept a set of workspaces in the `packages/` directory.
///
/// When a template is provided, Yarn initializes the project, installs the template in a temporary context, and runs its binary from the new project.
///
#[cli::command(proxy)]
#[cli::path("init")]
#[cli::category("Project management")]
pub struct InitWithTemplate {
    /// Mark the new package as private
    #[cli::option("-p,--private")]
    private: Option<bool>,

    /// Configure the package as a workspace root
    #[cli::option("-w,--workspace", default = false)]
    workspace: bool,

    /// Package name to write to the manifest
    #[cli::option("-n,--name")]
    name: Option<String>,

    /// Template package to install and run
    template: LooseDescriptor,

    /// Arguments to pass to the template binary
    args: Vec<String>,

    /// Hidden legacy Yarn 1 compatibility flag
    #[cli::option("-2", default = false)]
    usev2: bool,

    /// Hidden legacy Yarn 1 compatibility flag
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
            version: self.cli_environment.info.version.clone(),
        };

        let mut project
            = init_project(&init_cwd, params).await?;

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind: zpm_semver::RangeKind::Exact,
            resolve_tags: true,
            allow_reuse: true,
        };

        let package_cache
            = project.package_cache()?;

        let install_context
            = InstallContext::default()
                .with_package_cache(Some(&package_cache))
                .with_project(Some(&project));

        let template
            = self.template.resolve(&install_context, &resolve_options).await?;

        let enforced_resolutions
            = vec![template.clone()].into_iter()
                .filter_map(|resolution| resolution.locator.map(|locator| (resolution.descriptor, Some(locator))))
                .collect();

        project.run_install(RunInstallOptions {
            enforced_resolutions,
            ..Default::default()
        }).await?;

        println!();

        install_and_run_single(self.template.clone(), InstallAndRunOptions {
            args: self.args.clone(),
            run_cwd: Some(init_cwd.clone()),
            fallback_binary: true,
            ..Default::default()
        }).await?;

        Ok(())
    }
}

/// Initialize a package in the current directory
///
/// This command creates a manifest and supporting project files in the current directory.
///
#[cli::command]
#[cli::path("init")]
#[derive(Debug)]
pub struct Init {
    /// Mark the new package as private
    #[cli::option("-p,--private")]
    private: Option<bool>,

    /// Configure the package as a workspace root
    #[cli::option("-w,--workspace", default = false)]
    workspace: bool,

    /// Package name to write to the manifest
    #[cli::option("-n,--name")]
    name: Option<String>,

    // Hidden legacy options
    /// Hidden legacy Yarn 1 compatibility flag
    #[cli::option("-2", default = false)]
    usev2: bool,

    /// Hidden legacy Yarn 1 compatibility flag
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
            version: self.cli_environment.info.version.clone(),
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
    version: String,
}

#[derive(Deserialize)]
struct YarnRcInit {
    #[serde(default, rename = "initFields")]
    init_fields: BTreeMap<String, serde_json::Value>,
}

fn apply_init_fields(document: &mut JsonDocument, init_cwd: &Path) -> Result<(), Error> {
    let rc_filename = crate::commands::rc_helpers::rc_filename();

    // Walk every rc on the way up from `init_cwd`, plus the home rc,
    // so the manifest reflects the same cascade `Configuration::load`
    // would produce. Stopping at the first hit would silently drop an
    // `initFields` set higher up the chain.
    let mut rc_paths: Vec<Path> = Vec::new();
    let mut current: Option<Path> = Some(init_cwd.clone());
    while let Some(dir) = current {
        let rc_path = dir.with_join_str(&rc_filename);
        if rc_path.fs_exists() {
            rc_paths.push(rc_path);
        }
        current = dir.dirname();
    }

    if let Ok(home_rc) = crate::commands::rc_helpers::home_rc_path() {
        if home_rc.fs_exists() && !rc_paths.iter().any(|p| p == &home_rc) {
            rc_paths.push(home_rc);
        }
    }

    // Reverse to apply outermost first so inner rcs override.
    for rc_path in rc_paths.into_iter().rev() {
        let Ok(text) = rc_path.fs_read_text() else { continue };
        let Ok(parsed) = zpm_parsers::YamlDocument::hydrate_from_str::<YarnRcInit>(&text) else { continue };

        for (key, value) in parsed.init_fields {
            let parser_value = json_to_parser_value(&value);
            document.set_path(
                &zpm_parsers::Path::from_segments(vec![key]),
                parser_value,
            )?;
        }
    }

    Ok(())
}

fn json_to_parser_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => Value::Number(n.to_string()),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(items) => Value::Array(items.iter().map(json_to_parser_value).collect()),
        serde_json::Value::Object(entries) => Value::Object(
            entries.iter()
                .map(|(k, v)| (k.clone(), json_to_parser_value(v)))
                .collect(),
        ),
    }
}

pub async fn init_project(init_cwd: &Path, params: InitParams) -> Result<Project, Error> {
    let existing_project
        = Project::find_closest_project(init_cwd.clone()).ok();

    let manifest_path
        = init_cwd.with_join_str("package.json");
    let manifest_content
        = manifest_path.fs_read_prealloc()
            .ok_missing()?
            .unwrap_or_else(|| b"{}\n".to_vec());

    let mut document
        = JsonDocument::new(manifest_content)?;

    if !manifest_path.fs_exists() {
        let init_name
            = params.name.as_ref()
                .map(|n| Ident::new(n))
                .unwrap_or_else(|| Ident::new(init_cwd.basename().unwrap_or("package")));

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["name".to_string()]),
            Value::String(init_name.to_file_string()),
        )?;

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["type".to_string()]),
            Value::String("module".to_file_string()),
        )?;
    }

    document.set_path(
        &zpm_parsers::Path::from_segments(vec!["packageManager".to_string()]),
        Value::String(format!("yarn@{}", params.version)),
    )?;

    apply_init_fields(&mut document, init_cwd)?;

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
        let packages_dir
            = init_cwd
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
        = init_cwd
            .with_join_str("README.md");

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
    let is_project_root
        = existing_project
            .as_ref()
            .map(|(project_cwd, _)| project_cwd == init_cwd)
            .unwrap_or(true);

    if is_project_root {
        let lockfile_path
            = init_cwd
                .with_join_str("yarn.lock");

        if !lockfile_path.fs_exists() {
            lockfile_path
                .fs_write_text("")?;

            changed_paths.push(
                lockfile_path.clone(),
            );
        }

        let gitignore_path
            = init_cwd
                .with_join_str(".gitignore");

        if !gitignore_path.fs_exists() {
            let gitignore_content = [
                "node_modules\n",
            ];

            gitignore_path
                .fs_write_text(&gitignore_content.join(""))?;

            changed_paths.push(
                gitignore_path.clone(),
            );
        }

        let gitattributes_path
            = init_cwd
                .with_join_str(".gitattributes");

        if !gitattributes_path.fs_exists() {
            let gitattributes_content = [
                "/.yarn/**         linguist-vendored\n",
                "/.pnp.*           linguist-generated binary\n",
            ];

            gitattributes_path
                .fs_write_text(&gitattributes_content.join(""))?;

            changed_paths.push(
                gitattributes_path.clone(),
            );
        }

        let editorconfig_path
            = init_cwd
                .with_join_str(".editorconfig");

        if !editorconfig_path.fs_exists() {
            let editorconfig_content = [
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
