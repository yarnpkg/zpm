use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use clipanion::cli;
use zpm_semver::Version;
use zpm_utils::{Path, ToFileString};

use crate::{
    cwd::get_final_cwd,
    daemons::{self, DaemonEntry},
    errors::Error,
    install::install_package_manager,
    links::{get_link, LinkTarget},
    manifest::{find_closest_package_manager, PackageManagerReference},
    yarn::get_default_yarn_version,
    yarn_enums::ReleaseLine,
};

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
        let project_cwd
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&project_cwd)?;

        let detected_root
            = find_result
                .detected_root_path
                .ok_or(Error::NoProjectFound)?;

        if let Some(existing) = daemons::get_daemon(&detected_root)? {
            if daemons::is_process_alive(existing.pid) {
                if self.check_daemon_ready(existing.port).await.is_ok() {
                    println!("ws://127.0.0.1:{}", existing.port);
                    return Ok(());
                }
                // Process alive but not answering — terminate it and its children before replacing
                let pid = existing.pid;
                let _ = tokio::task::spawn_blocking(move || daemons::kill_daemon_gracefully(pid)).await;
            }

            daemons::unregister_daemon(&detected_root)?;
        }

        if let Some(link) = get_link(&detected_root)? {
            if let LinkTarget::Local { bin_path } = link.link_target {
                return self.start_with_binary(&detected_root, &bin_path, "local").await;
            }
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => package_manager.into_reference("yarn"),
            None => get_default_yarn_version(Some(ReleaseLine::Classic)).await,
        }?;

        match &reference {
            PackageManagerReference::Version(version_ref) => {
                let mut binary
                    = install_package_manager(version_ref).await?;

                self.start_with_command(&detected_root, &mut binary, &version_ref.version.to_file_string())
                    .await
            },

            PackageManagerReference::Local(local_ref) => {
                self.start_with_binary(&detected_root, &local_ref.path, "local")
                    .await
            },
        }
    }

    async fn start_with_binary(&self, detected_root: &Path, bin_path: &Path, version_label: &str) -> Result<(), Error> {
        let mut binary
            = Command::new(bin_path.to_path_buf());

        self.start_with_command(detected_root, &mut binary, version_label)
            .await
    }

    async fn start_with_command(&self, detected_root: &Path, binary: &mut Command, version_label: &str) -> Result<(), Error> {
        binary
            .arg("debug")
            .arg("daemon")
            .current_dir(detected_root.to_file_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        if let Ok(home) = std::env::var("HOME") {
            binary.env("HOME", home);
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            binary.env("USERPROFILE", userprofile);
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            binary.process_group(0);
        }

        let mut child
            = binary
                .spawn()
                .map_err(|e| Error::FailedToStartDaemon(Arc::new(e)))?;

        let pid
            = child.id();
        let port
            = self.read_port_from_child(&mut child).await?;

        let entry = DaemonEntry {
            project_cwd: detected_root.clone(),
            yarn_version: version_label.parse().unwrap_or_else(|_| Version::new()),
            pid,
            port,
        };

        daemons::register_daemon(&entry)?;

        if let Err(e) = self.wait_for_ready(port).await {
            // Daemon failed to become ready — kill it and clean up registry
            let _ = tokio::task::spawn_blocking(move || daemons::kill_daemon_gracefully(pid)).await;
            daemons::unregister_daemon(&detected_root)?;
            return Err(e);
        }

        println!("ws://127.0.0.1:{}", port);

        Ok(())
    }

    async fn read_port_from_child(&self, child: &mut std::process::Child) -> Result<u16, Error> {
        use std::io::{BufRead, BufReader};

        let stdout
            = child
                .stdout
                .take()
                .ok_or_else(|| Error::DaemonStartTimeout)?;

        let port
            = tokio::task::spawn_blocking(move || {
                let reader
                    = BufReader::new(stdout);

                let mut lines
                    = reader.lines();

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

    async fn wait_for_ready(&self, port: u16) -> Result<(), Error> {
        let max_attempts
            = 100;
        let poll_interval
            = Duration::from_millis(50);

        for _ in 0..max_attempts {
            if self.check_daemon_ready(port).await.is_ok() {
                return Ok(());
            }

            tokio::time::sleep(poll_interval).await;
        }

        Err(Error::DaemonStartTimeout)
    }

    async fn check_daemon_ready(&self, port: u16) -> Result<(), Error> {
        let url
            = format!("ws://127.0.0.1:{}", port);

        // Just attempt to establish a WebSocket connection - if it succeeds, daemon is ready
        let (mut ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| {
                Error::DaemonConnectionFailed(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                )))
            })?;

        // Close the connection properly to avoid connection resets
        ws.close(None).await.ok();

        Ok(())
    }
}
