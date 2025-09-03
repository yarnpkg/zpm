use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, FromFileString, ToFileString, ToHumanString};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeLinker {
    Pnp,
    Pnpm,
    NodeModules,
}

impl FromFileString for NodeLinker {
    type Error = String;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        match s {
            "pnp" => Ok(NodeLinker::Pnp),
            "pnpm" => Ok(NodeLinker::Pnpm),
            "node-modules" => Ok(NodeLinker::NodeModules),
            _ => Err(format!("Invalid node linker: {}", s)),
        }
    }
}

impl ToFileString for NodeLinker {
    fn to_file_string(&self) -> String {
        match self {
            NodeLinker::Pnp
                => "pnp".to_string(),

            NodeLinker::Pnpm
                => "pnpm".to_string(),

            NodeLinker::NodeModules
                => "node-modules".to_string(),
        }
    }
}

impl ToHumanString for NodeLinker {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_file_string_from_str!(NodeLinker);
impl_file_string_serialization!(NodeLinker);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnpFallbackMode {
    None,
    DependenciesOnly,
    All,
}

impl FromFileString for PnpFallbackMode {
    type Error = String;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        match s {
            "none" => Ok(Self::None),
            "dependencies-only" => Ok(Self::DependenciesOnly),
            "all" => Ok(Self::All),
            _ => Err(format!("Invalid PnP fallback mode: {}", s)),
        }
    }
}

impl ToFileString for PnpFallbackMode {
    fn to_file_string(&self) -> String {
        match self {
            PnpFallbackMode::None => "none".to_string(),
            PnpFallbackMode::DependenciesOnly => "dependencies-only".to_string(),
            PnpFallbackMode::All => "all".to_string(),
        }
    }
}

impl ToHumanString for PnpFallbackMode {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_file_string_from_str!(PnpFallbackMode);
impl_file_string_serialization!(PnpFallbackMode);
