use std::{cell::RefCell, future::Future, sync::LazyLock, time::SystemTime};

use tokio::{sync::RwLock, task::futures::TaskLocalFuture};
use zpm_utils::ToHumanString;

use crate::{error::Error, primitives::{Descriptor, Locator}, ui};

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
        r.start_section(name);
    }).await;

    let spinner = ui::spinner::Spinner::open();
    let res = f.await;
    spinner.close();

    current_report(|r| {
        r.end_section();
    }).await;

    res
}

pub enum ReportContext {
    Descriptor(Descriptor),
    Locator(Locator),
}

tokio::task_local! {
    static CONTEXT: RefCell<Option<ReportContext>>;
}

pub fn with_context<F>(context: ReportContext, f: F) -> TaskLocalFuture<RefCell<Option<ReportContext>>, F> where F: Future {
    CONTEXT.scope(RefCell::new(Some(context)), f)
}
pub fn with_context_result<F, R>(context: ReportContext, f: F) -> TaskLocalFuture<RefCell<Option<ReportContext>>, impl Future<Output = Result<R, Error>>> where F: Future<Output = Result<R, Error>> {
    CONTEXT.scope(RefCell::new(Some(context)), async move {
        let res = f.await;

        if let Err(e) = &res {
            current_report(|r| {
                r.report(ReportMessage::Error(e.clone()));
            }).await;
        }

        res
    })
}

pub struct StreamReportConfig {
    pub enable_timers: bool,
}

pub enum ReportMessage {
    Info(String),
    Error(Error)
}

pub struct StreamReport {
    config: StreamReportConfig,
    prefix: String,
    start_time: Option<SystemTime>,
}

impl StreamReport {
    pub fn new(config: StreamReportConfig) -> Self {
        Self {
            config,
            prefix: String::new(),
            start_time: None,
        }
    }

    pub fn report(&mut self, message: ReportMessage) {
        CONTEXT.with(move |context| {
            let context = context.borrow();

            let mut line = String::new();

            if !self.prefix.is_empty() {
                line.push_str("\x1b[2K\r");
            }

            line.push_str(&self.prefix);

            if let Some(context) = &*context {
                line.push_str(&match context {
                    ReportContext::Descriptor(descriptor) => descriptor.to_print_string(),
                    ReportContext::Locator(locator) => locator.to_print_string(),
                });

                line.push_str(": ");
            }

            line.push_str(&match message {
                ReportMessage::Info(message) => message,
                ReportMessage::Error(error) => error.to_string(),
            });

            println!("{}", line);
        });
    }

    pub fn start_section(&mut self, name: &str) {
        println!("┌ {}", name);

        self.prefix.push_str("│ ");

        if self.config.enable_timers {
            self.start_time = Some(SystemTime::now());
        }
    }

    pub fn end_section(&mut self) {
        self.prefix.pop();
        self.prefix.pop();

        if let Some(start_time) = self.start_time {
            if let Ok(elapsed) = start_time.elapsed() {
                println!("└ Completed in {}", pretty_duration::pretty_duration(&elapsed, None));
            } else {
                println!("└ Completed");
            }
        } else {
            println!("└ Completed");
        }
    }
}
