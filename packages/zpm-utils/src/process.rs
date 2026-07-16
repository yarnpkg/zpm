use std::{path::PathBuf, process::{Command, ExitStatus}};

#[cfg(any(windows, test))]
use std::{ffi::OsString, path::Path};
use shlex::{try_quote, QuoteError};

/// RAII guard to ignore SIGINT and SIGTERM while waiting for a child process.
///
/// When a terminal user presses Ctrl-C, SIGINT is sent to the entire
/// foreground process group. Similarly, SIGTERM may be sent by init
/// systems (e.g. Docker/tini) to request graceful shutdown. If a parent
/// process is waiting for a child, both receive the signal. By ignoring
/// these signals in the parent, we ensure the child can handle them and
/// exit gracefully, and the parent can properly propagate the child's
/// exit code.
///
/// On drop, restores the previous signal handlers.
#[cfg(unix)]
pub struct IgnoreSignals {
    prev_sigint: libc::sighandler_t,
    prev_sigterm: libc::sighandler_t,
}

#[cfg(unix)]
impl IgnoreSignals {
    /// Creates a new guard that ignores SIGINT and SIGTERM until dropped.
    pub fn new() -> Self {
        // SAFETY: We're setting SIG_IGN which is always safe
        let prev_sigint = unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
        let prev_sigterm = unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
        // If signal() returns SIG_ERR, use SIG_DFL as safe fallback so Drop
        // restores a known-valid handler rather than attempting to set SIG_ERR.
        let prev_sigint = if prev_sigint == libc::SIG_ERR { libc::SIG_DFL } else { prev_sigint };
        let prev_sigterm = if prev_sigterm == libc::SIG_ERR { libc::SIG_DFL } else { prev_sigterm };
        Self { prev_sigint, prev_sigterm }
    }
}

#[cfg(unix)]
impl Default for IgnoreSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
impl Drop for IgnoreSignals {
    fn drop(&mut self) {
        // SAFETY: We're restoring the previous handlers
        unsafe {
            libc::signal(libc::SIGINT, self.prev_sigint);
            libc::signal(libc::SIGTERM, self.prev_sigterm);
        }
    }
}

pub fn to_shell_line(cmd: &Command) -> Result<String, QuoteError> {
    let mut parts: Vec<String> = Vec::new();

    // 1.  cd …
    if let Some(dir) = cmd.get_current_dir() {
        parts.push(format!("cd {} &&", try_quote(dir.to_str().unwrap())?));
    }

    // 2.  VAR1=val1 VAR2=val2 …
    let env_entries = cmd.get_envs()
        .filter_map(|(key, value)| value.map(|v| (key, v)))
        .map(|(key, value)| Ok((try_quote(key.to_str().unwrap())?.to_string(), try_quote(value.to_str().unwrap())?.to_string())))
        .collect::<Result<Vec<_>, _>>()?;

    let env_parts = env_entries.iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>();

    parts.extend(env_parts);

    // 3.  executable and args
    parts.push(try_quote(cmd.get_program().to_str().unwrap())?.to_string());

    for arg in cmd.get_args() {
        parts.push(try_quote(arg.to_str().unwrap())?.to_string());
    }

    // Glue it together
    Ok(format!("({})", parts.join(" ")))
}

#[cfg(any(windows, test))]
fn windows_path_exts(pathext: Option<OsString>) -> Vec<String> {
    let pathext = pathext
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());

    pathext.split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| {
            if ext.starts_with('.') {
                ext.to_ascii_lowercase()
            } else {
                format!(".{}", ext.to_ascii_lowercase())
            }
        })
        .collect()
}

#[cfg(any(windows, test))]
fn has_windows_executable_extension(path: &Path, pathexts: &[String]) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let extension = format!(".{}", extension).to_ascii_lowercase();
    pathexts.iter().any(|pathext| pathext == &extension)
}

#[cfg(any(windows, test))]
fn windows_executable_candidates(path: PathBuf, pathexts: &[String]) -> Vec<PathBuf> {
    if has_windows_executable_extension(&path, pathexts) {
        vec![path]
    } else {
        pathexts.iter()
            .map(move |ext| path.with_extension(&ext[1..]))
            .collect()
    }
}

#[cfg(any(windows, test))]
fn resolve_windows_spawn_program_with_env(program: &str, path_env: Option<OsString>, pathext: Option<OsString>) -> Option<PathBuf> {
    let program_path = Path::new(program);
    let pathexts = windows_path_exts(pathext);

    if program_path.components().count() > 1 {
        return windows_executable_candidates(program_path.to_path_buf(), &pathexts)
            .into_iter()
            .find(|candidate| candidate.is_file());
    }

    std::env::split_paths(&path_env.unwrap_or_default())
        .flat_map(|path| windows_executable_candidates(path.join(program), &pathexts))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
pub fn resolve_spawn_program(program: &str, path_env: &str) -> PathBuf {
    resolve_windows_spawn_program_with_env(program, Some(OsString::from(path_env)), std::env::var_os("PATHEXT"))
        .unwrap_or_else(|| PathBuf::from(program))
}

#[cfg(not(windows))]
pub fn resolve_spawn_program(program: &str, _path_env: &str) -> PathBuf {
    PathBuf::from(program)
}

pub fn exit_status_from_code(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }

    #[cfg(not(any(unix, windows)))]
    panic!("synthetic exit statuses are not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::resolve_windows_spawn_program_with_env;

    #[test]
    fn resolves_windows_cmd_shims_from_path() {
        let root = std::env::temp_dir().join(format!("zpm-process-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        std::fs::write(root.join("node-gyp"), "#!/bin/sh\n").unwrap();
        let shim = root.join("node-gyp.cmd");
        std::fs::write(&shim, "@echo off\n").unwrap();

        assert_eq!(
            resolve_windows_spawn_program_with_env("node-gyp", Some(root.into_os_string()), Some(".EXE;.CMD".into())),
            Some(shim),
        );

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("zpm-process-test-{}", std::process::id())));
    }
}

pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::{CloseHandle, STILL_ACTIVE}, System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION}};

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }

            let mut exit_code = 0;
            let result = GetExitCodeProcess(handle, &mut exit_code);
            let _ = CloseHandle(handle);
            result != 0 && exit_code == STILL_ACTIVE as u32
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

pub fn terminate_process(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, libc::SIGTERM) == 0 }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE}};

        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                return false;
            }

            let result = TerminateProcess(handle, 1) != 0;
            let _ = CloseHandle(handle);
            result
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}
