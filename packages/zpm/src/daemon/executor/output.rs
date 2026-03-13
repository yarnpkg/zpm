use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStderr;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;

use super::super::events::Stream;

pub struct OutputLine {
    pub line: String,
    pub stream: Stream,
}

pub async fn stream_output(
    stdout: ChildStdout,
    stderr: ChildStderr,
    tx: mpsc::UnboundedSender<OutputLine>,
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
                        if tx.send(OutputLine {
                            line,
                            stream: Stream::Stdout,
                        }).is_err() {
                            return;
                        }
                    }
                    Ok(None) | Err(_) => { stdout_done = true; }
                }
            }
            line = stderr_reader.next_line(), if !stderr_done => {
                match line {
                    Ok(Some(line)) => {
                        if tx.send(OutputLine {
                            line,
                            stream: Stream::Stderr,
                        }).is_err() {
                            return;
                        }
                    }
                    Ok(None) | Err(_) => { stderr_done = true; }
                }
            }
        }

        if stdout_done && stderr_done {
            break;
        }
    }
}
