use clipanion::cli;
use zpm_utils::{Path, ToFileString};

#[cli::command]
#[cli::path("switch", "which")]
#[derive(Debug)]
pub struct WhichCommand {
}

impl WhichCommand {
    pub async fn execute(&self) {
        println!("{}", Path::current_exe().unwrap().to_file_string());
    }
}
