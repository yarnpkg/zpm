use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use zpm_macro_enum::zpm_enum;
use zpm_utils::{FromFileString, ToFileString, ToHumanString};

use crate::{ConfigurationError, Interpolated};

/// One field of a `supportedArchitectures` entry. It can be set to a single
/// value, to a list of values, or to `null` (in which case every value is
/// supported - as opposed to an empty list, which supports none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchitectureFilter<T> {
    Any,
    List(Vec<T>),
}

impl<T> ArchitectureFilter<T> {
    /// The supported values, or `None` if the filter accepts them all.
    pub fn as_list(&self) -> Option<&[T]> {
        match self {
            ArchitectureFilter::Any => None,
            ArchitectureFilter::List(values) => Some(values),
        }
    }
}

impl<T> Default for ArchitectureFilter<T> {
    fn default() -> Self {
        ArchitectureFilter::List(Vec::new())
    }
}

impl<T: FromFileString> FromFileString for ArchitectureFilter<T> {
    type Error = <T as FromFileString>::Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        if s == "null" {
            return Ok(ArchitectureFilter::Any);
        }

        if s.is_empty() {
            return Ok(ArchitectureFilter::List(Vec::new()));
        }

        let values = s.split(',')
            .map(|segment| T::from_file_string(segment.trim()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ArchitectureFilter::List(values))
    }
}

impl<T: ToFileString> ToFileString for ArchitectureFilter<T> {
    fn to_file_string(&self) -> String {
        match self {
            ArchitectureFilter::Any => "null".to_string(),
            ArchitectureFilter::List(values) => values.iter()
                .map(|value| value.to_file_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    }
}

impl<T: ToHumanString> ToHumanString for ArchitectureFilter<T> {
    fn to_print_string(&self) -> String {
        match self {
            ArchitectureFilter::Any => "null".to_string(),
            ArchitectureFilter::List(values) => values.iter()
                .map(|value| value.to_print_string())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

impl<T: Serialize> Serialize for ArchitectureFilter<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ArchitectureFilter::Any => serializer.serialize_none(),
            ArchitectureFilter::List(values) => values.serialize(serializer),
        }
    }
}

struct ArchitectureFilterVisitor<T> {
    marker: PhantomData<T>,
}

impl<'de, T> de::Visitor<'de> for ArchitectureFilterVisitor<T>
    where T: FromFileString + Deserialize<'de>, <T as FromFileString>::Error: fmt::Display
{
    type Value = ArchitectureFilter<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string, a list of strings, or null")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(ArchitectureFilter::Any)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(ArchitectureFilter::Any)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        ArchitectureFilter::from_file_string(value)
            .map_err(de::Error::custom)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut values
            = Vec::new();

        // We go through `Interpolated` so that each item can reference
        // environment variables, just like any other setting.
        while let Some(value) = seq.next_element::<Interpolated<T>>()? {
            values.push(value.into_inner());
        }

        Ok(ArchitectureFilter::List(values))
    }
}

impl<'de, T> Deserialize<'de> for ArchitectureFilter<T>
    where T: FromFileString + Deserialize<'de>, <T as FromFileString>::Error: fmt::Display
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ArchitectureFilterVisitor {marker: PhantomData})
    }
}

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
pub enum NodePackageMapType {
    #[literal("standard")]
    Standard,

    #[literal("loose")]
    Loose,
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

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LazyInstallMode {
    #[literal("focused")]
    Focused,

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
