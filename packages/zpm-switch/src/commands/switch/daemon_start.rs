use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use clipanion::cli;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use zpm_semver::Version;
use zpm_utils::{DataType, Path, ToFileString, ToHumanString};

use crate::{
    cwd::get_final_cwd,
    daemons::{self, DaemonEntry},
    errors::Error,
    install::install_package_manager,
    ipc::{socket_path, DaemonRequest, DaemonResponse},
    links::{get_link, LinkTarget},
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

        // Check if there's a linked binary for this project
        if let Some(link) = get_link(&detected_root)? {
            if let LinkTarget::Local { bin_path } = link.link_target {
                return self.start_with_binary(&detected_root, &bin_path, "local").await;
            }
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
        self.start_with_command(&detected_root, &mut binary, &version.to_file_string()).await
    }

    async fn start_with_binary(
        &self,
        detected_root: &zpm_utils::Path,
        bin_path: &zpm_utils::Path,
        version_label: &str,
    ) -> Result<(), Error> {
        let mut binary = Command::new(bin_path.to_path_buf());
        self.start_with_command(detected_root, &mut binary, version_label).await
    }

    async fn start_with_command(
        &self,
        detected_root: &zpm_utils::Path,
        binary: &mut Command,
        version_label: &str,
    ) -> Result<(), Error> {
        // Spawn the daemon process in the background
        binary
            .arg("debug")
            .arg("daemon")
            .current_dir(detected_root.to_file_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Ensure HOME is passed to daemon (important for socket path)
        if let Ok(home) = std::env::var("HOME") {
            binary.env("HOME", home);
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            binary.env("USERPROFILE", userprofile);
        }

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
            yarn_version: version_label.parse().unwrap_or_else(|_| Version::new()),
            pid,
        };

        daemons::register_daemon(&entry)?;

        // Wait for daemon to be ready
        self.wait_for_ready(detected_root).await?;

        println!(
            "{} Started daemon for {} (Yarn {}, PID: {})",
            DataType::Success.colorize("✓"),
            detected_root.to_print_string(),
            version_label,
            pid
        );

        Ok(())
    }

    /// Wait for the daemon to be ready by polling the socket and sending a ping
    async fn wait_for_ready(&self, project_cwd: &Path) -> Result<(), Error> {
        let sock_path = socket_path(project_cwd)?;
        let max_attempts = 100; // 100 * 50ms = 5 seconds max
        let poll_interval = Duration::from_millis(50);

        for _ in 0..max_attempts {
            // Try to connect to the socket
            if let Ok(stream) = UnixStream::connect(sock_path.to_path_buf()).await {
                // Send a ping to verify the daemon is responding
                if self.send_ping(stream).await.is_ok() {
                    return Ok(());
                }
            }
            tokio::time::sleep(poll_interval).await;
        }

        Err(Error::DaemonStartTimeout)
    }

    /// Send a ping message and wait for pong response
    async fn send_ping(&self, stream: UnixStream) -> Result<(), Error> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let request = DaemonRequest::Ping;
        let request_json = serde_json::to_string(&request)
            .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

        writer.write_all(request_json.as_bytes()).await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;
        writer.write_all(b"\n").await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;
        writer.flush().await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;

        let mut response_line = String::new();
        reader.read_line(&mut response_line).await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;

        let response: DaemonResponse = serde_json::from_str(&response_line)
            .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

        match response {
            DaemonResponse::Pong => Ok(()),
        }
    }
}
