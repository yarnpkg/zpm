use std::{cell::RefCell, future::Future, io::{self, Write}, sync::{mpsc, LazyLock}, thread::JoinHandle, time::{Duration, SystemTime}};

use tokio::{sync::RwLock, task::futures::TaskLocalFuture};
use zpm_utils::ToHumanString;

use crate::{error::Error, primitives::{Descriptor, Locator}};

pub static REPORT: LazyLock<RwLock<Option<StreamReport>>> = LazyLock::new(|| RwLock::new(None));

pub async fn set_current_report(report: StreamReport) {
    REPORT.write().await.replace(report);
}

pub async fn current_report<F: FnOnce(&mut StreamReport) -> ()>(f: F) -> () {
    if let Some(report) = REPORT.write().await.as_mut() {
        f(report);
    };
}

pub async fn async_section<F: Future>(name: &str, f: F) -> F::Output {
    current_report(|r| {
        r.push_section(name.to_string());
    }).await;

    let res
        = f.await;

    current_report(|r| {
        r.pop_section();
    }).await;

    res
}

pub async fn error_handler<T, F: Future<Output = Result<(), Error>>>(f: F) -> () {
    let res = f.await;

    if let Err(e) = &res {
        current_report(|r| {
            r.error(e.clone());
        }).await;
    }

    ()
}

pub enum ReportContext {
    Descriptor(Descriptor),
    Locator(Locator),
}

tokio::task_local! {
    static CONTEXT: RefCell<Option<ReportContext>>;
}

pub async fn with_report<F, R>(report: StreamReport, f: F) -> R where F: Future<Output = R> {
    set_current_report(report).await;

    let res
        = CONTEXT.scope(RefCell::new(None), f).await;

    let report
        = REPORT.write().await.take()
            .expect("No report set");

    report.close();

    res
}

pub async fn with_report_result<F, R>(report: StreamReport, f: F) -> Result<R, Error> where F: Future<Output = Result<R, Error>> {
    with_report(report, async move {
        let res
            = f.await;

        if let Err(e) = &res {
            current_report(|r| {
                r.error(e.clone());
            }).await;

            return Err(Error::SilentError);
        }

        res
    }).await
}

pub async fn with_context<F>(context: ReportContext, f: F) -> () where F: Future {
    CONTEXT.scope(RefCell::new(Some(context)), f).await;
}

pub async fn with_context_result<F, R>(context: ReportContext, f: F) -> Result<R, Error> where F: Future<Output = Result<R, Error>> {
    CONTEXT.scope(RefCell::new(Some(context)), async move {
        let res = f.await;

        if let Err(e) = &res {
            current_report(|r| {
                r.error(e.clone());
            }).await;
        }

        res
    }).await
}

#[derive(Debug)]
pub struct StreamReportConfig {
    pub enable_timers: bool,
    pub silent_or_error: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Error,
}

#[derive(Debug)]
pub enum ReportMessage {
    Line(Severity, String),
    PushSection(String),
    PopSection,
}

struct Reporter {
    config: StreamReportConfig,
    prefix: String,
    start_time: Option<SystemTime>,
    buffered_lines: Option<Vec<String>>,
    spinner_idx: Option<usize>,
}

impl Reporter {
    pub fn new(config: StreamReportConfig) -> Self {
        let buffered_lines
            = config.silent_or_error.then_some(Vec::new());

        Self {
            config,
            prefix: String::new(),
            start_time: None,
            buffered_lines,
            spinner_idx: None,
        }
    }

    pub fn clear_spinner<T: Write>(&mut self, writer: &mut T) {
        if self.spinner_idx.is_some() {
            if !self.config.silent_or_error {
                write!(writer, "\x1b[2K\r").unwrap();
            }
        }
    }

    pub fn write_spinner<T: Write>(&mut self, writer: &mut T) {
        if let Some(spinner_idx) = self.spinner_idx {
            if !self.config.silent_or_error {
                let chars = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".chars().collect::<Vec<_>>();
                write!(writer, "{}", chars[spinner_idx]).unwrap();

                self.spinner_idx = Some((spinner_idx + 1) % chars.len());
            }
        }
    }

    pub fn report<T: Write>(&mut self, writer: &mut T, message: ReportMessage) {
        match message {
            ReportMessage::Line(severity, message) => {
                self.on_line(writer, severity, &message);
            },

            ReportMessage::PushSection(name) => {
                self.on_push_section(writer, &name);
            },

            ReportMessage::PopSection => {
                self.on_pop_section(writer);
            },
        }
    }

    fn on_line<T: Write>(&mut self, writer: &mut T, severity: Severity, message: &str) {
        if self.config.silent_or_error {
            if severity == Severity::Error {
                self.stop_buffering(writer);
            }
        }

        self.write_line(writer, &message);
    }

    fn on_push_section<T: Write>(&mut self, writer: &mut T, name: &str) {
        self.write_line(writer, &format!("┌ {}", name));

        self.prefix.push_str("│ ");
        self.spinner_idx = Some(0);

        if self.config.enable_timers {
            self.start_time = Some(SystemTime::now());
        }
    }

    fn on_pop_section<T: Write>(&mut self, writer: &mut T) {
        self.prefix.pop();
        self.prefix.pop();

        self.spinner_idx = None;

        if let Some(start_time) = self.start_time {
            if let Ok(elapsed) = start_time.elapsed() {
                self.write_line(writer, &format!("└ Completed in {}", pretty_duration::pretty_duration(&elapsed, None)));
                return;
            }
        }

        self.write_line(writer, "└ Completed");
    }

    fn write_line<T: Write>(&mut self, writer: &mut T, line: &str) {
        if let Some(buffered_lines) = &mut self.buffered_lines {
            buffered_lines.push(line.to_string());
        } else {
            writeln!(writer, "{}", line).unwrap();
        }
    }

    pub fn stop_buffering<T: Write>(&mut self, writer: &mut T) {
        self.config.silent_or_error = false;

        if let Some(buffered_lines) = self.buffered_lines.take() {
            for line in buffered_lines {
                writeln!(writer, "{}", line).unwrap();
            }
        }
    }
}

pub struct StreamReport {
    handle: JoinHandle<()>,
    break_request_tx: mpsc::Sender<bool>,
    msg_queue_tx: mpsc::Sender<ReportMessage>,
}

impl StreamReport {
    pub fn new(config: StreamReportConfig) -> Self {
        let mut reporter
            = Reporter::new(config);

        let (break_request_tx, break_request_rx)
            = mpsc::channel::<bool>();
        let (msg_queue_tx, msg_queue_rx)
            = mpsc::channel::<ReportMessage>();

        let handle = std::thread::spawn(move || {
            let chars = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".chars().collect::<Vec<_>>();

            let mut idx = 0;
            loop {
                let break_request
                    = break_request_rx.recv_timeout(Duration::from_millis(50));

                if break_request == Ok(true) {
                    break;
                }

                idx = (idx + 1) % chars.len();

                let mut stdout = io::stdout();

                reporter.clear_spinner(&mut stdout);

                for msg in msg_queue_rx.try_iter() {
                    reporter.report(&mut stdout, msg);
                }

                reporter.write_spinner(&mut stdout);

                stdout.flush().unwrap();
            }
        });

        Self {
            handle,
            break_request_tx,
            msg_queue_tx,
        }
    }

    pub fn info(&self, message: String) {
        self.report(ReportMessage::Line(Severity::Info, self.with_content_prefix(message)));
    }

    pub fn error(&self, error: Error) {
        if !matches!(error, Error::SilentError) {
            self.report(ReportMessage::Line(Severity::Error, self.with_content_prefix(error.to_string())));
        }
    }

    pub fn push_section(&self, name: String) {
        self.report(ReportMessage::PushSection(name));
    }

    pub fn pop_section(&self) {
        self.report(ReportMessage::PopSection);
    }

    fn with_content_prefix(&self, mut message: String) -> String {
        CONTEXT.with(move |context: &RefCell<Option<ReportContext>>| {
            let context
                = context.borrow();

            let Some(context) = context.as_ref() else {
                return message;
            };

            let prefix = match context {
                ReportContext::Descriptor(descriptor) => descriptor.to_print_string(),
                ReportContext::Locator(locator) => locator.to_print_string(),
            };

            message.reserve(prefix.len() + 2 + message.len());

            message.insert_str(0, &prefix);
            message.insert_str(prefix.len(), ": ");

            message
        })
    }

    fn report(&self, message: ReportMessage) {
        // TODO: This should let us send many messages but only wake up for the important ones
        let should_wake_up = true;

        self.msg_queue_tx.send(message).unwrap();

        if should_wake_up {
            self.break_request_tx.send(false).unwrap();
        }
    }

    pub fn close(self) {
        self.break_request_tx.send(true).unwrap();
        self.handle.join().unwrap();
    }
}
