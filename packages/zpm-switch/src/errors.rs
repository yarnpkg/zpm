use std::sync::Arc;

use reqwest::StatusCode;
use zpm_utils::PathError;

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error(transparent)]
    PathError(#[from] PathError),

    #[error(transparent)]
    SemverError(#[from] zpm_semver::Error),

    #[error(transparent)]
    FormatError(#[from] zpm_formats::Error),

    #[error(transparent)]
    IOError(#[from] Arc<std::io::Error>),

    #[error(transparent)]
    Utf8Error(#[from] Arc<std::str::Utf8Error>),

    #[error(transparent)]
    RequestError(#[from] Arc<reqwest::Error>),

    #[error("Unknown binary name: {0}")]
    UnknownBinaryName(String),

    #[error("Failed to get current executable path")]
    FailedToGetExecutablePath,

    #[error("Invalid packageManager string")]
    InvalidPackageManagerString,

    #[error("Invalid version selector: {0}")]
    InvalidVersionSelector(String),

    #[error("Failed to parse manifest: {0}")]
    FailedToParseManifest(Arc<sonic_rs::Error>),

    #[error("Server answered with HTTP {0} ({1})")]
    HttpStatus(StatusCode, String),

    #[error("Failed to retrieve the latest tag from the Yarn registry")]
    FailedToRetrieveLatestYarnTag,

    #[error("Missing home folder")]
    MissingHomeFolder,

    #[error("Invalid package manager reference ({0})")]
    InvalidPackageManagerReference(String),

    #[error("Package manifests aren't allowed to reference local binaries ({0})")]
    PackageManifestsCannotReferenceLocalBinaries(String),

    #[error("Explicit paths must contain a slash character")]
    InvalidExplicitPathParameter,

    #[error("This package manager cannot be used to interact on project configured for use with {0}")]
    UnsupportedProject(String),
}

impl From<std::str::Utf8Error> for Error {
    fn from(value: std::str::Utf8Error) -> Self {
        Error::Utf8Error(Arc::new(value))
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Error::from(Arc::new(value))
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::from(Arc::new(value))
    }
}
