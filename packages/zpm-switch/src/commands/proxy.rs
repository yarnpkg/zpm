use std::{io::IsTerminal, process::{ExitStatus, Stdio}};

use clipanion::cli;
use clipanion::core::{Completion, CompletionContext};
use zpm_utils::{DataType, Note, ToFileString};

use crate::{config::validate_yarn_version, cwd::{get_fake_cwd, get_final_cwd}, errors::Error, install::install_package_manager, ipc::YARNSW_PATH_ENV, links::{LinkTarget, get_link, unset_link}, manifest::{LocalPackageManagerReference, PackageManagerField, PackageManagerReference, find_closest_package_manager}, yarn::get_default_yarn_version, yarn_enums::ReleaseLine};

use super::switch::explicit::ExplicitCommand;

pub async fn resolve_proxy_reference() -> Result<PackageManagerReference, Error> {
    let lookup_path
        = get_final_cwd()?;

    let mut find_result
        = find_closest_package_manager(&lookup_path)?;

    if let Some(detected_root_path) = find_result.detected_root_path {
        std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());

        if let Some(link) = get_link(&detected_root_path)? {
            match link.link_target {
                LinkTarget::Local {bin_path} => {
                    if std::io::stdout().is_terminal() {
                        Note::Warning(format!("This command runs a local development version of Yarn. Use {} to remove the link.", DataType::Code.colorize("yarn switch unlink"))).print();
                        println!();
                    }

                    find_result.detected_package_manager
                        = Some(PackageManagerField::new_yarn(LocalPackageManagerReference {path: bin_path}.into()));
                },

                LinkTarget::Migration => {
                    if let Some(migration) = find_result.detected_package_manager_migration {
                        std::env::set_var("YARN_ENABLE_MIGRATION_MODE", "1");

                        if std::io::stdout().is_terminal() {
                            Note::Warning(format!("This command runs the checked-in migration version of Yarn. Use {} to opt-out of the migration.", DataType::Code.colorize("yarn switch unlink"))).print();
                            println!();
                        }

                        find_result.detected_package_manager
                            = Some(PackageManagerField::new_yarn(migration.into_reference("yarn")?));
                    } else {
                        unset_link(&detected_root_path)?;
                    }
                },
            };
        }
    }

    let reference = match find_result.detected_package_manager {
        Some(package_manager) => package_manager.into_reference("yarn"),
        None => get_default_yarn_version(Some(ReleaseLine::Classic)).await,
    }?;

    Ok(reference)
}

pub fn proxy_completer(ctx: &CompletionContext) -> Vec<Completion> {
    let handle = tokio::runtime::Handle::current();

    tokio::task::block_in_place(|| {
        handle.block_on(async {
            proxy_completer_async(ctx).await
        })
    })
}

async fn proxy_completer_async(ctx: &CompletionContext<'_>) -> Vec<Completion> {
    let Ok(reference) = resolve_proxy_reference().await else {
        return vec![];
    };

    let mut binary = match &reference {
        PackageManagerReference::Version(params) => {
            let Ok(()) = validate_yarn_version(&params.version) else {
                return vec![];
            };

            let Ok(cmd) = install_package_manager(params).await else {
                return vec![];
            };
            cmd
        },
        PackageManagerReference::Local(params) => {
            std::process::Command::new(params.path.to_path_buf())
        },
    };

    let mut args = vec![String::from("--clipanion-complete")];

    let user_args: Vec<&str> = ctx.args_before.iter()
        .copied()
        .chain(std::iter::once(ctx.current))
        .collect();

    let index = ctx.args_before.len();
    args.push(index.to_string());
    args.push(String::from("--"));
    args.extend(user_args.into_iter().map(String::from));

    if let Some(cwd) = get_fake_cwd() {
        binary.args(&[cwd.to_file_string()]);
    }

    binary.args(&args);
    binary.stdout(Stdio::piped());
    binary.stderr(Stdio::null());

    if let Ok(switch_path) = std::env::current_exe() {
        binary.env(YARNSW_PATH_ENV, switch_path);
    }

    let Ok(output) = binary.output() else {
        return vec![];
    };

    let Ok(stdout) = String::from_utf8(output.stdout) else {
        return vec![];
    };

    stdout.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let mut parts = line.splitn(2, '\t');
        let text = parts.next()?.to_string();
        let description = parts.next().map(|s| s.to_string());

        let mut completion = Completion::new(text);
        if let Some(desc) = description {
            completion = completion.with_description(desc);
        }
        Some(completion)
    }).collect()
}

/// Run the Yarn version selected for the current project
///
/// Yarn Switch resolves the nearest `packageManager` field, applies any local development link or migration link, installs the selected Yarn binary
/// if needed, then forwards the command to it.
///
#[cli::command(default, proxy)]
#[cli::category("General commands")]
#[derive(Debug)]
pub struct ProxyCommand {
    /// Yarn command and arguments to forward
    #[cli::positional(completer = proxy_completer)]
    args: Vec<String>,
}

impl ProxyCommand {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let reference
            = resolve_proxy_reference().await?;

        let mut args
            = self.args.clone();

        if let Some(cwd) = get_fake_cwd() {
            args.insert(0, cwd.to_file_string());
        }

        ExplicitCommand::run(&reference, &args).await
    }
}
