use clipanion::cli;
use zpm_utils::{DataType, ToHumanString};

use crate::{
    cwd::get_final_cwd,
    daemons,
    errors::Error,
    manifest::find_closest_package_manager,
};

#[cli::command]
#[cli::path("switch", "daemon")]
#[cli::category("Daemon management")]
#[derive(Debug)]
pub struct DaemonKillCommand {
    #[cli::option("--kill")]
    _kill: bool,
}

impl DaemonKillCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let project_cwd = get_final_cwd()?;

        let find_result = find_closest_package_manager(&project_cwd)?;

        let detected_root = find_result
            .detected_root_path
            .ok_or(Error::NoProjectFound)?;

        let Some(daemon) = daemons::get_daemon(&detected_root)? else {
            println!(
                "{} No daemon registered for this project",
                DataType::Info.colorize("ℹ")
            );
            return Ok(());
        };

        if !daemons::is_process_alive(daemon.pid) {
            daemons::unregister_daemon(&detected_root)?;
            println!(
                "{} Daemon was not running (cleaned up stale entry)",
                DataType::Info.colorize("ℹ")
            );
            return Ok(());
        }

        let success = daemons::kill_and_unregister_daemon(&daemon).await?;

        if success {
            println!(
                "{} Stopped daemon for {} (PID: {})",
                DataType::Success.colorize("✓"),
                detected_root.to_print_string(),
                daemon.pid
            );
        } else {
            println!(
                "{} Failed to stop daemon (PID: {})",
                DataType::Error.colorize("✗"),
                daemon.pid
            );
        }

        Ok(())
    }
}
