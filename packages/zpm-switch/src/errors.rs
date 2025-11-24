use std::sync::Arc;

use reqwest::StatusCode;
use zpm_utils::{DataType, Path, PathError, ToHumanString};

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
    JsonError(#[from] zpm_parsers::Error),

    #[error("Failed to execute the {program} binary: {error}", program = DataType::Code.colorize(&.0), error = .1.to_string())]
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
    FailedToParseManifest(zpm_parsers::Error),

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

    #[error("You opted-in to a package manager migration, but the manifest in {} doesn't list a {} field", .0.to_print_string(), DataType::Code.colorize("packageManagerMigration"))]
    MissingMigration(Path),

    #[error("Yarn cannot be used on project configured for use with {0}")]
    UnsupportedProject(&'static str),
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
