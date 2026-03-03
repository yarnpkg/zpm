use chrono::{Local, TimeZone, Utc};
use colored::Colorize;
use zpm_tasks::TaskId;
use zpm_utils::{DataType, FromFileString, ToHumanString};

use crate::daemon::{AttachedLongLivedTask, LONG_LIVED_CONTEXT_ID};

pub fn format_task_id(task_id: &str) -> String {
    let base
        = task_id.rsplit_once('@').map(|(b, _)| b).unwrap_or(task_id);

    TaskId::from_file_string(base)
        .map(|t| t.to_print_string())
        .unwrap_or_else(|_| base.to_string())
}

pub fn format_timestamp() -> String {
    DataType::Timestamp.colorize(&Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string())
}

pub fn is_long_lived_task(task_id: &str) -> bool {
    task_id.ends_with(&format!("@{}", LONG_LIVED_CONTEXT_ID))
}

pub fn format_start_time(started_at_ms: u64) -> String {
    let secs
        = (started_at_ms / 1000) as i64;

    let nanos
        = ((started_at_ms % 1000) * 1_000_000) as u32;

    Utc.timestamp_opt(secs, nanos)
        .single()
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn print_attach_header(attached: &AttachedLongLivedTask) {
    println!(
        "{}",
        format!(
            "Attaching to long-lived task {} (started at {})",
            format_task_id(&attached.task_id),
            format_start_time(attached.started_at_ms)
        ).bold()
    );
}

pub fn print_detach_footer(task_name: &str) {
    println!("{}", "The long-lived task is still running in the background.".bold());
    println!();
    println!("{}", format!("  To re-attach: yarn tasks run {}", task_name).dimmed());
    println!("{}", format!("  To stop:      yarn tasks stop {}", task_name).dimmed());
}
