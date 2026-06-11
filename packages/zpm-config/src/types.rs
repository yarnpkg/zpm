use zpm_macro_enum::zpm_enum;

use crate::ConfigurationError;

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeLinker {
    #[literal("pnp")]
    Pnp,

    #[literal("pnpm")]
    Pnpm,

    #[literal("node-modules")]
    NodeModules,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandLinker {
    #[literal("node-modules")]
    NodeModules,

    #[literal("venv")]
    Venv,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnpFallbackMode {
    #[literal("none")]
    None,

    #[literal("dependencies-only")]
    DependenciesOnly,

    #[literal("all")]
    All,
}

/// How far the node-modules linker is allowed to hoist a workspace's
/// dependencies. Mirrors berry's `nmHoistingLimits`.
#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmHoistingLimits {
    /// No limit — packages bubble up to the project root.
    #[literal("none")]
    None,

    /// Dependencies stop at their owning workspace; nothing escapes
    /// past the workspace's own `node_modules/`.
    #[literal("workspaces")]
    Workspaces,

    /// Only direct dependencies hoist up one level; transitive
    /// dependencies stay inside their parent.
    #[literal("dependencies")]
    Dependencies,
}

/// Whether the node-modules linker copies files, hardlinks them per
/// project, or hardlinks them through a shared content-addressed
/// index. Mirrors berry's `nmMode`.
#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmMode {
    #[literal("classic")]
    Classic,

    #[literal("hardlinks-local")]
    HardlinksLocal,

    #[literal("hardlinks-global")]
    HardlinksGlobal,
}

/// On Windows, choose between true symlinks (which require dev mode or
/// admin) and NTFS junctions. On every other platform this setting is
/// a no-op and symlinks are always used.
#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinLinkType {
    #[literal("symlinks")]
    Symlinks,

    #[literal("junctions")]
    Junctions,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    #[literal("discard")]
    Discard,

    #[literal("info")]
    Info,

    #[literal("warning")]
    Warning,

    #[literal("error")]
    ErrorLevel,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpmPublishAccess {
    #[literal("public")]
    Public,

    #[literal("restricted")]
    Restricted,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcosystemFilter {
    #[literal("npm")]
    Npm,

    #[literal("pypi")]
    Pypi,
}
