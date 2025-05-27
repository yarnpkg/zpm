use std::{fs::Permissions, os::unix::fs::PermissionsExt, process::{Command, ExitCode, ExitStatus, Stdio}, str::FromStr};

use clipanion::{cli, prelude::*};
use install::install_package_manager;
use manifest::{find_closest_package_manager, validate_package_manager, PackageManagerField, PackageManagerReference};
use yarn::get_default_yarn_version;
use zpm_utils::{DataType, ExplicitPath, Path};

mod errors;
mod http;
mod install;
mod manifest;
mod yarn;

use crate::errors::Error;

#[cli::command(default, proxy)]
#[derive(Debug)]
pub struct ProxyCommand {
    #[cli::positional(is_prefix = true)]
    cwd: Option<ExplicitPath>,

    args: Vec<String>,
}

impl ProxyCommand {
    async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path = match &self.cwd {
            Some(cwd) => Path::current_dir()?.with_join(&cwd.raw_path.path),
            None => Path::current_dir()?,
        };

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_string());
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => validate_package_manager(package_manager, "yarn"),
            None => get_default_yarn_version(Some("classic")).await,
        }?;

        ExplicitCommand::run(&reference, &self.args).await
    }
}

#[cli::command(proxy)]
#[cli::path("init")]
#[derive(Debug)]
pub struct InitCommand {
    #[cli::positional(is_prefix = true)]
    cwd: Option<ExplicitPath>,

    args: Vec<String>,
}

impl InitCommand {
    async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path = match &self.cwd {
            Some(cwd) => Path::current_dir()?.with_join(&cwd.raw_path.path),
            None => Path::current_dir()?,
        };

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_string());
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => validate_package_manager(package_manager, "yarn"),
            None => get_default_yarn_version(None).await,
        }?;

        let mut args = vec!["init".to_string()];
        args.extend_from_slice(&self.args);

        ExplicitCommand::run(&reference, &args).await
    }
}

fn insert_rc_line(rc_path: Path, line: String) {
    let rc_content = rc_path
        .fs_read_text_prealloc();

    let initial_rc_content = match rc_content {
        Ok(content) => {
            content
        },

        Err(e) if e.io_kind() == Some(std::io::ErrorKind::NotFound) => {
            String::new()
        },

        Err(_) => {
            println!("Failed to read rc file");
            return;
        },
    };

    if initial_rc_content.contains(&line) {
        println!("Line already exists");
        return;
    }

    let mut rc_content
        = initial_rc_content.clone();

    let header
        = "# BEGIN YARN SWITCH MANAGED BLOCK\n";
    let footer
        = "# END YARN SWITCH MANAGED BLOCK\n";

    let header_position = rc_content
        .find(header);
    let footer_position = rc_content
        .find(footer);

    let final_string
        = header.to_string() + &line + &footer;

    match (header_position, footer_position) {
        (Some(header_position), Some(footer_position)) => {
            rc_content.replace_range(header_position..footer_position + footer.len(), &final_string);
        },

        (Some(header_position), None) => {
            rc_content.replace_range(header_position..header_position + header.len(), &final_string);
        },

        (None, Some(footer_position)) => {
            rc_content.replace_range(footer_position..footer_position + footer.len(), &final_string);
        },

        (None, None) => {
            if rc_content.is_empty() || rc_content.ends_with("\n\n") {
                // All good, we can insert the line right away!
            } else if rc_content.ends_with("\n") {
                rc_content.push('\n');
            } else {
                rc_content.push_str("\n\n");
            }

            rc_content.push_str(&final_string);
        },
    }

    let _ = rc_path
        .fs_change(rc_content, Permissions::from_mode(0o644));

    println!("We updated the {} file for you; please restart your shell to apply the changes.", DataType::Path.colorize(rc_path.as_str()));
}

#[cli::command]
#[cli::path("switch", "postinstall")]
#[derive(Debug)]
pub struct PostinstallCommand {
    #[cli::option("-H,--home-dir")]
    home_dir: Option<Path>,
}

impl PostinstallCommand {
    async fn execute(&self) {
        let bin_dir = Path::current_exe()
            .ok()
            .and_then(|p| p.dirname());

        let Some(bin_dir) = bin_dir else {
            return;
        };

        println!(
            "Yarn {} was successfully installed to {}.",
            DataType::Code.colorize(self.cli_environment.info.version.as_str()),
            DataType::Path.colorize(bin_dir.as_str())
        );

        let Ok(shell) = std::env::var("SHELL") else {
            return;
        };

        let Ok(shell_path) = Path::from_str(&shell) else {
            return;
        };

        let Some(shell_name) = shell_path.basename() else {
            return;
        };

        let Some(home) = self.home_dir.clone().or_else(|| Path::home_dir().unwrap_or_default()) else {
            return;
        };

        match shell_name {
            "bash" => {
                let insert_line
                    = format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_string());

                let bashrc_path = home
                    .with_join_str(".bashrc");

                insert_rc_line(bashrc_path, insert_line);
            },

            "zsh" => {
                let insert_line
                    = format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_string());

                let zshrc_path = home
                    .with_join_str(".zshrc");

                insert_rc_line(zshrc_path, insert_line);
            },

            _ => {
                println!("We couldn't find a supported shell to update ({}). Please manually add the following line to your shell configuration file:", DataType::Code.colorize(&format!("SHELL={}", shell)));
                println!("{}", DataType::Code.colorize(&format!("export PATH=\"{}:$PATH\"", bin_dir)));
            },
        }
    }
}

#[cli::command(proxy)]
#[cli::path("switch")]
#[derive(Debug)]
pub struct ExplicitCommand {
    #[cli::positional(is_prefix = true)]
    cwd: Option<ExplicitPath>,

    package_manager: PackageManagerField,
    args: Vec<String>,
}

impl ExplicitCommand {
    async fn run(reference: &PackageManagerReference, args: &[String]) -> Result<ExitStatus, Error> {
        let mut binary = match reference {
            PackageManagerReference::Version(params)
                => install_package_manager(params).await?,
    
            PackageManagerReference::Local(params)
                => Command::new(params.path.to_path_buf()),
        };

        binary.stdout(Stdio::inherit());
        binary.args(args);
    
        Ok(binary.status()?)
    }

    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path = match &self.cwd {
            Some(cwd) => Path::current_dir()?.with_join(&cwd.raw_path.path),
            None => Path::current_dir()?,
        };

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_string());
        }

        ExplicitCommand::run(&self.package_manager.reference, &self.args).await
    }
}

clipanion::program_async!(SwitchExecCli, [
    PostinstallCommand,
    ExplicitCommand,
    ProxyCommand,
    InitCommand,
]);

#[tokio::main()]
async fn main() -> ExitCode {
    let self_path = Path::current_exe()
        .unwrap()
        .to_string();

    std::env::set_var("YARNSW_EXEC_PATH", self_path);

    SwitchExecCli::run_default().await
}
