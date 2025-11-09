use clipanion::{cli, prelude::CommandProvider};

use crate::commands::SwitchExecCli;

/// Print the version of the current Yarn Switch binary
#[cli::command]
#[cli::path("switch")]
#[cli::category("Switch commands")]
pub struct ClipanionCommandsCommand {
    #[cli::option("--clipanion-commands")]
    _clipanion_commands: bool,
}

impl ClipanionCommandsCommand {
    pub async fn execute(&self) {
        let commands
            = SwitchExecCli::registered_commands()
                .expect("Expected the CLI to build successfully");

        let commands_json
            = zpm_parsers::JsonDocument::to_string(&commands)
                .expect("Expected the CLI to serialize successfully");

        println!("{}", commands_json);
    }
}
