use std::{cell::RefCell, collections::{BTreeMap, BTreeSet}, future::Future, io::{self, Write}, sync::{Arc, LazyLock, atomic::AtomicU32, mpsc}, thread::JoinHandle, time::{Duration, SystemTime}};

use colored::{Color, Colorize};
use dialoguer::{Input, Password};
use itertools::Itertools;
use tokio::sync::{Mutex, RwLock, RwLockReadGuard};
use zpm_config::Configuration;
use zpm_primitives::{Descriptor, Locator};
use zpm_switch::get_bin_version;
use zpm_utils::{DataType, Path, ToHumanString, Unit};

use crate::error::Error;

const TOP_LEVEL_PREFIX: char = '·';

pub static REPORT: LazyLock<RwLock<Option<StreamReport>>> = LazyLock::new(|| RwLock::new(None));

pub async fn set_current_report(report: StreamReport) {
    REPORT.write().await.replace(report);
}

pub async fn current_report() -> RwLockReadGuard<'static, Option<StreamReport>> {
    REPORT.read().await
}

pub async fn async_section<F: Future>(name: &str, f: F) -> F::Output {
    current_report().await.as_ref().map(|r| {
        r.push_section(name.to_string());
    });

    let res
        = f.await;

    current_report().await.as_ref().map(|r| {
        r.pop_section();
    });

    res
}

pub async fn error_handler<T, F: Future<Output = Result<(), Error>>>(f: F) -> () {
    let res = f.await;

    if let Err(e) = &res {
        current_report().await.as_ref().map(|r| {
            r.error(e.clone());
        });
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
            current_report().await.as_ref().map(|r| {
                r.error(e.clone());
            });

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
            current_report().await.as_ref().map(|r| {
                r.error(e.clone());
            });
        }

        res
    }).await
}

#[derive(Debug, Default)]
pub struct StreamReportConfig {
    pub enable_progress_bars: bool,
    pub enable_timers: bool,
    pub include_version: bool,
    pub silent_or_error: bool,
}

impl StreamReportConfig {
    pub fn from_config(config: &Configuration) -> Self {
        Self {
            enable_progress_bars: config.settings.enable_progress_bars.value,
            enable_timers: config.settings.enable_timers.value,
            include_version: false,
            silent_or_error: false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn color(&self) -> DataType {
        match self {
            Severity::Info => DataType::Info,
            Severity::Warning => DataType::Warning,
            Severity::Error => DataType::Error,
        }
    }
}

#[derive(Debug)]
pub enum PromptType {
    Input(String),
    Password(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastMessageType {
    Prompt,
    Line,
}

#[derive(Debug, Default)]
pub struct ReportCounters {
    pub resolution_count: AtomicU32,
    pub fetch_count: AtomicU32,
    pub fetch_size: AtomicU32,
}

#[derive(Debug)]
pub enum ReportMessage {
    Line(Severity, String),
    LogFile(Path),
    RemoteManifestParse {
        locator: String,
        origin: String,
        path: String,
        reason: String,
    },
    PushSection(String),
    PopSection,
    Prompt(PromptType),
}

#[derive(Debug, Default)]
struct RemoteManifestErrorAggregate {
    locators: BTreeSet<String>,
}

struct Reporter {
    config: StreamReportConfig,

    level: usize,
    indent: usize,
    counters: Arc<ReportCounters>,

    last_message_type: Option<LastMessageType>,
    start_time: Option<SystemTime>,
    buffered_lines: Option<Vec<String>>,
    log_paths: Vec<Path>,
    remote_manifest_errors: BTreeMap<(String, String, String), RemoteManifestErrorAggregate>,
    spinner_idx: Option<usize>,
    prompt_tx: mpsc::Sender<String>,
}

impl Reporter {
    pub fn new(config: StreamReportConfig, counters: Arc<ReportCounters>, prompt_tx: mpsc::Sender<String>) -> Self {
        let buffered_lines
            = config.silent_or_error.then_some(Vec::new());

        Self {
            config,
            level: 0,
            indent: 0,
            counters,
            last_message_type: None,
            start_time: None,
            buffered_lines,
            log_paths: Vec::new(),
            remote_manifest_errors: BTreeMap::new(),
            spinner_idx: None,
            prompt_tx,
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
                let prefix
                    = Severity::Info.color().colorize("➤");
                let indent
                    = "  ".repeat(self.indent.saturating_sub(1));

                let chars = "◴◷◶◵".chars().collect_vec();
                write!(writer, "{} {}{} {}", prefix, indent, chars[spinner_idx], self.get_spinner_label()).unwrap();

                self.spinner_idx = Some((spinner_idx + 1) % chars.len());
            }
        }
    }

    fn get_plural_packages(count: u32) -> &'static str {
        if count == 1 {
            "package"
        } else {
            "packages"
        }
    }

    fn get_spinner_label(&self) -> String {
        let resolution_count
            = self.counters.resolution_count.load(std::sync::atomic::Ordering::Relaxed);
        let fetch_count
            = self.counters.fetch_count.load(std::sync::atomic::Ordering::Relaxed);
        let fetch_size
            = self.counters.fetch_size.load(std::sync::atomic::Ordering::Relaxed);

        let color
            = DataType::Custom(144, 144, 144);

        if resolution_count > 0 && fetch_count > 0 {
            color.colorize(&format!(
                "Resolved {} {}, fetched {} {} ({})",
                DataType::Number.colorize(&resolution_count.to_string()),
                Self::get_plural_packages(resolution_count),
                DataType::Number.colorize(&fetch_count.to_string()),
                Self::get_plural_packages(fetch_count),
                Unit::bytes(fetch_size).to_print_string(),
            ))
        } else if resolution_count > 0 {
            color.colorize(&format!(
                "Resolved {} {}",
                DataType::Number.colorize(&resolution_count.to_string()),
                Self::get_plural_packages(resolution_count),
            ))
        } else if fetch_count > 0 {
            color.colorize(&format!(
                "Fetched {} {} ({})",
                DataType::Number.colorize(&fetch_count.to_string()),
                Self::get_plural_packages(fetch_count),
                Unit::bytes(fetch_size).to_print_string(),
            ))
        } else {
            String::new()
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

            ReportMessage::RemoteManifestParse { locator, origin, path, reason } => {
                self.on_remote_manifest_parse(locator, origin, path, reason);
            },

            ReportMessage::PushSection(name) => {
                self.on_push_section(writer, &name);
            },

            ReportMessage::PopSection => {
                self.on_pop_section(writer);
            },

            ReportMessage::Prompt(prompt) => {
                self.on_prompt(writer, prompt);
            },
        }
    }

    fn on_start<T: Write>(&mut self, writer: &mut T) {
        if self.config.enable_progress_bars {
            writer.write_all(b"\x1b[?25l").unwrap();
        }
    }

    fn on_end<T: Write>(&mut self, writer: &mut T) {
        if !self.remote_manifest_errors.is_empty() {
            let mut entries = self.remote_manifest_errors
                .iter()
                .map(|((origin, path, reason), aggregate)| {
                    (
                        origin.clone(),
                        path.clone(),
                        reason.clone(),
                        aggregate.locators.len() as u32,
                        aggregate.locators.clone(),
                    )
                })
                .collect::<Vec<_>>();

            entries.sort_by(|left, right| right.3.cmp(&left.3));

            for (origin, path, reason, count, locators) in entries {
                let summary = if count == 1 {
                    format!("Invalid package metadata in {} ({}): {}", path, origin, reason)
                } else {
                    format!("{} packages have invalid package metadata in {} ({}): {}", count, path, origin, reason)
                };

                self.on_line(writer, Severity::Error, &summary);

                let sample = locators
                    .into_iter()
                    .take(3)
                    .collect_vec();

                if !sample.is_empty() {
                    let hidden_count = count.saturating_sub(sample.len() as u32);
                    let suffix = if hidden_count > 0 {
                        format!(" (+{} more)", hidden_count)
                    } else {
                        String::new()
                    };

                    self.on_line(
                        writer,
                        Severity::Error,
                        &format!("Affected packages: {}{}", sample.join(", "), suffix),
                    );
                }
            }
        }

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

    fn on_remote_manifest_parse(&mut self, locator: String, origin: String, path: String, reason: String) {
        let aggregate = self.remote_manifest_errors
            .entry((origin, path, reason))
            .or_default();

        aggregate.locators.insert(locator);
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

    fn format_prompt(&self, prompt: &str) -> String {
        format!("{} {}", "?".color(Color::TrueColor {r: 47, g: 186, b: 135}), prompt.bold())
    }

    fn on_prompt<T: Write>(&mut self, writer: &mut T, prompt: PromptType) {
        if let Some(last_message_type) = self.last_message_type {
            if last_message_type == LastMessageType::Line {
                writeln!(writer, "").unwrap();
            }
        }

        self.last_message_type = Some(LastMessageType::Prompt);

        match prompt {
            PromptType::Input(prompt) => {
                let label
                    = self.format_prompt(&prompt);

                let input = Input::<String>::new()
                    .with_prompt(label)
                    .interact_text()
                    .unwrap();

                self.prompt_tx.send(input).unwrap();
            },

            PromptType::Password(prompt) => {
                let label
                    = self.format_prompt(&prompt);

                let password = Password::new()
                    .with_prompt(label)
                    .interact()
                    .unwrap();

                self.prompt_tx.send(password).unwrap();
            },
        }
    }

    fn on_pop_section<T: Write>(&mut self, writer: &mut T) {
        if self.level == 0 {
            panic!("Cannot pop section when no sections are pushed");
        }

        self.spinner_idx = None;

        let spinner_label
            = self.get_spinner_label();

        self.counters.resolution_count.store(0, std::sync::atomic::Ordering::Relaxed);
        self.counters.fetch_count.store(0, std::sync::atomic::Ordering::Relaxed);
        self.counters.fetch_size.store(0, std::sync::atomic::Ordering::Relaxed);

        if !spinner_label.is_empty() {
            self.write_line(writer, &spinner_label, Severity::Info);
        }

        self.indent -= 1;

        if let Some(start_time) = self.start_time && let Ok(elapsed) = start_time.elapsed() {
            self.write_line(writer, &format!("└ Completed in {}", Unit::duration_ms_raw(elapsed.as_millis()).to_print_string()), Severity::Info);
        } else {
            self.write_line(writer, "└ Completed", Severity::Info);
        }

        self.level -= 1;
    }

    fn format_indent(&self) -> String {
        if self.level > 0 {
            "│ ".repeat(self.indent)
        } else {
            format!("{} ", TOP_LEVEL_PREFIX)
        }
    }

    fn format_prefix(&self, severity: Severity) -> String {
        format!("{} {}", severity.color().colorize("➤"), self.format_indent())
    }

    fn write_line<T: Write>(&mut self, writer: &mut T, line: &str, severity: Severity) {
        if let Some(last_message_type) = self.last_message_type {
            if last_message_type == LastMessageType::Prompt {
                writeln!(writer, "").unwrap();
            }
        }

        self.last_message_type = Some(LastMessageType::Line);

        let prefix
            = self.format_prefix(severity);

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
    pub counters: Arc<ReportCounters>,

    handle: JoinHandle<()>,
    break_request_tx: mpsc::Sender<bool>,
    msg_queue_tx: mpsc::Sender<ReportMessage>,
    prompt_rx: Mutex<mpsc::Receiver<String>>,
}

impl StreamReport {
    pub fn new(config: StreamReportConfig) -> Self {
        let counters
            = Arc::new(ReportCounters::default());

        let (break_request_tx, break_request_rx)
            = mpsc::channel::<bool>();
        let (msg_queue_tx, msg_queue_rx)
            = mpsc::channel::<ReportMessage>();
        let (prompt_tx, prompt_rx)
            = mpsc::channel::<String>();

        let mut reporter
            = Reporter::new(config, counters.clone(), prompt_tx);

        let handle = std::thread::spawn(move || {
            if reporter.config.enable_progress_bars {
                let mut stdout = io::stdout();
                reporter.on_start(&mut stdout);
                stdout.flush().unwrap();
            }

            if reporter.config.include_version {
                reporter.write_line(&mut io::stdout(), &format!("Yarn {}", get_bin_version()).bold().to_string(), Severity::Info);
            }

            loop {
                let break_request
                    = break_request_rx.recv_timeout(Duration::from_millis(50));

                if break_request == Ok(true) {
                    break;
                }

                let mut stdout
                    = io::stdout();

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
            counters,
            handle,
            break_request_tx,
            msg_queue_tx,
            prompt_rx: Mutex::new(prompt_rx),
        }
    }

    pub fn info(&self, message: String) {
        self.report(ReportMessage::Line(Severity::Info, self.with_content_prefix(message)));
    }

    pub fn warn(&self, message: String) {
        self.report(ReportMessage::Line(Severity::Warning, self.with_content_prefix(message)));
    }

    pub fn error(&self, error: Error) {
        if let Error::RemoteManifestParseError { locator, origin, path, reason } = &error {
            self.report(ReportMessage::RemoteManifestParse {
                locator: locator.to_print_string(),
                origin: origin.to_string(),
                path: path.to_string(),
                reason: reason.to_string(),
            });
            return;
        }

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

    pub async fn prompt(&self, prompt: PromptType) -> String {
        self.report(ReportMessage::Prompt(prompt));

        let prompt_rx
            = self.prompt_rx.lock().await;

        prompt_rx
            .recv()
            .unwrap()
    }

    pub fn close(self) {
        self.break_request_tx.send(true).unwrap();
        self.handle.join().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{ReportCounters, Reporter, StreamReportConfig};

    fn new_reporter() -> Reporter {
        let (prompt_tx, _prompt_rx) = std::sync::mpsc::channel();

        Reporter::new(
            StreamReportConfig {
                enable_progress_bars: false,
                enable_timers: false,
                include_version: false,
                silent_or_error: false,
            },
            Arc::new(ReportCounters::default()),
            prompt_tx,
        )
    }

    #[test]
    fn remote_manifest_summary_counts_unique_locators() {
        let mut reporter = new_reporter();

        reporter.on_remote_manifest_parse(
            "sass-embedded-linux-x64@npm:1.97.3".to_string(),
            "npm registry metadata".to_string(),
            "package.json".to_string(),
            "type mismatch: expected a sequence, got string \"glibc\" (line 1 column 1672)".to_string(),
        );
        reporter.on_remote_manifest_parse(
            "sass-embedded-linux-x64@npm:1.97.3".to_string(),
            "npm registry metadata".to_string(),
            "package.json".to_string(),
            "type mismatch: expected a sequence, got string \"glibc\" (line 1 column 1672)".to_string(),
        );

        let mut output = Vec::new();
        reporter.on_end(&mut output);
        let output = String::from_utf8(output).expect("report output should be valid utf8");

        assert!(output.contains("Invalid package metadata in package.json (npm registry metadata): type mismatch: expected a sequence, got string \"glibc\" (line 1 column 1672)"));
        assert!(!output.contains("2 packages have invalid package metadata"));
    }

    #[test]
    fn remote_manifest_hidden_count_uses_unique_locator_count() {
        let mut reporter = new_reporter();

        for locator in [
            "sass-embedded-linux-x64@npm:1.97.3",
            "sass-embedded-linux-x64@npm:1.97.3",
            "sass-embedded-linux-arm64@npm:1.97.3",
            "sass-embedded-linux-arm64@npm:1.97.3",
        ] {
            reporter.on_remote_manifest_parse(
                locator.to_string(),
                "npm registry metadata".to_string(),
                "package.json".to_string(),
                "type mismatch: expected a sequence, got string \"glibc\" (line 1 column 1672)".to_string(),
            );
        }

        let mut output = Vec::new();
        reporter.on_end(&mut output);
        let output = String::from_utf8(output).expect("report output should be valid utf8");

        assert!(output.contains("2 packages have invalid package metadata in package.json (npm registry metadata):"));
        assert!(!output.contains("(+2 more)"));
    }
}
