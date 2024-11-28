use std::{io::Write, sync::{Arc, Mutex}, thread::JoinHandle, time::Duration};

pub struct Spinner {
    handle: JoinHandle<()>,
    running_w: Arc<Mutex<bool>>,
}

impl Spinner {
    pub fn open() -> Self {
        print!("\x1b[?25l");
        std::io::stdout().flush().unwrap();

        let running_w = std::sync::Arc::new(std::sync::Mutex::new(true));
        let running_r = running_w.clone();

        let handle = std::thread::spawn(move || {
            let chars = "◐◓◑◒".chars().collect::<Vec<_>>();
            let mut idx = 0;

            while *running_r.lock().unwrap() {
                let mut stdout = std::io::stdout();
                write!(stdout, "\x1b[2K\r{}", chars[idx]).unwrap();
                stdout.flush().unwrap();

                idx = (idx + 1) % chars.len();

                std::thread::sleep(Duration::from_millis(50));
            }
        });

        Spinner {
            handle,
            running_w,
        }
    }

    pub fn close(self) {
        let mut stdout = std::io::stdout();
        write!(stdout, "\x1b[2K\r").unwrap();
        stdout.flush().unwrap();

        *self.running_w.lock().unwrap() = false;
        self.handle.join().unwrap();
    }
}
