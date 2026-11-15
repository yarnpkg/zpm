use clipanion::cli;
use zpm_utils::Path;

/// Print the path of the current Yarn Switch binary
///
/// ## Examples
///
/// Print the path of the Yarn Switch binary
///
/// `yarn switch which`
///
#[cli::command]
#[cli::path("switch", "which")]
#[cli::category("Switch commands")]
#[derive(Debug)]
pub struct WhichCommand {
}

impl WhichCommand {
    pub async fn execute(&self) {
        println!("{}", Path::current_exe().unwrap().to_native_string());
    }
}
