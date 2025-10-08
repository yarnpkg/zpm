use clipanion::cli;
use zpm_utils::{Path, ToFileString};

/// Print the path of the current Yarn Switch binary
#[cli::command]
#[cli::path("switch", "which")]
#[cli::category("Switch commands")]
#[derive(Debug)]
pub struct WhichCommand {
}

impl WhichCommand {
    pub async fn execute(&self) {
        println!("{}", Path::current_exe().unwrap().to_file_string());
    }
}
