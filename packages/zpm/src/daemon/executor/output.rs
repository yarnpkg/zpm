use tokio::{io::{AsyncBufReadExt, BufReader}, process::{ChildStderr, ChildStdout}};

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
) {
    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let mut stdout_done = false;
    let mut stderr_done = false;

    loop {
        tokio::select! {
            line = stdout_reader.next_line(), if !stdout_done => {
                match line {
                    Ok(Some(line)) => {
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
