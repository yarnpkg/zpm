use std::{future::Future, sync::Arc};

use zpm_primitives::{Descriptor, Ident, Locator, Range};
use zpm_utils::{DataType, Path, ToHumanString};
use tokio::task::JoinError;

fn render_backtrace(backtrace: &std::backtrace::Backtrace) -> String {
    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        backtrace.to_string().trim_end().to_string()
    } else {
        "Run with RUST_BACKTRACE=1 to get a backtrace".to_string()
    }
}

pub async fn set_timeout<F: Future>(timeout: std::time::Duration, f: F) -> Result<F::Output, Error> {
    let res = tokio::time::timeout(timeout, f).await
        .map_err(|_| Error::TaskTimeout)?;

    Ok(res)
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Home directory not found")]
    HomeDirectoryNotFound,

    #[error("Failed to read the requested setting: {0}")]
    ConfigurationError(#[from] zpm_config::GetError),

    #[error("Failed to hydrate the requested setting: {0}")]
    ConfigurationHydrateError(#[from] zpm_config::HydrateError),

    #[error("Invalid locator: {0}")]
    LocatorError(#[from] zpm_primitives::LocatorError),

    #[error("Generic internal error: Please replace this error with a more specific one")]
    ReplaceMe,

    #[error("Unsupported code path")]
    Unsupported,

    #[error(transparent)]
    SwitchError(#[from] zpm_switch::Error),

    #[error("Network error: {0}{}", .1.as_deref().map(|s| format!(" ({})", s)).unwrap_or_default())]
    HttpError(Arc<reqwest::Error>, Option<String>),

    #[error(transparent)]
    PathError(#[from] zpm_utils::PathError),

    #[error(transparent)]
    SyncError(#[from] zpm_utils::SyncError),

    #[error(transparent)]
    SyncError2(#[from] zpm_sync::SyncError),

    #[error("Private packages cannot be published")]
    CannotPublishPrivatePackage,

    #[error("Cannot publish packages with a missing name or version")]
    CannotPublishMissingNameOrVersion,

    #[error("Invalid publish access: {0}")]
    InvalidNpmPublishAccess(String),

    #[error("Missing environment variable when creating the provenance payload: {0}")]
    MissingEnvironmentVariableForProvenancePayload(String),

    #[error("Provenance error: {0}")]
    ProvenanceError(String),

    #[error("Publishing a package with provenance requires authentication")]
    ProvenanceRequiresAuthentication,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Conflicting options: {0}")]
    ConflictingOptions(String),

    #[error("Can't link the project to itself")]
    CannotLinkToSelf,

    #[error("The linked package at {} doesn't have a name", .0.to_print_string())]
    LinkedPackageMissingName(Path),

    #[error("Checksum mismatch for {}", .0.to_print_string())]
    ChecksumMismatch(Locator),

    #[error("[YN0028] The lockfile would have been created by this install, which is explicitly forbidden.")]
    ImmutableLockfile,

    #[error("Cannot autofix a lockfile when running an immutable install.")]
    ImmutableLockfileAutofix,

    #[error("Found an incorrectly formatted package manifest when running an immutable install ({})", .0.to_print_string())]
    ImmutablePackageManifest(Path),

    #[error("Git returned an error when attempting to autofix the lockfile: {0}")]
    LockfileAutofixGitError(String),

    #[error("The argument folder didn't get created by 'yarn patch'")]
    NotAPatchFolder(Path),

    #[error("Git returned an error when attempting to diff the folders: {0}")]
    DiffFailed(String),

    #[error("No changes found when attempting to diff the folders")]
    EmptyDiff,

    #[error("The lockfile is a v1 lockfile; please first migrate to Yarn Berry then migrate again to Yarn ZPM")]
    LockfileV1Error,

    #[error("[YN0056] Cache entry required but missing for {0:?}.")]
    ImmutableCache(Locator),

    #[error("[YN0056] {} appears to be unused and would be marked for deletion, but the cache is immutable", .0.to_print_string())]
    ImmutableCacheCleanup(Path),

    #[error("[YN0091] Cache path does not exist ({}).", .0.to_print_string())]
    MissingCacheFolder(Path),

    #[error("[YN0080] Request to '{0}' has been blocked because of your configuration settings.")]
    NetworkDisabledError(reqwest::Url),

    #[error("[YN0081] Unsafe http requests must be explicitly whitelisted in your configuration ({}).", .0.host_str().expect("\"http:\" URL should have a host"))]
    UnsafeHttpError(reqwest::Url),

    #[error("Algolia registry error")]
    AlgoliaRegistryError(Arc<reqwest::Error>),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Failed to change the current working directory")]
    FailedToChangeCwd,

    #[error("Failed to retrieve the latest Yarn Classic version")]
    FailedToRetrieveLatestClassicVersion,

    #[error("Format error ({0})")]
    FormatError(#[from] zpm_formats::Error),

    #[error("File parsing error ({0})")]
    FileParsingError(#[from] zpm_parsers::Error),

    #[error("Semver error ({0})")]
    SemverError(#[from] zpm_semver::Error),

    #[error("URL error ({0})")]
    UrlError(#[from] url::ParseError),

    #[error("Invalid ident ({0})")]
    InvalidIdent(String),

    #[error("Workspace profile not found ({0})")]
    WorkspaceProfileNotFound(String),

    #[error("Catalog not found ({0})")]
    CatalogNotFound(String),

    #[error("Catalog entry not found ({0}:{})", .1.to_print_string())]
    CatalogEntryNotFound(String, Ident),

    #[error("Package manifest not found ({})", .0.to_print_string())]
    ManifestNotFound(Path),

    #[error("Package manifest failed to parse ({}): {}", .0.to_print_string(), .1)]
    ManifestParseError(Path, Arc<dyn std::error::Error + Send + Sync>),

    #[error("Invalid descriptor ({0})")]
    InvalidDescriptor(String),

    #[error("Invalid range ({0})")]
    InvalidRange(String),

    #[error("Invalid reference ({0})")]
    InvalidReference(String),

    #[error("Project not found ({p})", p = .0.to_print_string())]
    ProjectNotFound(Path),

    #[error("Invalid value; expected an ident or a locator ({0})")]
    InvalidIdentOrLocator(String),

    #[error("Tag not found ({0})")]
    TagNotFound(String),

    #[error("Package not found ({})", .0.to_print_string())]
    PackageNotFound(Ident),

    #[error("No matching variant found for {}", .0.to_print_string())]
    NoMatchingVariantFound(Locator),

    #[error("No candidates found for {}", .0.to_print_string())]
    NoCandidatesFound(Range),

    #[error("I/O error ({inner})\n\n{}", render_backtrace(backtrace))]
    IoError {
        inner: Arc<std::io::Error>,
        backtrace: Arc<std::backtrace::Backtrace>,
    },

    #[error("Time error: {0}")]
    TimeError(#[from] std::time::SystemTimeError),

    #[error("Chrono error: {0}")]
    ChronoError(#[from] chrono::ParseError),

    #[error("Invalid glob pattern ({0})")]
    InvalidGlob(String),

    #[error("Glob error")]
    GlobError(#[from] globset::Error),

    #[error("Glob walk error")]
    GlobWalkError(#[from] Arc<wax::walk::WalkError>),

    #[error("UTF-8 error")]
    Utf8Error(#[from] Arc<std::str::Utf8Error>),

    #[error("UTF-8 error")]
    Utf8Error2(#[from] std::str::Utf8Error),

    #[error("Non-UTF-8 path")]
    NonUtf8Path,

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

    #[error("Remote error ({0:?})")]
    RemoteRegistryError(Arc<reqwest::Error>),

    #[error("Internal serialization error")]
    InternalSerializationError(#[from] Arc<bincode::error::EncodeError>),

    #[error("Internal serialization error")]
    InternalDeserializationError(#[from] Arc<bincode::error::DecodeError>),

    #[error("An error occured while reading the lockfile from disk")]
    LockfileReadError(Arc<std::io::Error>),

    #[error("An error occured while parsing the lockfile: {0}")]
    LockfileParseError(zpm_parsers::Error),

    #[error("Can't perform this operation without a git root")]
    NoGitRoot,

    #[error("Can't perform this operation with zero base refs specified")]
    NoBaseRefs,

    #[error("No merge base could be found between any of HEAD and {args}", args = .0.join(", "))]
    NoMergeBaseFound(Vec<String>),

    #[error("An error occured while parsing the Yarn Berry lockfile: {0}")]
    LegacyLockfileParseError(Arc<serde_yaml::Error>),

    #[error("An error occured while parsing your configuration: {0}")]
    ConfigurationParseError(Arc<dyn std::error::Error + Send + Sync>),

    #[error("Lockfile generation error: {0}")]
    LockfileGenerationError(zpm_parsers::Error),

    #[error("Incompatible options: {}", .0.join(", "))]
    IncompatibleOptions(Vec<String>),

    #[error("Repository clone failed")]
    RepositoryCloneFailed(String),

    #[error("Repository checkout failed")]
    RepositoryCheckoutFailed(String, String),

    #[error("Invalid Git commit ({0})")]
    InvalidGitCommit(String),

    #[error("Invalid Git branch ({0})")]
    InvalidGitBranch(String),

    #[error("Invalid dedupe strategy ({0})")]
    InvalidDedupeStrategy(String),

    #[error("Invalid Git specifier")]
    InvalidGitSpecifier,

    #[error("Unknown error")]
    UnknownError(Arc<Box<dyn std::error::Error + Send + Sync>>),

    #[error("Invalid tar file path ({0})")]
    InvalidTarFilePath(String),

    #[error("Missing package manifest")]
    MissingPackageManifest,

    #[error("Missing package name")]
    MissingPackageName,

    #[error("We don't know how to infer the package name with only the provided range ({})", .0.to_print_string())]
    UnsufficientLooseDescriptor(Range),

    #[error("Config key not found ({0})")]
    ConfigKeyNotFound(String),

    #[error("Invalid config value for {0} ({1})")]
    InvalidConfigValue(String, String),

    #[error("Package conversion error ({0})")]
    PackageConversionError(Arc<Box<dyn std::error::Error + Send + Sync>>),

    #[error("Workspace not found ({})", .0.to_print_string())]
    WorkspaceNotFound(Ident),

    #[error("Workspace path not found ({})", .0.to_print_string())]
    WorkspacePathNotFound(Path),

    #[error("Constraints configuration file not found")]
    ConstraintsConfigNotFound,

    #[error("Automatic constraints check failed; run {} to obtain details", DataType::Code.colorize("yarn constraints"))]
    AutoConstraintsError,

    #[error("Install state file not found; please run an install operation first")]
    InstallStateNotFound,

    #[error("Invalid install state; please run an install operation to fix it")]
    InvalidInstallState,

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

    #[error("Binary failed to spawn: {2} ({}, in {})", DataType::Code.colorize(.0), .1.to_print_string())]
    SpawnFailed(String, Path, Arc<Box<dyn std::error::Error + Send + Sync>>),

    #[error("No binaries available in the dlx context")]
    MissingBinariesDlxContent,

    #[error("Ambiguous dlx context; use the -p syntax to clarify package and binary names")]
    AmbiguousDlxContext,

    #[error("Circular build dependency detected")]
    CircularBuildDependency(Locator),

    #[error("Some build scripts failed to run")]
    BuildScriptsFailedToRun,

    #[error("Invalid pack pattern ({0})")]
    InvalidPackPattern(String),

    #[error("Invalid url ({0})")]
    InvalidUrl(String),

    #[error("Invalid git url ({0})")]
    InvalidGitUrl(String),

    #[error("Child process failed ({0})")]
    ChildProcessFailed(String),

    #[error("Child process failed ({}); check {} for details", .0, .1.to_print_string())]
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

    #[error("Patched file not found ({})", .0.to_print_string())]
    PatchedFileNotFound(Path),

    #[error("Unmatched hunk")]
    UnmatchedHunk(usize),

    #[error("Invalid resolution")]
    InvalidResolution(String),

    #[error("Bad resolution")]
    BadResolution(Descriptor, Locator),

    #[error("Task timeout")]
    TaskTimeout,

    #[error("Invalid install mode ({0})")]
    InvalidInstallMode(String),

    #[error("Invalid benchmark name ({0}); expected one of: gatsby, monorepo, next")]
    InvalidBenchName(String),

    #[error("Invalid benchmark mode ({0}); expected one of: install-full-cold, install-cache-only, install-cache-and-lock, install-ready")]
    InvalidBenchMode(String),

    #[error("Internal error: Join failed ({0})")]
    JoinFailed(#[from] Arc<JoinError>),

    #[error("Your version of npm doesn't support workspaces")]
    UnsupportedNpmWorkspaces(zpm_semver::Version),

    #[error("Declining a version bump is only allowed when using the `--deferred` flag or when `preferDeferredVersions` is enabled")]
    VersionDeclineNotAllowed,

    #[error("Cannot use {0} as a version bump strategy when using the `--deferred` flag or when `preferDeferredVersions` is enabled and `--immediate` isn't set")]
    InvalidDeferredVersionBump(String),

    #[error("Can't bump the version if there wasn't a version to begin with - use 0.0.0 as initial version then run the command again.")]
    NoVersionFoundForActiveWorkspace,

    #[error("No existing version found for workspace {}", .0.to_print_string())]
    NoVersionFoundForWorkspace(Ident),

    #[error("Recursive version apply is not implemented yet")]
    RecursiveVersionApplyNotImplemented,

    #[error("Can't bump the version to one that would be lower than the current one (trying to bump {} from version {} to version {})", .0.to_print_string(), .1.to_print_string(), .2.to_print_string())]
    VersionBumpLowerThanCurrent(Ident, zpm_semver::Version, zpm_semver::Version),

    #[error("Can't bump the version to one that would be lower than the current deferred one ({})", .0.to_print_string())]
    VersionBumpLowerThanDeferred(zpm_semver::Version),

    #[error("The project doesn't seem to require a version bump.")]
    NoVersionBumpRequiredForProject,

    #[error("No versioning file found")]
    VersioningFileNotFound,

    #[error("Multiple versioning files found")]
    MultipleVersioningFilesFound,

    #[error("Failed to get detected root")]
    FailedToGetSwitchDetectedRoot,

    #[error("The following options of the run command cannot be used when running scripts: {}", .0.join(", "))]
    InvalidRunScriptOptions(Vec<String>),

    #[error("Rustup doesn't seem to be installed; first install it by running {}", DataType::Code.colorize("curl https://sh.rustup.rs | bash"))]
    MissingRustup,

    #[error("Samply doesn't seem to be installed; first install it by running {}", DataType::Code.colorize("curl https://github.com/mstange/samply/releases/download/samply-v0.13.1/samply-installer.sh | sh"))]
    MissingSamply,

    // Silent error; no particular message, just exit with an exit code 1
    #[error("")]
    SilentError,
}

impl Error {
    pub fn ignore<T, F: FnOnce(&Error) -> bool>(self, f: F) -> Result<Option<T>, Error> {
        match f(&self) {
            true => Ok(None),
            false => Err(self),
        }
    }
}

impl From<JoinError> for Error {
    fn from(error: JoinError) -> Self {
        Arc::new(error).into()
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

impl From<wax::walk::WalkError> for Error {
    fn from(error: wax::walk::WalkError) -> Self {
        Arc::new(error).into()
    }
}

impl From<bincode::error::EncodeError> for Error {
    fn from(error: bincode::error::EncodeError) -> Self {
        Arc::new(error).into()
    }
}

impl From<std::convert::Infallible> for Error {
    fn from(_: std::convert::Infallible) -> Self {
        unreachable!()
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::HttpError(Arc::new(error), None)
    }
}
