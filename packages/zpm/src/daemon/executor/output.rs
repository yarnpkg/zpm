use std::sync::{Arc, Mutex};

use tokio::{io::{AsyncBufReadExt, BufReader}, process::{ChildStderr, ChildStdout}};

use crate::task_cache::LogLine;

use super::super::{
    coordinator_commands::{CommandSender, CoordinatorCommand},
    coordinator_state::ContextualTaskId,
    events::Stream,
};

pub async fn stream_output(
    stdout: ChildStdout,
    stderr: ChildStderr,
    task_id: ContextualTaskId,
    command_tx: CommandSender,
    capture: Option<Arc<Mutex<Vec<LogLine>>>>,
) {
    let record = |stream: &Stream, line: &str| {
        if let Some(capture) = &capture {
            capture.lock().unwrap().push(LogLine {stream: stream.as_str().to_string(), line: line.to_string()});
        }
    };

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let mut stdout_done = false;
    let mut stderr_done = false;

    loop {
        tokio::select! {
            line = stdout_reader.next_line(), if !stdout_done => {
                match line {
                    Ok(Some(line)) => {
                        record(&Stream::Stdout, &line);
                        if command_tx.send(CoordinatorCommand::TaskOutput {
                            task_id: task_id.clone(),
                            line,
                            stream: Stream::Stdout,
                        }).is_err() {
                            return;
                        }
                    }
                    Ok(None) => { stdout_done = true; }
                    Err(e) => {
                        eprintln!("stdout read error for task {:?}: {}", task_id, e);
                        stdout_done = true;
                    }
                }
            }
            line = stderr_reader.next_line(), if !stderr_done => {
                match line {
                    Ok(Some(line)) => {
                        record(&Stream::Stderr, &line);
                        if command_tx.send(CoordinatorCommand::TaskOutput {
                            task_id: task_id.clone(),
                            line,
                            stream: Stream::Stderr,
                        }).is_err() {
                            return;
                        }
                    }
                    Ok(None) => { stderr_done = true; }
                    Err(e) => {
                        eprintln!("stderr read error for task {:?}: {}", task_id, e);
                        stderr_done = true;
                    }
                }
            }
        }

        if stdout_done && stderr_done {
            break;
        }
    }
}
