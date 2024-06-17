use std::{ffi::OsString, process};

use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::process::Command;

use crate::{error::Error, install::{InstallContext, InstallManager}, linker, project};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Install {},

    #[command(external_subcommand)]
    External(Vec<String>),
}

const CJS_LOADER_MATCHER: Lazy<Regex> = Lazy::new(|| regex::Regex::new(r"\s*--require\s+\S*\.pnp\.c?js\s*").unwrap());
const ESM_LOADER_MATCHER: Lazy<Regex> = Lazy::new(|| regex::Regex::new(r"\s*--experimental-loader\s+\S*\.pnp\.loader\.mjs\s*").unwrap());

fn setup_script_environment(cmd: &mut Command, project: &project::Project) {
    let node_options = std::env::var("NODE_OPTIONS")
        .unwrap_or_else(|_| "".to_string());

    let node_options = CJS_LOADER_MATCHER.replace_all(&node_options, " ");
    let node_options = ESM_LOADER_MATCHER.replace_all(&node_options, " ");

    let mut node_options = node_options.trim().to_string();

    if let Some(pnp_path) = project.pnp_path().if_exists() {
        node_options = format!("{} --require {}", node_options, pnp_path.to_string());
    }

    if let Some(pnp_loader_path) = project.pnp_loader_path().if_exists() {
        node_options = format!("{} --experimental-loader {}", node_options, pnp_loader_path.to_string());
    }

    if node_options.len() > 0 {
        cmd.env("NODE_OPTIONS", node_options);
    }
}

pub async fn run_cli() -> Result<(), Error> {
    let cli = Cli::parse();

    match &cli.command {
        None | Some(Commands::Install {}) => {
            let project
                = project::Project::new(None)?;

            let package_cache
                = project.package_cache();

            let install_context = InstallContext::default()
                .with_package_cache(Some(&package_cache))
                .with_project(Some(&project));

            let install = InstallManager::default()
                .with_context(install_context)
                .with_lockfile(project.lockfile()?)
                .with_roots_iter(project.workspaces.values().map(|w| w.descriptor()))
                .run().await?;

            project
                .write_lockfile(&install.lockfile)?;

            linker::link_project(&project, &install)
                .await?;
        }

        Some(Commands::External(args)) => {
            let project
                = project::Project::new(None)?;

            match args[0].as_str() {
                "node" => {
                    let mut command = Command::new("node");

                    command.args(&args[1..]);

                    setup_script_environment(&mut command, &project);

                    let exit_status
                        = command.status().await.unwrap();

                    let exit_code
                        = exit_status.code().unwrap_or(1);

                    process::exit(exit_code);
                },

                _ => {
                    panic!("Unknown external subcommand: {}", args[0]);
                }
            };
        }
    }

    Ok(())
}
