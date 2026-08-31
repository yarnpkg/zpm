use zpm_macro_enum::zpm_enum;
use zpm_primitives::{PythonTargetEnv, PythonTargetError, PythonTargetInput};
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use zpm_utils::{Cpu, FromFileString, Libc, Os, System, ToFileString, ToHumanString};

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

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum StructuredSettingParseError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PythonTarget {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub full_version: Option<String>,
    #[serde(default)]
    pub implementation_name: Option<String>,
    #[serde(default)]
    pub implementation_version: Option<String>,
    #[serde(default)]
    pub platform_release: Option<String>,
    #[serde(default)]
    pub platform_version: Option<String>,
}

impl PythonTarget {
    pub fn as_input(&self) -> PythonTargetInput<'_> {
        PythonTargetInput {
            version: self.version.as_deref(),
            full_version: self.full_version.as_deref(),
            implementation_name: self.implementation_name.as_deref(),
            implementation_version: self.implementation_version.as_deref(),
            platform_release: self.platform_release.as_deref(),
            platform_version: self.platform_version.as_deref(),
        }
    }
}

impl ToHumanString for PythonTarget {
    fn to_print_string(&self) -> String {
        let mut fields
            = Vec::new();

        if let Some(version) = &self.version {
            fields.push(format!("version={version}"));
        }

        if let Some(full_version) = &self.full_version {
            fields.push(format!("fullVersion={full_version}"));
        }

        if let Some(implementation_name) = &self.implementation_name {
            fields.push(format!("implementationName={implementation_name}"));
        }

        if let Some(implementation_version) = &self.implementation_version {
            fields.push(format!("implementationVersion={implementation_version}"));
        }

        if let Some(platform_release) = &self.platform_release {
            fields.push(format!("platformRelease={platform_release}"));
        }

        if let Some(platform_version) = &self.platform_version {
            fields.push(format!("platformVersion={platform_version}"));
        }

        format!("python{{{}}}", fields.join(", "))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportedTarget {
    #[serde(default)]
    pub cpu: Option<Cpu>,
    #[serde(default)]
    pub libc: Option<Libc>,
    #[serde(default)]
    pub os: Option<Os>,
    #[serde(default)]
    pub python: Option<PythonTarget>,
}

impl SupportedTarget {
    pub fn to_system(&self) -> System {
        let current
            = System::from_current();

        System {
            arch: self.cpu.as_ref().map(|cpu| match cpu {
                Cpu::Current => current.arch.clone(),
                cpu => Some(cpu.clone()),
            }).flatten(),
            os: self.os.as_ref().map(|os| match os {
                Os::Current => current.os.clone(),
                os => Some(os.clone()),
            }).flatten(),
            libc: self.libc.as_ref().map(|libc| match libc {
                Libc::Current => current.libc.clone(),
                libc => Some(libc.clone()),
            }).flatten(),
        }
    }

    pub fn to_python_target_env(&self) -> Result<Option<PythonTargetEnv>, PythonTargetError> {
        let Some(python) = &self.python else {
            return Ok(None);
        };

        Ok(Some(PythonTargetEnv::from_system(&self.to_system(), python.as_input())?))
    }
}

impl ToHumanString for SupportedTarget {
    fn to_print_string(&self) -> String {
        let mut fields
            = Vec::new();

        if let Some(os) = &self.os {
            fields.push(format!("os={}", os.to_file_string()));
        }

        if let Some(cpu) = &self.cpu {
            fields.push(format!("cpu={}", cpu.to_file_string()));
        }

        if let Some(libc) = &self.libc {
            fields.push(format!("libc={}", libc.to_file_string()));
        }

        if let Some(python) = &self.python {
            fields.push(python.to_print_string());
        }

        format!("target{{{}}}", fields.join(", "))
    }
}

impl FromFileString for SupportedTarget {
    type Error = StructuredSettingParseError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        serde_yaml::from_str(src)
            .map_err(|err| StructuredSettingParseError::Message(err.to_string()))
    }
}

impl ToFileString for SupportedTarget {
    fn to_file_string(&self) -> String {
        serde_yaml::to_string(self)
            .unwrap_or_else(|err| panic!("Failed to serialize supported target: {err}"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IslandPython {
    #[serde(default)]
    pub link_version: Option<String>,
}

impl ToHumanString for IslandPython {
    fn to_print_string(&self) -> String {
        match &self.link_version {
            Some(link_version) => format!("python{{linkVersion={link_version}}}"),
            None => "python{}".to_string(),
        }
    }
}

impl FromFileString for IslandPython {
    type Error = StructuredSettingParseError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        serde_yaml::from_str(src)
            .map_err(|err| StructuredSettingParseError::Message(err.to_string()))
    }
}

impl ToFileString for IslandPython {
    fn to_file_string(&self) -> String {
        serde_yaml::to_string(self)
            .unwrap_or_else(|err| panic!("Failed to serialize island python settings: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_target_rejects_unknown_fields() {
        let err
            = serde_yaml::from_str::<SupportedTarget>("os: linux\nunknown: true\n")
                .unwrap_err();

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn test_python_target_rejects_unknown_fields() {
        let err
            = serde_yaml::from_str::<SupportedTarget>("python:\n  version: '3.12'\n  unknown: true\n")
                .unwrap_err();

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn test_island_python_rejects_unknown_fields() {
        let err
            = serde_yaml::from_str::<IslandPython>("linkVersion: '3.12'\nunknown: true\n")
                .unwrap_err();

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn test_supported_target_to_python_target_env() {
        let target: SupportedTarget
            = serde_yaml::from_str("os: linux\ncpu: x64\nlibc: glibc\npython:\n  version: '3.12'\n").unwrap();
        let target_env
            = target.to_python_target_env().unwrap().unwrap();

        assert_eq!(target_env.python_version, "3.12");
        assert_eq!(target_env.sys_platform.as_deref(), Some("linux"));
        assert_eq!(target_env.platform_machine.as_deref(), Some("x86_64"));
        assert_eq!(target_env.libc.as_deref(), Some("glibc"));
    }
}

#[zpm_enum(error = ConfigurationError, or_else = |s| Err(ConfigurationError::EnumError(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcosystemFilter {
    #[literal("npm")]
    Npm,

    #[literal("pypi")]
    Pypi,
}
