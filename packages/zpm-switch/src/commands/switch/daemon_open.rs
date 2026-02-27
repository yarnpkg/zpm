use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use clipanion::cli;
use futures::stream::StreamExt;
use futures::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use zpm_semver::Version;
use zpm_utils::{Path, ToFileString};

use crate::{
    cwd::get_final_cwd,
    daemons::{self, DaemonEntry},
    errors::Error,
    install::install_package_manager,
    ipc::{daemon_url, DaemonRequest, DaemonResponse},
    links::{get_link, LinkTarget},
    manifest::{find_closest_package_manager, PackageManagerReference},
    yarn::get_default_yarn_version,
    yarn_enums::ReleaseLine,
};

/// Open a daemon for the current project, starting it if needed
#[cli::command]
#[cli::path("switch", "daemon")]
#[cli::category("Daemon management")]
#[derive(Debug)]
pub struct DaemonOpenCommand {
    #[cli::option("--open")]
    _open: bool,
}

impl DaemonOpenCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let project_cwd = get_final_cwd()?;

        let find_result = find_closest_package_manager(&project_cwd)?;

        let detected_root = find_result
            .detected_root_path
            .ok_or(Error::NoProjectFound)?;

        // Check if a daemon is already running for this project
        if let Some(existing) = daemons::get_daemon(&detected_root)? {
            if daemons::is_process_alive(existing.pid) {
                // Verify the daemon is responding
                if self.ping_daemon(existing.port).await.is_ok() {
                    println!("{}", daemon_url(existing.port));
                    return Ok(());
                }
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
        self.start_with_command(&detected_root, &mut binary, &version.to_file_string())
            .await
    }

    async fn start_with_binary(
        &self,
        detected_root: &Path,
        bin_path: &Path,
        version_label: &str,
    ) -> Result<(), Error> {
        let mut binary = Command::new(bin_path.to_path_buf());
        self.start_with_command(detected_root, &mut binary, version_label)
            .await
    }

    async fn start_with_command(
        &self,
        detected_root: &Path,
        binary: &mut Command,
        version_label: &str,
    ) -> Result<(), Error> {
        // Spawn the daemon process with stdout piped to capture port
        binary
            .arg("debug")
            .arg("daemon")
            .current_dir(detected_root.to_file_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        // Ensure HOME is passed to daemon
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

        let mut child = binary
            .spawn()
            .map_err(|e| Error::FailedToStartDaemon(Arc::new(e)))?;

        let pid = child.id();

        // Read the port from stdout
        let port = self.read_port_from_child(&mut child).await?;

        // Register the daemon
        let entry = DaemonEntry {
            project_cwd: detected_root.clone(),
            yarn_version: version_label.parse().unwrap_or_else(|_| Version::new()),
            pid,
            port,
        };

        daemons::register_daemon(&entry)?;

        // Wait for daemon to be ready
        self.wait_for_ready(port).await?;

        // Print the WebSocket URL
        println!("{}", daemon_url(port));

        Ok(())
    }

    /// Read the port number from the daemon's stdout
    async fn read_port_from_child(&self, child: &mut std::process::Child) -> Result<u16, Error> {
        use std::io::{BufRead, BufReader};

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::DaemonStartTimeout)?;

        // Use blocking read in a separate task with timeout
        let port = tokio::task::spawn_blocking(move || {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            if let Some(Ok(line)) = lines.next() {
                line.trim().parse::<u16>().ok()
            } else {
                None
            }
        })
        .await
        .map_err(|e| Error::JoinFailed(Arc::new(e)))?
        .ok_or(Error::DaemonStartTimeout)?;

        Ok(port)
    }

    /// Wait for the daemon to be ready by sending a ping
    async fn wait_for_ready(&self, port: u16) -> Result<(), Error> {
        let max_attempts = 100; // 100 * 50ms = 5 seconds max
        let poll_interval = Duration::from_millis(50);

        for _ in 0..max_attempts {
            if self.ping_daemon(port).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(poll_interval).await;
        }

        Err(Error::DaemonStartTimeout)
    }

    /// Send a ping message to the daemon and wait for pong response
    async fn ping_daemon(&self, port: u16) -> Result<(), Error> {
        let url = daemon_url(port);

        let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| {
                Error::DaemonConnectionFailed(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                )))
            })?;

        let (mut write, mut read) = ws_stream.split();

        let request = DaemonRequest::Ping;
        let request_json =
            serde_json::to_string(&request).map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

        write
            .send(Message::Text(request_json.into()))
            .await
            .map_err(|e| {
                Error::DaemonConnectionFailed(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )))
            })?;

        // Wait for response with timeout
        let response = tokio::time::timeout(Duration::from_secs(5), read.next())
            .await
            .map_err(|_| Error::DaemonStartTimeout)?
            .ok_or(Error::DaemonStartTimeout)?
            .map_err(|e| {
                Error::DaemonConnectionFailed(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )))
            })?;

        if let Message::Text(text) = response {
            let response: DaemonResponse = serde_json::from_str(&text)
                .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

            match response {
                DaemonResponse::Pong => Ok(()),
                _ => Err(Error::InvalidDaemonMessage("Expected Pong response".to_string())),
            }
        } else {
            Err(Error::InvalidDaemonMessage("Expected text message".to_string()))
        }
    }
}
