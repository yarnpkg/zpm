use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use zpm_parsers::JsonDocument;
use zpm_semver::Version;
use zpm_utils::{Hash64, IoResultExt, Path, ToFileString};

use crate::errors::Error;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DaemonEntry {
    pub project_cwd: Path,
    pub yarn_version: Version,
    pub pid: u32,
    pub port: u16,
}

pub fn daemons_dir() -> Result<Path, Error> {
    let daemons_dir
        = Path::home_dir()?
            .ok_or(Error::MissingHomeFolder)?
            .with_join_str(".yarn/switch/daemons");

    Ok(daemons_dir)
}

fn daemon_file_path(project_cwd: &Path) -> Result<Path, Error> {
    let hash
        = Hash64::from_data(project_cwd.to_file_string().as_bytes());

    let daemon_path
        = daemons_dir()?
            .with_join_str(format!("{}.json", hash.short()));

    Ok(daemon_path)
}

pub fn register_daemon(entry: &DaemonEntry) -> Result<(), Error> {
    let daemon_path
        = daemon_file_path(&entry.project_cwd)?;

    daemon_path
        .fs_create_parent()?
        .fs_write(JsonDocument::to_string(entry)?)?;

    // Set restrictive permissions (owner read/write only) to protect sensitive daemon info
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        daemon_path.fs_set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

pub fn unregister_daemon(project_cwd: &Path) -> Result<(), Error> {
    let daemon_path
        = daemon_file_path(project_cwd)?;

    daemon_path
        .fs_rm()
        .ok_missing()?;

    Ok(())
}

pub fn get_daemon(project_cwd: &Path) -> Result<Option<DaemonEntry>, Error> {
    let daemon_path
        = daemon_file_path(project_cwd)?;

    let daemon
        = daemon_path
            .fs_read_text()
            .ok_missing()?
            .and_then(|content| {
                JsonDocument::hydrate_from_str::<DaemonEntry>(&content)
                    .map_err(|e| eprintln!("Warning: failed to parse daemon file {:?}: {}", daemon_path, e))
                    .ok()
            });

    Ok(daemon)
}

pub fn list_daemons() -> Result<BTreeSet<DaemonEntry>, Error> {
    let daemons_dir
        = daemons_dir()?;

    let Some(dir_entries) = daemons_dir.fs_read_dir().ok_missing()? else {
        return Ok(BTreeSet::new());
    };

    let daemons
        = dir_entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map_or(false, |f| f.is_file()))
            .filter_map(|entry| Path::try_from(entry.path()).ok())
            .filter_map(|path| {
                path.fs_read_text()
                    .map_err(|e| eprintln!("Warning: failed to read daemon file {:?}: {}", path, e))
                    .ok()
            })
            .filter_map(|content| JsonDocument::hydrate_from_str::<DaemonEntry>(&content).ok())
            .collect::<BTreeSet<_>>();

    Ok(daemons)
}

pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(windows)]
    {
        use std::ptr::null_mut;
        unsafe {
            let handle = winapi::um::processthreadsapi::OpenProcess(
                winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            );
            if handle.is_null() {
                false
            } else {
                winapi::um::handleapi::CloseHandle(handle);
                true
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

pub fn kill_process(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, libc::SIGTERM) == 0 }
    }

    #[cfg(windows)]
    {
        use std::ptr::null_mut;
        unsafe {
            let handle = winapi::um::processthreadsapi::OpenProcess(
                winapi::um::winnt::PROCESS_TERMINATE,
                0,
                pid,
            );
            if handle.is_null() {
                false
            } else {
                let result = winapi::um::processthreadsapi::TerminateProcess(handle, 1) != 0;
                winapi::um::handleapi::CloseHandle(handle);
                result
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Kill a daemon process and all its children (process group).
/// Sends SIGTERM first, waits for the process to exit, then sends SIGKILL if needed.
/// Returns true if the process was successfully killed.
pub fn kill_daemon_gracefully(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // First, send SIGTERM to the daemon process itself
        // The daemon's signal handler should propagate to children
        let term_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if term_result != 0 {
            // Process doesn't exist or we don't have permission
            return false;
        }

        // Wait up to 6 seconds for the daemon to shut down gracefully
        // (daemon waits 5s internally for children to exit, plus 1s buffer)
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !is_process_alive(pid) {
                return true;
            }
        }

        // If still alive after 6 seconds, send SIGKILL to the process group
        // Use negative PID to target the entire process group
        let pgid = unsafe { libc::getpgid(pid as i32) };
        if pgid > 0 {
            let _ = unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
        // Also send SIGKILL directly to the daemon in case it's not the process group leader
        let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };

        // Wait a bit for the process to actually die
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !is_process_alive(pid) {
                return true;
            }
        }

        // Return false if we couldn't verify death after SIGKILL
        !is_process_alive(pid)
    }

    #[cfg(windows)]
    {
        // On Windows, just use TerminateProcess (no graceful shutdown)
        kill_process(pid)
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

pub fn cleanup_stale_daemons() -> Result<(), Error> {
    let daemons
        = list_daemons()?;

    for daemon in daemons {
        if !is_process_alive(daemon.pid) {
            unregister_daemon(&daemon.project_cwd)?;
        }
    }

    Ok(())
}

pub fn list_live_daemons() -> Result<BTreeSet<DaemonEntry>, Error> {
    cleanup_stale_daemons()?;
    list_daemons()
}

/// Kill a daemon and unregister it, returning true if successful.
pub async fn kill_and_unregister_daemon(daemon: &DaemonEntry) -> Result<bool, Error> {
    let pid = daemon.pid;
    let success
        = tokio::task::spawn_blocking(move || kill_daemon_gracefully(pid))
            .await
            .unwrap_or(false);

    if success {
        unregister_daemon(&daemon.project_cwd)?;
    }

    Ok(success)
}
