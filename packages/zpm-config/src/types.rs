use bincode::{Decode, Encode};
use zpm_macro_enum::zpm_enum;

use crate::ConfigurationError;

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Eq)]
pub enum NodeLinker {
    #[literal("pnp")]
    Pnp,

    #[literal("pnpm")]
    Pnpm,

    #[literal("node-modules")]
    NodeModules,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Eq)]
pub enum PnpFallbackMode {
    #[literal("none")]
    None,

    #[literal("dependencies-only")]
    DependenciesOnly,

    #[literal("all")]
    All,
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub enum Cpu {
    #[literal("current")]
    Current,

    #[literal("ia32")]
    I386,

    #[literal("x64")]
    X86_64,

    #[literal("arm64")]
    Aarch64,

    #[fallback]
    Other(String),
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub enum Libc {
    #[literal("current")]
    Current,

    #[literal("glibc")]
    Glibc,

    #[literal("musl")]
    Musl,

    #[fallback]
    Other(String),
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub enum Os {
    #[literal("current")]
    Current,

    #[literal("darwin")]
    MacOS,

    #[literal("linux")]
    Linux,

    #[literal("win32")]
    Windows,

    #[fallback]
    Other(String),
}
