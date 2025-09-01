use serde::{Deserialize, Deserializer};
use zpm_utils::FromFileString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeLinker {
    Pnp,
    Pnpm,
    NodeModules,
}

impl<'de> Deserialize<'de> for NodeLinker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Self::from_file_string(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnpFallbackMode {
    None,
    DependenciesOnly,
    All,
}

impl<'de> Deserialize<'de> for PnpFallbackMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Self::from_file_string(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
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
