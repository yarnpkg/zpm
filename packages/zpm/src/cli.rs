use clap::{Parser, Subcommand};
use tokio::process::Command;

use crate::{error::Error, linker, project};

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
            project::persist_lockfile().await?;
            linker::link_project().await?;
        }

        Some(Commands::Node {eval}) => {
            let mut command = Command::new("node");

            if let Some(pnp_path) = linker::pnp_path() {
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
