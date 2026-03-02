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

    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let _ = tx.send(OutputLine {
                            line,
                            stream: Stream::Stdout,
                        });
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            line = stderr_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let _ = tx.send(OutputLine {
                            line,
                            stream: Stream::Stderr,
                        });
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }
    }

    while let Ok(Some(line)) = stderr_reader.next_line().await {
        let _ = tx.send(OutputLine {
            line,
            stream: Stream::Stderr,
        });
    }
}
