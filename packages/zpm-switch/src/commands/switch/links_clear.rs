use clipanion::cli;
use zpm_utils::IoResultExt;

use crate::{errors::Error, links::links_dir};

#[cli::command]
#[cli::path("switch", "links")]
#[cli::category("Local Yarn development")]
#[cli::description("Clear all local links")]
#[derive(Debug)]
pub struct LinksClearCommand {
    #[cli::option("-c,--clear,--clean")]
    _clear: bool,
}

impl LinksClearCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let links_dir
            = links_dir()?;

        links_dir
            .fs_rm()
            .ok_missing()?;

        Ok(())
    }
}
