// ============================================================================
// Platform-specific process operations
//
// Consolidates all platform-gated process operations into one module.
// Every other file imports from here instead of writing its own #[cfg] blocks.
// ============================================================================

/// Send SIGTERM to a process group (Unix) or terminate the process (Windows).
#[cfg(unix)]
pub fn kill_process_group(pid: u32) {
    let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
    if result != 0 {
        let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
}

#[cfg(not(unix))]
pub fn kill_process_group(_pid: u32) {}

/// Send SIGKILL to a process (Unix) or terminate the process (Windows).
#[cfg(unix)]
pub fn kill_process(pid: u32) {
    let result = unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
    if result != 0 {
        let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
}

#[cfg(not(unix))]
pub fn kill_process(_pid: u32) {}

/// Check if a process is still alive.
#[cfg(unix)]
pub fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
pub fn is_process_alive(_pid: u32) -> bool {
    false
}
