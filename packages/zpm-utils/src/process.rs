use std::process::Command;
use shlex::{try_quote, QuoteError};

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
