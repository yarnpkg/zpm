/// Send SIGTERM to a process group (Unix) or terminate the process (Windows).
#[cfg(unix)]
pub fn kill_process_group(pid: u32) {
    let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
    if result != 0 {
        let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
}

#[cfg(windows)]
pub fn kill_process_group(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(not(any(unix, windows)))]
pub fn kill_process_group(_pid: u32) {}

/// Send SIGKILL to a process (Unix) or terminate the process (Windows).
#[cfg(unix)]
pub fn kill_process(pid: u32) {
    let result = unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
    if result != 0 {
        let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
}

#[cfg(windows)]
pub fn kill_process(pid: u32) {
    zpm_utils::terminate_process(pid);
}

#[cfg(not(any(unix, windows)))]
pub fn kill_process(_pid: u32) {}

/// Check if a process is still alive.
#[cfg(unix)]
pub fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
pub fn is_process_alive(pid: u32) -> bool {
    zpm_utils::is_process_alive(pid)
}

#[cfg(not(any(unix, windows)))]
pub fn is_process_alive(_pid: u32) -> bool {
    true
}
