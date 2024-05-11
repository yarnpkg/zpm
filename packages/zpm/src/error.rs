use std::sync::Arc;

use arca::Path;

use crate::primitives::{Ident, Range};

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
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

    #[error("Invalid semver version ({0})")]
    InvalidSemverVersion(String),

    #[error("Invalid semver range ({0})")]
    InvalidSemverRange(String),

    #[error("Missing semver tag ({0})")]
    MissingSemverTag(String),

    #[error("No candidates found for {0:?}")]
    NoCandidatesFound(Range),

    #[error("I/O error")]
    IoError(#[from] Arc<std::io::Error>),

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

    #[error("Remote registry error")]
    RemoteRegistryError(Arc<reqwest::Error>),

    #[error("Internal serialization error")]
    InternalSerializationError(#[from] Arc<bincode::error::EncodeError>),

    #[error("Internal serialization error")]
    InternalDeserializationError(#[from] Arc<bincode::error::DecodeError>),

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

    #[error("Package conversion error")]
    PackageConversionError(Arc<Box<dyn std::error::Error + Send + Sync>>),

    #[error("Workspace not found ({0})")]
    WorkspaceNotFound(Ident),
}
