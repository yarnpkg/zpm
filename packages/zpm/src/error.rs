use std::sync::Arc;

use arca::Path;

use crate::primitives::{Ident, Locator};

fn render_backtrace(backtrace: &std::backtrace::Backtrace) -> String {
    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        backtrace.to_string().trim_end().to_string()
    } else {
        "Run with RUST_BACKTRACE=1 to get a backtrace".to_string()
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Generic internal error: Please replace this error with a more specific one")]
    ReplaceMe,

    #[error("Unsupported code path")]
    Unsupported,

    #[error("Failed to change the current working directory")]
    FailedToChangeCwd,

    #[error("Invalid ident ({0})")]
    InvalidIdent(String),

    #[error("Package manifest not found")]
    ManifestNotFound,

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

    #[error("Tag not found ({0})")]
    TagNotFound(String),

    #[error("Package not found ({0}, at {1})")]
    PackageNotFound(Ident, String),

    #[error("No candidates found for {0:?}")]
    NoCandidatesFound(String),

    #[error("I/O error ({inner})\n\n{}", render_backtrace(backtrace))]
    IoError {
        inner: Arc<std::io::Error>,
        backtrace: Arc<std::backtrace::Backtrace>,
    },

    #[error("UTF-8 error")]
    Utf8Error(#[from] Arc<std::str::Utf8Error>),

    #[error("UTF-8 error")]
    Utf8Error2(#[from] std::str::Utf8Error),

    #[error("Invalid JSON data ({0})")]
    InvalidJsonData(#[from] Arc<sonic_rs::Error>),

    #[error("Error parsing an integer value")]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error("Invalid SHA256 data")]
    InvalidSha256(String),

    #[error("Invalid YAML data")]
    InvalidYamlData(#[from] Arc<serde_yaml::Error>),

    #[error("DNS resolution error")]
    DnsResolutionError(Arc<dyn std::error::Error + Send + Sync>),

    #[error("Invalid workspace pattern ({0})")]
    InvalidWorkspacePattern(String),

    #[error("Invalid file pattern ({0})")]
    InvalidFilePattern(String),

    #[error("Remote registry error ({0})")]
    RemoteRegistryError(Arc<reqwest::Error>),

    #[error("Internal serialization error")]
    InternalSerializationError(#[from] Arc<bincode::error::EncodeError>),

    #[error("Internal serialization error")]
    InternalDeserializationError(#[from] Arc<bincode::error::DecodeError>),

    #[error("An error occured while reading the lockfile from disk")]
    LockfileReadError(Arc<std::io::Error>),

    #[error("An error occured while parsing the lockfile ({0})")]
    LockfileParseError(Arc<sonic_rs::Error>),

    #[error("An error occured while parsing the lockfile ({0})")]
    LegacyLockfileParseError(Arc<serde_yaml::Error>),

    #[error("Lockfile generation error")]
    LockfileGenerationError(Arc<sonic_rs::Error>),

    #[error("Repository clone failed")]
    RepositoryCloneFailed(String),

    #[error("Repository checkout failed")]
    RepositoryCheckoutFailed(String, String),

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
    WorkspaceNotFound(Ident),

    #[error("Workspace path not found ()")]
    WorkspacePathNotFound(),

    #[error("Install state file not found; please run an install operation first")]
    InstallStateNotFound,

    #[error("Couldn't find a package matching the current working directory")]
    ActivePackageNotFound,

    #[error("The active package is not a workspace")]
    ActivePackageNotWorkspace,

    #[error("Script not found ({0})")]
    ScriptNotFound(String),

    #[error("Global script not found ({0})")]
    GlobalScriptNotFound(String),

    #[error("Multiple definitions of the same global script ({0})")]
    AmbiguousScriptName(String),

    #[error("Binary not found ({0})")]
    BinaryNotFound(String),

    #[error("Circular build dependency detected")]
    CircularBuildDependency(Locator),

    #[error("Some build scripts failed to run")]
    BuildScriptsFailedToRun,

    #[error("Invalid pack pattern ({0})")]
    InvalidPackPattern(String),

    #[error("Invalid git url ({0})")]
    InvalidGitUrl(String),

    #[error("Child process failed ({0})")]
    ChildProcessFailed(String),

    #[error("Child process failed ({0}); check {1} for details")]
    ChildProcessFailedWithLog(String, Path),

    #[error("Failed to interpret as an utf8 string")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),

    #[error("Unrecognized pragma in patch file ({0})")]
    UnrecognizedPatchPragma(String),

    #[error("Unsufficient pragma context")]
    UnsufficientPragmaContext,

    #[error("Hunk lines encountered before the hunk header")]
    HunkLinesBeforeHeader,

    #[error("Invalid hunk header ({0})")]
    InvalidHunkHeader(String),

    #[error("Invalid diff line ({0})")]
    InvalidDiffLine(String),

    #[error("Hunk integrity check failed")]
    HunkIntegrityCheckFailed,

    #[error("Invalid mode in patch file ({0})")]
    InvalidModeInPatchFile(u32),

    #[error("No changes found in this patch file")]
    EmptyPatchFile,

    #[error("Missing rename target in patch file")]
    MissingRenameTarget,

    #[error("Missing source path")]
    MissingFromPath,

    #[error("Missing target path")]
    MissingToPath,

    #[error("Patched file not found ({0})")]
    PatchedFileNotFound(String),

    #[error("Unmatched hunk")]
    UnmatchedHunk(usize),

    #[error("Invalid resolution")]
    InvalidResolution(String),

    #[error("Task timeout")]
    TaskTimeout,
}

impl Error {
    pub fn ignore<T, F: FnOnce(&Error) -> bool>(self, f: F) -> Result<Option<T>, Error> {
        match f(&self) {
            true => Ok(None),
            false => Err(self),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::IoError {
            inner: Arc::new(error),
            backtrace: Arc::new(std::backtrace::Backtrace::capture()),
        }
    }
}

impl From<bincode::error::EncodeError> for Error {
    fn from(error: bincode::error::EncodeError) -> Self {
        Arc::new(error).into()
    }
}

impl From<sonic_rs::Error> for Error {
    fn from(error: sonic_rs::Error) -> Self {
        Arc::new(error).into()
    }
}

impl From<ignore::Error> for Error {
    fn from(error: ignore::Error) -> Self {
        match error {
            ignore::Error::Io(err) => {
                err.into()
            }

            _ => {
                Error::UnknownError(Arc::new(Box::new(error)))
            }
        }
    }
}
