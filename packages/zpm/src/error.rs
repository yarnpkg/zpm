use std::sync::Arc;

use arca::Path;

use crate::primitives::{Ident, Range};

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Failed to change the current working directory")]
    FailedToChangeCwd,

    #[error("Unsupported code path")]
    Unsupported,

    #[error("Invalid descriptor ({0})")]
    InvalidDescriptor(String),

    #[error("Invalid range ({0})")]
    InvalidRange(String),

    #[error("Invalid reference ({0})")]
    InvalidReference(String),

    #[error("Project not found ({0:?})")]
    ProjectNotFound(Path),

    #[error("Invalid value; expected an ident or a locator ({0})")]
    InvalidIdentOrLocator(String),

    #[error("Invalid semver version ({0})")]
    InvalidSemverVersion(String),

    #[error("Invalid semver range ({0})")]
    InvalidSemverRange(String),

    #[error("Missing semver tag ({0})")]
    MissingSemverTag(String),

    #[error("Package not found ({0}, at {1})")]
    PackageNotFound(Ident, String),

    #[error("No candidates found for {0:?}")]
    NoCandidatesFound(Range),

    #[error("I/O error")]
    IoError(#[from] Arc<std::io::Error>),

    #[error("UTF-8 error")]
    Utf8Error(#[from] Arc<std::str::Utf8Error>),

    #[error("Invalid JSON data")]
    InvalidJsonData(#[from] Arc<serde_json::Error>),

    #[error("Invalid SHA256 data")]
    InvalidSha256(String),

    #[error("Invalid YAML data")]
    InvalidYamlData(#[from] Arc<serde_yaml::Error>),

    #[error("DNS resolution error")]
    DnsResolutionError(Arc<dyn std::error::Error + Send + Sync>),

    #[error("Invalid workspace pattern ({0})")]
    InvalidWorkspacePattern(String),

    #[error("Remote registry error ({0})")]
    RemoteRegistryError(Arc<reqwest::Error>),

    #[error("Internal serialization error")]
    InternalSerializationError(#[from] Arc<bincode::error::EncodeError>),

    #[error("Internal serialization error")]
    InternalDeserializationError(#[from] Arc<bincode::error::DecodeError>),

    #[error("An error occured while reading the lockfile from disk")]
    LockfileReadError(Arc<std::io::Error>),

    #[error("An error occured while parsing the lockfile")]
    LockfileParseError(Arc<serde_json::Error>),

    #[error("Lockfile generation error")]
    LockfileGenerationError(Arc<serde_json::Error>),

    #[error("An error occured while persisting the lockfile on disk")]
    LockfileWriteError(Arc<std::io::Error>),

    #[error("Git error")]
    GitError,

    #[error("Invalid Git commit ({0})")]
    InvalidGitCommit(String),

    #[error("Invalid Git branch ({0})")]
    InvalidGitBranch(String),

    #[error("Invalid Git specifier")]
    InvalidGitSpecifier,

    #[error("Unknown error")]
    UnknownError(Arc<Box<dyn std::error::Error + Send + Sync>>),

    #[error("Invalid tar file path ({0})")]
    InvalidTarFilePath(String),

    #[error("Missing package manifest")]
    MissingPackageManifest,

    #[error("Package conversion error ({0})")]
    PackageConversionError(Arc<Box<dyn std::error::Error + Send + Sync>>),

    #[error("Workspace not found ({0})")]
    WorkspaceNotFoundByName(Ident),

    #[error("Workspace not found ({0})")]
    WorkspaceNotFoundByPath(String),

    #[error("Install state file not found; please run an install operation first")]
    InstallStateNotFound,

    #[error("Couldn't find a package matching the current working directory")]
    ActivePackageNotFound,

    #[error("Script not found ({0})")]
    ScriptNotFound(String),

    #[error("Global script not found ({0})")]
    GlobalScriptNotFound(String),

    #[error("Multiple definitions of the same global script ({0})")]
    AmbiguousScriptName(String),

    #[error("Binary not found ({0})")]
    BinaryNotFound(String),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Arc::new(error).into()
    }
}
