use std::process::Command;
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

/// Windows equivalent of the Unix guard: Ctrl-C and Ctrl-Break are sent to
/// every process attached to the console, so we register a handler that
/// swallows them while the child is running. Unlike ignoring the events
/// through `SetConsoleCtrlHandler(None, TRUE)`, a handler isn't inherited
/// by the processes we spawn afterwards.
#[cfg(windows)]
pub struct IgnoreSignals {
    _private: (),
}

#[cfg(windows)]
unsafe extern "system" fn ignore_console_ctrl_event(ctrl_type: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    (ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT) as windows_sys::core::BOOL
}

#[cfg(windows)]
impl IgnoreSignals {
    pub fn new() -> Self {
        // SAFETY: The handler is a plain function that doesn't touch any state
        unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ignore_console_ctrl_event), 1); }
        Self { _private: () }
    }
}

#[cfg(windows)]
impl Default for IgnoreSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl Drop for IgnoreSignals {
    fn drop(&mut self) {
        // SAFETY: Unregisters the handler we registered in `new`
        unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ignore_console_ctrl_event), 0); }
    }
}

/// Prevents the standard handles we received from our parent from being
/// inherited by the processes we spawn. Windows makes every inheritable
/// handle available to children, so a detached process (like the daemon)
/// would otherwise keep our parent's stdout pipe open, and tools waiting
/// for it to close (CI runners, `child_process.execFile`, ...) would hang.
/// Children configured to inherit our stdio still get it, since Rust
/// duplicates the standard handles for them.
#[cfg(windows)]
pub fn windows_disable_std_handles_inheritance() {
    use windows_sys::Win32::{Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE}, System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE}};

    for std_handle in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: We only change the inheritance flag of the handles the
        // process was started with; failures (e.g. no handle) are harmless
        unsafe {
            let handle
                = GetStdHandle(std_handle);

            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Whether the process with the given pid is still running.
#[cfg(windows)]
pub fn windows_is_process_alive(pid: u32) -> bool {
    use windows_sys::Win32::{Foundation::{CloseHandle, STILL_ACTIVE}, System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION}};

    // SAFETY: The handle is checked before use and closed afterwards
    unsafe {
        let handle
            = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);

        if handle.is_null() {
            return false;
        }

        let mut exit_code = 0;
        let is_alive
            = GetExitCodeProcess(handle, &mut exit_code) != 0 && exit_code == STILL_ACTIVE as u32;

        CloseHandle(handle);
        is_alive
    }
}

/// Terminates the process with the given pid along with all its
/// descendants. Windows has neither process groups nor termination
/// signals, so this is the closest equivalent to `killpg(SIGKILL)`.
#[cfg(windows)]
pub fn windows_kill_process_tree(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    status.map_or(false, |status| status.success())
}

/// Builds an `ExitStatus` reporting the given exit code.
pub fn exit_status_from_code(code: i32) -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw((code & 0xff) << 8)
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    }
}

/// Returns the signal that terminated the process, if any (always `None`
/// on Windows, where processes don't get terminated by signals).
pub fn exit_status_signal(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }

    #[cfg(not(unix))]
    {
        let _ = status;
        None
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
