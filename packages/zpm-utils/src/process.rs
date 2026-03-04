use std::process::Command;
use shlex::{try_quote, QuoteError};

/// RAII guard to ignore SIGINT while waiting for a child process.
///
/// When a terminal user presses Ctrl-C, SIGINT is sent to the entire
/// foreground process group. If a parent process is waiting for a child,
/// both receive the signal. By ignoring SIGINT in the parent, we ensure
/// the child can handle the signal and exit gracefully, and the parent
/// can properly propagate the child's exit code.
///
/// On drop, restores the previous signal handler.
#[cfg(unix)]
pub struct IgnoreSigint {
    prev_handler: libc::sighandler_t,
}

#[cfg(unix)]
impl IgnoreSigint {
    /// Creates a new guard that ignores SIGINT until dropped.
    pub fn new() -> Self {
        // SAFETY: We're setting SIG_IGN which is always safe
        let prev_handler = unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
        Self { prev_handler }
    }
}

#[cfg(unix)]
impl Default for IgnoreSigint {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
impl Drop for IgnoreSigint {
    fn drop(&mut self) {
        // SAFETY: We're restoring the previous handler
        unsafe { libc::signal(libc::SIGINT, self.prev_handler) };
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
