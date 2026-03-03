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
            .and_then(|content| JsonDocument::hydrate_from_str::<DaemonEntry>(&content).ok());

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
            .filter_map(|path| path.fs_read_text().ok())
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
