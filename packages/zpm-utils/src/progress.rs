use std::future::Future;
use std::io::Write;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct ProgressHandle {
    stop_tx: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressHandle {
    pub fn stop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());

            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.stop_tx.is_some()
    }
}

impl Drop for ProgressHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start_progress<F>(write_progress: F) -> ProgressHandle
where
    F: Fn(usize) -> String + Send + 'static,
{
    let (stop_tx, stop_rx)
        = mpsc::channel::<()>();

    let handle
        = std::thread::spawn(move || {
            run_progress_thread(write_progress, stop_rx);
        });

    ProgressHandle {
        stop_tx: Some(stop_tx),
        handle: Some(handle),
    }
}

pub async fn with_progress<F, W, R>(write_progress: F, work: W) -> R
where
    F: Fn(usize) -> String + Send + 'static,
    W: Future<Output = R>,
{
    let mut handle
        = start_progress(write_progress);

    let result
        = work.await;

    handle.stop();

    result
}

fn run_progress_thread<F>(write_progress: F, stop_rx: mpsc::Receiver<()>)
where
    F: Fn(usize) -> String,
{
    let spinner_chars: [char; 6]
        = ['⠾', '⠷', '⠯', '⠟', '⠻', '⠽'];

    let mut frame_idx: usize
        = 0;

    let mut stdout
        = std::io::stdout().lock();

    stdout.write_all(b"\x1b[?25l").ok();

    loop {
        match stop_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let progress_text
            = write_progress(frame_idx);

        if !progress_text.is_empty() {
            let mut stdout
                = std::io::stdout().lock();

            write!(
                stdout,
                "\x1b[2K\r{} {}",
                spinner_chars[frame_idx % spinner_chars.len()],
                progress_text
            ).ok();

            stdout.flush().ok();

            frame_idx = frame_idx.wrapping_add(1);
        }
    }

    let mut stdout
        = std::io::stdout().lock();

    stdout.write_all(b"\x1b[2K\r\x1b[?25h").ok();
    stdout.flush().ok();
}
