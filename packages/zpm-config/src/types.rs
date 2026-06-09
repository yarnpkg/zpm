use zpm_macro_enum::zpm_enum;
use serde::{Deserialize, Serialize};
use zpm_primitives::{PythonTargetEnv, PythonTargetError, PythonTargetInput};
use zpm_utils::{Cpu, FromFileString, Libc, Os, System, ToFileString, ToHumanString};

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
    fn test_supported_target_to_python_target_env() {
        let target: SupportedTarget
            = serde_yaml::from_str("os: linux\ncpu: x64\npython:\n  version: '3.12'\n").unwrap();
        let target_env
            = target.to_python_target_env().unwrap().unwrap();

        assert_eq!(target_env.python_version, "3.12");
        assert_eq!(target_env.sys_platform.as_deref(), Some("linux"));
        assert_eq!(target_env.platform_machine.as_deref(), Some("x86_64"));
    }
}
