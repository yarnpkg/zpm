use std::{cell::RefCell, future::Future, io::{self, Write}, sync::{mpsc, LazyLock}, thread::JoinHandle, time::{Duration, SystemTime}};

use colored::{Color, Colorize};
use tokio::sync::RwLock;
use zpm_switch::get_bin_version;
use zpm_utils::{Path, ToHumanString};

use crate::{config::Config, error::Error, primitives::{Descriptor, Locator}};

const TOP_LEVEL_PREFIX: char = '·';

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
    pub enable_progress_bars: bool,
    pub enable_timers: bool,
    pub include_version: bool,
    pub silent_or_error: bool,
}

impl StreamReportConfig {
    pub fn from_config(config: &Config) -> Self {
        Self {
            enable_progress_bars: config.user.enable_progress_bars.value,
            enable_timers: true,
            include_version: false,
            silent_or_error: false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Error,
}

#[derive(Debug)]
pub enum ReportMessage {
    Line(Severity, String),
    LogFile(Path),
    PushSection(String),
    PopSection,
}

struct Reporter {
    config: StreamReportConfig,

    level: usize,
    indent: usize,

    start_time: Option<SystemTime>,
    buffered_lines: Option<Vec<String>>,
    log_paths: Vec<Path>,
    spinner_idx: Option<usize>,
}

impl Reporter {
    pub fn new(config: StreamReportConfig) -> Self {
        let buffered_lines
            = config.silent_or_error.then_some(Vec::new());

        Self {
            config,
            level: 0,
            indent: 0,
            start_time: None,
            buffered_lines,
            log_paths: Vec::new(),
            spinner_idx: None,
        }
    }

    pub fn clear_spinner<T: Write>(&mut self, writer: &mut T) {
        if self.spinner_idx.is_some() {
            if !self.config.silent_or_error && self.config.enable_progress_bars {
                write!(writer, "\x1b[2K\r").unwrap();
            }
        }
    }

    pub fn write_spinner<T: Write>(&mut self, writer: &mut T) {
        if let Some(spinner_idx) = self.spinner_idx {
            if !self.config.silent_or_error && self.config.enable_progress_bars {
                let chars = "◐◓◑◒".chars().collect::<Vec<_>>();
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

            ReportMessage::LogFile(log_path) => {
                self.log_paths.push(log_path);
            },

            ReportMessage::PushSection(name) => {
                self.on_push_section(writer, &name);
            },

            ReportMessage::PopSection => {
                self.on_pop_section(writer);
            },
        }
    }

    fn on_start<T: Write>(&mut self, writer: &mut T) {
        if self.config.enable_progress_bars {
            writer.write_all(b"\x1b[?25l").unwrap();
        }
    }

    fn on_end<T: Write>(&mut self, writer: &mut T) {
        for log_path in &self.log_paths {
            writeln!(writer, "\n{}\n", log_path.to_print_string()).unwrap();

            let log_content = log_path.fs_read_text().unwrap();
            writeln!(writer, "{}", log_content).unwrap();
        }

        if self.config.enable_progress_bars {
            writer.write_all(b"\x1b[?25h").unwrap();
        }
    }

    fn on_line<T: Write>(&mut self, writer: &mut T, severity: Severity, message: &str) {
        if self.config.silent_or_error {
            if severity == Severity::Error {
                self.stop_buffering(writer);
            }
        }

        self.write_line(writer, message, severity);
    }

    fn on_push_section<T: Write>(&mut self, writer: &mut T, name: &str) {
        self.level += 1;

        self.write_line(writer, &format!("┌ {}", name), Severity::Info);

        self.indent += 1;

        self.spinner_idx = Some(0);

        if self.config.enable_timers {
            self.start_time = Some(SystemTime::now());
        }
    }

    fn on_pop_section<T: Write>(&mut self, writer: &mut T) {
        self.indent -= 1;

        self.spinner_idx = None;

        if let Some(start_time) = self.start_time {
            if let Ok(elapsed) = start_time.elapsed() {
                self.write_line(writer, &format!("└ Completed in {}", pretty_duration::pretty_duration(&elapsed, None)), Severity::Info);
                return;
            }
        }

        self.write_line(writer, "└ Completed", Severity::Info);

        self.level -= 1;
    }

    fn format_indent(&self) -> String {
        if self.level > 0 {
            "│ ".repeat(self.indent)
        } else {
            format!("{} ", TOP_LEVEL_PREFIX)
        }
    }

    fn format_prefix(&self, caret_color: Color) -> String {
        format!("{} {}", "➤".color(caret_color), self.format_indent())
    }

    fn write_line<T: Write>(&mut self, writer: &mut T, line: &str, severity: Severity) {
        let prefix = self.format_prefix(match severity {
            Severity::Info => Color::BrightBlue,
            Severity::Error => Color::BrightRed,
        });

        if let Some(buffered_lines) = &mut self.buffered_lines {
            buffered_lines.push(format!("{}{}", prefix, line));
        } else {
            writeln!(writer, "{}{}", prefix, line).unwrap();
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

            if reporter.config.enable_progress_bars {
                let mut stdout = io::stdout();
                reporter.on_start(&mut stdout);
                stdout.flush().unwrap();
            }

            if reporter.config.include_version {
                reporter.write_line(&mut io::stdout(), &format!("Yarn {}", get_bin_version()).bold().to_string(), Severity::Info);
            }

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

            let mut stdout = io::stdout();
            reporter.on_end(&mut stdout);
            stdout.flush().unwrap();
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

    pub fn error(&mut self, error: Error) {
        if !matches!(error, Error::SilentError) {
            self.report(ReportMessage::Line(Severity::Error, self.with_content_prefix(error.to_string())));
        }

        if let Error::ChildProcessFailedWithLog(_, log_path) = error {
            self.report(ReportMessage::LogFile(log_path));
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
