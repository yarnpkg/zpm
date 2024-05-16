use clap::{Parser, Subcommand};
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
    Node {
        #[arg(short, long)]
        eval: Option<String>,
    },
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

        Some(Commands::Node {eval}) => {
            let project
                = project::Project::new(None)?;

            let mut command = Command::new("node");

            if let Some(pnp_path) = project.pnp_path().if_exists() {
                command.env("NODE_OPTIONS", format!("--require {}", pnp_path.to_string()));
            }

            if let Some(eval) = eval {
                command.arg("-e").arg(eval);
            }

            command.status().await.unwrap();
        }
    }

    Ok(())
}
