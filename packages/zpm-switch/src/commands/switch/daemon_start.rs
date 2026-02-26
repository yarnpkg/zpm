use std::process::Stdio;

use clipanion::cli;
use zpm_utils::{DataType, ToFileString, ToHumanString};

use crate::{
    cwd::get_final_cwd,
    daemons::{self, DaemonEntry},
    errors::Error,
    install::install_package_manager,
    manifest::{find_closest_package_manager, PackageManagerReference},
    yarn::get_default_yarn_version,
    yarn_enums::ReleaseLine,
};

/// Start a daemon for the current project
#[cli::command]
#[cli::path("switch", "daemon")]
#[cli::category("Daemon management")]
#[derive(Debug)]
pub struct DaemonStartCommand {
    #[cli::option("--start")]
    _start: bool,
}

impl DaemonStartCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let project_cwd = get_final_cwd()?;

        let find_result = find_closest_package_manager(&project_cwd)?;

        let detected_root = find_result
            .detected_root_path
            .ok_or(Error::NoProjectFound)?;

        // Check if a daemon is already running for this project
        if let Some(existing) = daemons::get_daemon(&detected_root)? {
            if daemons::is_process_alive(existing.pid) {
                println!(
                    "{} A daemon is already running for this project (PID: {})",
                    DataType::Warning.colorize("!"),
                    existing.pid
                );
                return Ok(());
            }
            // Clean up stale entry
            daemons::unregister_daemon(&detected_root)?;
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => package_manager.into_reference("yarn"),
            None => get_default_yarn_version(Some(ReleaseLine::Classic)).await,
        }?;

        let version = match &reference {
            PackageManagerReference::Version(v) => v.version.clone(),
            PackageManagerReference::Local(_) => {
                return Err(Error::DaemonNotSupportedForLocalVersions);
            }
        };

        // Get the yarn binary
        let PackageManagerReference::Version(version_ref) = &reference else {
            unreachable!()
        };

        let mut binary = install_package_manager(version_ref).await?;

        // Spawn the daemon process in the background
        binary
            .arg("daemon")
            .current_dir(detected_root.to_file_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Create a new process group so the daemon outlives the parent
            binary.process_group(0);
        }

        let child = binary
            .spawn()
            .map_err(|e| Error::FailedToStartDaemon(std::sync::Arc::new(e)))?;

        let pid = child.id();

        // Register the daemon
        let entry = DaemonEntry {
            project_cwd: detected_root.clone(),
            yarn_version: version.clone(),
            pid,
        };

        daemons::register_daemon(&entry)?;

        println!(
            "{} Started daemon for {} (Yarn {}, PID: {})",
            DataType::Success.colorize("✓"),
            detected_root.to_print_string(),
            version.to_print_string(),
            pid
        );

        Ok(())
    }
}
