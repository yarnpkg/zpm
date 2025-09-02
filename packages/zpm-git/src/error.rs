use std::string::FromUtf8Error;

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Invalid Git URL: {0}")]
    InvalidGitUrl(String),

    #[error(transparent)]
    FromUtf8Error(#[from] FromUtf8Error),

    #[error(transparent)]
    SemverError(#[from] zpm_semver::Error),
}
