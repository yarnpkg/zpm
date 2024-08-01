use std::{ffi::OsString, process};

use clap::{Parser, Subcommand};

use crate::{error::Error, install::{InstallContext, InstallManager}, linker, project, script::ScriptEnvironment};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Install {},

    Exec {
        script: String,
    },

    #[command(external_subcommand)]
    External(Vec<String>),
}

pub async fn run_cli() -> Result<(), Error> {
    let mut argv = std::env::args_os()
        .map(OsString::into_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    if let Some(argv1) = argv.get(1) {
        if argv1.contains("/") {
            let path = std::path::PathBuf::from(argv1);

            std::env::set_current_dir(&path)
                .map_err(|_| Error::FailedToChangeCwd)?;

            argv.remove(0);
        }
    }

    let cli = Cli::parse_from(argv.iter());

    match &cli.command {
        None | Some(Commands::Install {}) => {
            let mut project
                = project::Project::new(None)?;

            let package_cache
                = project.package_cache();

            let install_context = InstallContext::default()
                .with_package_cache(Some(&package_cache))
                .with_project(Some(&project));

            let mut lockfile = project.lockfile()?;
            lockfile.forget_transient_resolutions();

            InstallManager::default()
                .with_context(install_context)
                .with_lockfile(lockfile)
                .with_roots_iter(project.workspaces.values().map(|w| w.descriptor()))
                .resolve_and_fetch().await?
                .finalize(&mut project).await?;
        }

        Some(Commands::Exec {script}) => {
            let mut project
                = project::Project::new(None)?;

            project
                .import_install_state()?;

            let exit_code = ScriptEnvironment::new()
                .with_project(&project)
                .run_script(&script)
                .await;

            process::exit(exit_code);
        }

        Some(Commands::External(args)) => {
            let mut project
                = project::Project::new(None)?;

            match args[0].as_str() {
                "node" => {
                    let exit_code = ScriptEnvironment::new()
                        .with_project(&project)
                        .run_exec("node", &args[1..])
                        .await;

                    process::exit(exit_code);
                },

                "run" => {
                    project
                        .import_install_state()?;

                    let (locator, script)
                        = project.find_script(args[1].as_str())?;

                    let exit_code = ScriptEnvironment::new()
                        .with_project(&project)
                        .with_package(&project, &locator)?
                        .run_script(&script)
                        .await;

                    process::exit(exit_code);
                },

                _ => {
                    project
                        .import_install_state()?;

                    let maybe_binary
                        = project.find_binary(args[0].as_str());

                    if let Ok(binary_path) = maybe_binary {
                        let exit_code = ScriptEnvironment::new()
                            .with_project(&project)
                            .with_package(&project, &project.active_package()?)?
                            .run_exec(&binary_path.to_string(), &args[1..])
                            .await;

                        process::exit(exit_code);
                    } else if let Err(Error::BinaryNotFound(_)) = maybe_binary {
                        let (locator, script)
                            = project.find_script(args[0].as_str())?;

                        let exit_code = ScriptEnvironment::new()
                            .with_project(&project)
                            .with_package(&project, &locator)?
                            .run_script(&script)
                            .await;

                        process::exit(exit_code);
                    } else {
                        maybe_binary?;
                    }
                }
            };
        }
    }

    Ok(())
}
