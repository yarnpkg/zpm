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
pub enum PnpFallbackMode {
    #[literal("none")]
    None,

    #[literal("dependencies-only")]
    DependenciesOnly,

    #[literal("all")]
    All,
}
