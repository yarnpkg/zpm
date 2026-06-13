use std::process::ExitCode;

use clipanion::cli;
use zpm_macro_enum::zpm_enum;
use zpm_utils::Path;

use crate::{errors::Error, links::{get_trusted, set_trusted}};

#[zpm_enum(or_else = |s| Err(Error::InvalidTrustLevel(s.to_string())))]
#[derive(Debug, Copy, Clone)]
enum TrustLevel {
    #[literal("true")]
    Trusted,

    #[literal("false")]
    Untrusted,

    #[literal("null")]
    Unknown,
}

impl TrustLevel {
    fn as_option(self) -> Option<bool> {
        match self {
            Self::Trusted => Some(true),
            Self::Untrusted => Some(false),
            Self::Unknown => None,
        }
    }
}

/// Check whether a project is trusted by Yarn Switch
#[cli::command]
#[cli::path("switch", "trust")]
#[cli::category("Project trust")]
#[derive(Debug)]
pub struct TrustCheckCommand {
    /// Check the stored trust state
    #[cli::option("--check")]
    _check: bool,

    /// Project path to check
    path: Path,
}

impl TrustCheckCommand {
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let path
            = self.path.fs_canonicalize()
                .unwrap_or_else(|_| self.path.clone());

        match get_trusted(&path)? {
            Some(true) => Ok(ExitCode::SUCCESS),
            Some(false) => Ok(ExitCode::from(2)),
            None => Ok(ExitCode::from(3)),
        }
    }
}

/// Set the Yarn Switch trust state for a project
#[cli::command]
#[cli::path("switch", "trust")]
#[cli::category("Project trust")]
#[derive(Debug)]
pub struct TrustSetCommand {
    /// Trust value to store: `true`, `false`, or `null`
    #[cli::option("--set")]
    trusted: TrustLevel,

    /// Project path to update
    path: Path,
}

impl TrustSetCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let path
            = self.path.fs_canonicalize()
                .unwrap_or_else(|_| self.path.clone());

        set_trusted(&path, self.trusted.as_option())?;

        Ok(())
    }
}
