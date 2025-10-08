use clipanion::cli;
use zpm_utils::get_system_string;

#[cli::command]
#[cli::path("debug", "print-platform")]
pub struct PrintPlatform {
}

impl PrintPlatform {
    pub async fn execute(&self) {
        let platform
            = get_system_string();

        println!("{}", platform);
    }
}
