use clipanion::cli;
use zpm_utils::{DataType, ToHumanString};

use crate::{daemons, errors::Error};

#[cli::command]
#[cli::path("switch", "daemon")]
#[cli::category("Daemon management")]
#[derive(Debug)]
pub struct DaemonKillAllCommand {
    #[cli::option("--kill-all")]
    _kill_all: bool,
}

impl DaemonKillAllCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let all_daemons = daemons::list_daemons()?;

        if all_daemons.is_empty() {
            println!(
                "{} No daemons registered",
                DataType::Info.colorize("ℹ")
            );
            return Ok(());
        }

        let mut killed = 0;
        let mut failed = 0;
        let mut stale = 0;

        for daemon in all_daemons {
            if !daemons::is_process_alive(daemon.pid) {
                daemons::unregister_daemon(&daemon.project_cwd)?;
                stale += 1;
                continue;
            }

            if daemons::kill_process(daemon.pid) {
                daemons::unregister_daemon(&daemon.project_cwd)?;
                println!(
                    "{} Stopped daemon for {} (PID: {})",
                    DataType::Success.colorize("✓"),
                    daemon.project_cwd.to_print_string(),
                    daemon.pid
                );
                killed += 1;
            } else {
                println!(
                    "{} Failed to stop daemon for {} (PID: {})",
                    DataType::Error.colorize("✗"),
                    daemon.project_cwd.to_print_string(),
                    daemon.pid
                );
                failed += 1;
            }
        }

        if stale > 0 {
            println!(
                "{} Cleaned up {} stale daemon entries",
                DataType::Info.colorize("ℹ"),
                stale
            );
        }

        if failed > 0 {
            println!(
                "\n{} Stopped {} daemons, {} failed",
                DataType::Warning.colorize("!"),
                killed,
                failed
            );
        } else if killed > 0 {
            println!(
                "\n{} Stopped {} daemons",
                DataType::Success.colorize("✓"),
                killed
            );
        }

        Ok(())
    }
}
