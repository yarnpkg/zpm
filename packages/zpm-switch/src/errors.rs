use std::sync::Arc;

use reqwest::StatusCode;
use zpm_utils::{PathError, ToHumanString};

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

    #[error(transparent)]
    JsonError(#[from] Arc<sonic_rs::Error>),

    #[error("Failed to execute the {0} binary: {0}")]
    FailedToExecuteBinary(String, Arc<std::io::Error>),

    #[error("Unknown binary name: {0}")]
    UnknownBinaryName(String),

    #[error("Cache not found: {version}", version = .0.to_print_string())]
    CacheNotFound(zpm_semver::Version),

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

    #[error("Project not found")]
    ProjectNotFound,

    #[error("Failed to retrieve the latest tag from the Yarn registry")]
    FailedToRetrieveLatestYarnTag,

    #[error("Failed to find a Yarn version matching {}", .0.to_print_string())]
    FailedToResolveYarnRange(zpm_semver::Range),

    #[error("Missing home folder")]
    MissingHomeFolder,

    #[error("Invalid package manager reference ({0})")]
    InvalidPackageManagerReference(String),

    #[error("Package manifests aren't allowed to reference local binaries ({0})")]
    PackageManifestsCannotReferenceLocalBinaries(String),

    #[error("Explicit paths must contain a slash character")]
    InvalidExplicitPathParameter,

    #[error("Volta's platform.json file is invalid; expected an object")]
    VoltaPlatformJsonInvalid,

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

impl From<sonic_rs::Error> for Error {
    fn from(value: sonic_rs::Error) -> Self {
        Error::JsonError(Arc::new(value))
    }
}
