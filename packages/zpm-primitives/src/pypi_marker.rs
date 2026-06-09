use std::{borrow::Cow, cmp::Ordering, str::FromStr};

use rkyv::Archive;
use serde::{Deserialize, Serialize};
use zpm_utils::{Cpu, Hash64, Os, System, ToFileString};

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum PythonTargetError {
    #[error("Python target is missing python.version")]
    MissingPythonVersion,

    #[error("Unsupported Python target system field {field}: {value}")]
    UnsupportedSystemValue {
        field: &'static str,
        value: String,
    },

    #[error("Invalid Python target version in {field}: {value}")]
    InvalidVersion {
        field: &'static str,
        value: String,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PythonTargetInput<'a> {
    pub version: Option<&'a str>,
    pub full_version: Option<&'a str>,
    pub implementation_name: Option<&'a str>,
    pub implementation_version: Option<&'a str>,
    pub platform_release: Option<&'a str>,
    pub platform_version: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct PythonTargetEnv {
    pub python_version: String,
    pub python_full_version: Option<String>,
    pub os_name: Option<String>,
    pub sys_platform: Option<String>,
    pub platform_machine: Option<String>,
    pub platform_system: Option<String>,
    pub platform_release: Option<String>,
    pub platform_version: Option<String>,
    pub platform_python_implementation: Option<String>,
    pub implementation_name: Option<String>,
    pub implementation_version: Option<String>,
}

impl PythonTargetEnv {
    pub fn from_system(system: &System, python: PythonTargetInput<'_>) -> Result<Self, PythonTargetError> {
        let configured_python_version
            = python.version.ok_or(PythonTargetError::MissingPythonVersion)?;
        let (python_version, default_python_full_version)
            = normalize_python_version(configured_python_version, "python.version")?;

        let python_full_version
            = match python.full_version {
                Some(full_version) => normalize_full_python_version(full_version, "python.fullVersion")?,
                None => default_python_full_version,
            };

        let implementation_name
            = python.implementation_name.unwrap_or("cpython");
        let implementation_version
            = python.implementation_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| python_full_version.clone());

        let (os_name, sys_platform, platform_system)
            = python_os_fields(system.os.as_ref())?;
        let platform_machine
            = python_platform_machine(system.arch.as_ref(), system.os.as_ref())?;

        Ok(Self {
            python_version,
            python_full_version: Some(python_full_version),
            os_name,
            sys_platform,
            platform_machine,
            platform_system,
            platform_release: python.platform_release.map(|value| value.to_string()),
            platform_version: python.platform_version.map(|value| value.to_string()),
            platform_python_implementation: Some(platform_python_implementation(implementation_name)),
            implementation_name: Some(implementation_name.to_string()),
            implementation_version: Some(implementation_version),
        })
    }

    pub fn fork_id(&self) -> Hash64 {
        Hash64::from_data(format!("python-target-v1\n{}", self.to_file_string()))
    }

    pub fn to_exact_marker_expr(&self) -> MarkerExpr {
        let mut expr
            = MarkerExpr::Any;

        for (variable, value) in [
            (MarkerVariable::PythonVersion, Some(self.python_version.as_str())),
            (MarkerVariable::PythonFullVersion, self.python_full_version.as_deref()),
            (MarkerVariable::OsName, self.os_name.as_deref()),
            (MarkerVariable::SysPlatform, self.sys_platform.as_deref()),
            (MarkerVariable::PlatformMachine, self.platform_machine.as_deref()),
            (MarkerVariable::PlatformSystem, self.platform_system.as_deref()),
            (MarkerVariable::PlatformRelease, self.platform_release.as_deref()),
            (MarkerVariable::PlatformVersion, self.platform_version.as_deref()),
            (MarkerVariable::PlatformPythonImplementation, self.platform_python_implementation.as_deref()),
            (MarkerVariable::ImplementationName, self.implementation_name.as_deref()),
            (MarkerVariable::ImplementationVersion, self.implementation_version.as_deref()),
        ] {
            let Some(value) = value else {
                continue;
            };

            let comparison = MarkerExpr::Compare {
                lhs: MarkerValue::Variable(variable),
                op: MarkerOp::Eq,
                rhs: MarkerValue::String(value.to_string()),
            };

            expr = expr.and(comparison);
        }

        expr
    }

    fn get(&self, variable: MarkerVariable) -> Option<&str> {
        match variable {
            MarkerVariable::PythonVersion => Some(&self.python_version),
            MarkerVariable::PythonFullVersion => self.python_full_version.as_deref(),
            MarkerVariable::OsName => self.os_name.as_deref(),
            MarkerVariable::SysPlatform => self.sys_platform.as_deref(),
            MarkerVariable::PlatformMachine => self.platform_machine.as_deref(),
            MarkerVariable::PlatformSystem => self.platform_system.as_deref(),
            MarkerVariable::PlatformRelease => self.platform_release.as_deref(),
            MarkerVariable::PlatformVersion => self.platform_version.as_deref(),
            MarkerVariable::PlatformPythonImplementation => self.platform_python_implementation.as_deref(),
            MarkerVariable::ImplementationName => self.implementation_name.as_deref(),
            MarkerVariable::ImplementationVersion => self.implementation_version.as_deref(),
            MarkerVariable::Extra => None,
        }
    }
}

impl ToFileString for PythonTargetEnv {
    fn to_file_string(&self) -> String {
        let mut out
            = String::new();

        write_canonical_field(&mut out, "python_version", Some(&self.python_version));
        write_canonical_field(&mut out, "python_full_version", self.python_full_version.as_ref());
        write_canonical_field(&mut out, "os_name", self.os_name.as_ref());
        write_canonical_field(&mut out, "sys_platform", self.sys_platform.as_ref());
        write_canonical_field(&mut out, "platform_machine", self.platform_machine.as_ref());
        write_canonical_field(&mut out, "platform_system", self.platform_system.as_ref());
        write_canonical_field(&mut out, "platform_release", self.platform_release.as_ref());
        write_canonical_field(&mut out, "platform_version", self.platform_version.as_ref());
        write_canonical_field(&mut out, "platform_python_implementation", self.platform_python_implementation.as_ref());
        write_canonical_field(&mut out, "implementation_name", self.implementation_name.as_ref());
        write_canonical_field(&mut out, "implementation_version", self.implementation_version.as_ref());

        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PythonFork {
    pub id: Hash64,
    pub condition: MarkerExpr,
    pub target: Option<PythonTargetEnv>,
}

impl PythonFork {
    pub fn from_target(target: PythonTargetEnv) -> Self {
        Self {
            id: target.fork_id(),
            condition: target.to_exact_marker_expr(),
            target: Some(target),
        }
    }
}

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum MarkerError {
    #[error("Invalid PEP 508 marker: {0}")]
    ParseError(String),

    #[error("Unsupported PEP 508 marker operator: {0}")]
    UnsupportedOperator(String),

    #[error("Marker target field {0} is unavailable for this Python target")]
    MissingTargetField(&'static str),

    #[error("Invalid PEP 440 marker version for {field}: {value}")]
    InvalidVersion {
        field: &'static str,
        value: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub enum MarkerVariable {
    PythonVersion,
    PythonFullVersion,
    OsName,
    SysPlatform,
    PlatformMachine,
    PlatformSystem,
    PlatformRelease,
    PlatformVersion,
    PlatformPythonImplementation,
    ImplementationName,
    ImplementationVersion,
    Extra,
}

impl MarkerVariable {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarkerVariable::PythonVersion => "python_version",
            MarkerVariable::PythonFullVersion => "python_full_version",
            MarkerVariable::OsName => "os_name",
            MarkerVariable::SysPlatform => "sys_platform",
            MarkerVariable::PlatformMachine => "platform_machine",
            MarkerVariable::PlatformSystem => "platform_system",
            MarkerVariable::PlatformRelease => "platform_release",
            MarkerVariable::PlatformVersion => "platform_version",
            MarkerVariable::PlatformPythonImplementation => "platform_python_implementation",
            MarkerVariable::ImplementationName => "implementation_name",
            MarkerVariable::ImplementationVersion => "implementation_version",
            MarkerVariable::Extra => "extra",
        }
    }

    fn uses_version_comparison(&self) -> bool {
        matches!(
            self,
            MarkerVariable::PythonVersion | MarkerVariable::PythonFullVersion | MarkerVariable::ImplementationVersion
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub enum MarkerOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    NotIn,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub enum MarkerValue {
    Variable(MarkerVariable),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MarkerExpr {
    Any,
    Never,
    And {
        lhs: Box<MarkerExpr>,
        rhs: Box<MarkerExpr>,
    },
    Or {
        lhs: Box<MarkerExpr>,
        rhs: Box<MarkerExpr>,
    },
    Not {
        expr: Box<MarkerExpr>,
    },
    Compare {
        lhs: MarkerValue,
        op: MarkerOp,
        rhs: MarkerValue,
    },
}

impl MarkerExpr {
    pub fn and(self, rhs: MarkerExpr) -> MarkerExpr {
        match (self, rhs) {
            (MarkerExpr::Any, rhs) => rhs,
            (lhs, MarkerExpr::Any) => lhs,
            (MarkerExpr::Never, _) | (_, MarkerExpr::Never) => MarkerExpr::Never,
            (lhs, rhs) => MarkerExpr::And {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        }
    }

    pub fn evaluate(&self, target: &PythonTargetEnv) -> Result<bool, MarkerError> {
        match self {
            MarkerExpr::Any => Ok(true),
            MarkerExpr::Never => Ok(false),
            MarkerExpr::And {lhs, rhs} => {
                if !lhs.evaluate(target)? {
                    return Ok(false);
                }

                rhs.evaluate(target)
            },
            MarkerExpr::Or {lhs, rhs} => {
                if lhs.evaluate(target)? {
                    return Ok(true);
                }

                rhs.evaluate(target)
            },
            MarkerExpr::Not {expr} => {
                Ok(!expr.evaluate(target)?)
            },
            MarkerExpr::Compare {lhs, op, rhs} => {
                evaluate_marker_comparison(lhs, *op, rhs, target)
            },
        }
    }

    pub fn from_pep508_marker(marker: &pep_508::Marker<'_>) -> Result<Self, MarkerError> {
        match marker {
            pep_508::Marker::And(lhs, rhs) => Ok(Self::And {
                lhs: Box::new(Self::from_pep508_marker(lhs)?),
                rhs: Box::new(Self::from_pep508_marker(rhs)?),
            }),
            pep_508::Marker::Or(lhs, rhs) => Ok(Self::Or {
                lhs: Box::new(Self::from_pep508_marker(lhs)?),
                rhs: Box::new(Self::from_pep508_marker(rhs)?),
            }),
            pep_508::Marker::Operator(lhs, op, rhs) => Ok(Self::Compare {
                lhs: MarkerValue::from_pep508_variable(lhs),
                op: MarkerOp::from_pep508_operator(*op)?,
                rhs: MarkerValue::from_pep508_variable(rhs),
            }),
        }
    }

    pub fn from_pep508_requirement(requirement: &str) -> Result<Self, MarkerError> {
        let dependency
            = pep_508::parse(requirement)
                .map_err(|errors| MarkerError::ParseError(format!("{errors:?}")))?;

        dependency.marker.as_ref()
            .map(Self::from_pep508_marker)
            .unwrap_or(Ok(Self::Any))
    }
}

impl MarkerValue {
    fn from_pep508_variable(variable: &pep_508::Variable<'_>) -> Self {
        match variable {
            pep_508::Variable::PythonVersion => Self::Variable(MarkerVariable::PythonVersion),
            pep_508::Variable::PythonFullVersion => Self::Variable(MarkerVariable::PythonFullVersion),
            pep_508::Variable::OsName => Self::Variable(MarkerVariable::OsName),
            pep_508::Variable::SysPlatform => Self::Variable(MarkerVariable::SysPlatform),
            pep_508::Variable::PlatformRelease => Self::Variable(MarkerVariable::PlatformRelease),
            pep_508::Variable::PlatformSystem => Self::Variable(MarkerVariable::PlatformSystem),
            pep_508::Variable::PlatformVersion => Self::Variable(MarkerVariable::PlatformVersion),
            pep_508::Variable::PlatformMachine => Self::Variable(MarkerVariable::PlatformMachine),
            pep_508::Variable::PlatformPythonImplementation => Self::Variable(MarkerVariable::PlatformPythonImplementation),
            pep_508::Variable::ImplementationName => Self::Variable(MarkerVariable::ImplementationName),
            pep_508::Variable::ImplementationVersion => Self::Variable(MarkerVariable::ImplementationVersion),
            pep_508::Variable::Extra => Self::Variable(MarkerVariable::Extra),
            pep_508::Variable::String(value) => Self::String(value.to_string()),
        }
    }

    fn evaluate<'a>(&'a self, target: &'a PythonTargetEnv) -> Result<EvaluatedMarkerValue<'a>, MarkerError> {
        match self {
            MarkerValue::String(value) => Ok(EvaluatedMarkerValue {
                value: Cow::Borrowed(value),
                variable: None,
            }),
            MarkerValue::Variable(variable) => {
                let value
                    = target.get(*variable)
                        .ok_or_else(|| MarkerError::MissingTargetField(variable.as_str()))?;

                Ok(EvaluatedMarkerValue {
                    value: Cow::Borrowed(value),
                    variable: Some(*variable),
                })
            },
        }
    }
}

impl MarkerOp {
    fn from_pep508_operator(op: pep_508::Operator) -> Result<Self, MarkerError> {
        match op {
            pep_508::Operator::Comparator(pep_508::Comparator::Eq) => Ok(Self::Eq),
            pep_508::Operator::Comparator(pep_508::Comparator::Ne) => Ok(Self::NotEq),
            pep_508::Operator::Comparator(pep_508::Comparator::Lt) => Ok(Self::Lt),
            pep_508::Operator::Comparator(pep_508::Comparator::Le) => Ok(Self::Lte),
            pep_508::Operator::Comparator(pep_508::Comparator::Gt) => Ok(Self::Gt),
            pep_508::Operator::Comparator(pep_508::Comparator::Ge) => Ok(Self::Gte),
            pep_508::Operator::Comparator(pep_508::Comparator::Cp) => Err(MarkerError::UnsupportedOperator("~=".to_string())),
            pep_508::Operator::Comparator(pep_508::Comparator::Ae) => Err(MarkerError::UnsupportedOperator("===".to_string())),
            pep_508::Operator::In => Ok(Self::In),
            pep_508::Operator::NotIn => Ok(Self::NotIn),
        }
    }
}

struct EvaluatedMarkerValue<'a> {
    value: Cow<'a, str>,
    variable: Option<MarkerVariable>,
}

fn evaluate_marker_comparison(lhs: &MarkerValue, op: MarkerOp, rhs: &MarkerValue, target: &PythonTargetEnv) -> Result<bool, MarkerError> {
    let lhs
        = lhs.evaluate(target)?;
    let rhs
        = rhs.evaluate(target)?;

    match op {
        MarkerOp::In => Ok(rhs.value.contains(lhs.value.as_ref())),
        MarkerOp::NotIn => Ok(!rhs.value.contains(lhs.value.as_ref())),
        MarkerOp::Eq | MarkerOp::NotEq | MarkerOp::Lt | MarkerOp::Lte | MarkerOp::Gt | MarkerOp::Gte => {
            let ordering
                = if lhs.variable.map_or(false, |variable| variable.uses_version_comparison())
                    || rhs.variable.map_or(false, |variable| variable.uses_version_comparison()) {
                    compare_marker_versions(&lhs, &rhs)?
                } else {
                    lhs.value.as_ref().cmp(rhs.value.as_ref())
                };

            Ok(match op {
                MarkerOp::Eq => ordering == Ordering::Equal,
                MarkerOp::NotEq => ordering != Ordering::Equal,
                MarkerOp::Lt => ordering == Ordering::Less,
                MarkerOp::Lte => ordering != Ordering::Greater,
                MarkerOp::Gt => ordering == Ordering::Greater,
                MarkerOp::Gte => ordering != Ordering::Less,
                MarkerOp::In | MarkerOp::NotIn => unreachable!(),
            })
        },
    }
}

fn compare_marker_versions(lhs: &EvaluatedMarkerValue<'_>, rhs: &EvaluatedMarkerValue<'_>) -> Result<Ordering, MarkerError> {
    let lhs_version
        = parse_marker_version(lhs)?;
    let rhs_version
        = parse_marker_version(rhs)?;

    Ok(lhs_version.cmp(&rhs_version))
}

fn parse_marker_version(value: &EvaluatedMarkerValue<'_>) -> Result<pep440_rs::Version, MarkerError> {
    let field
        = value.variable.map_or("literal", |variable| variable.as_str());

    pep440_rs::Version::from_str(value.value.as_ref())
        .map_err(|_| MarkerError::InvalidVersion {
            field,
            value: value.value.to_string(),
        })
}

fn python_os_fields(os: Option<&Os>) -> Result<(Option<String>, Option<String>, Option<String>), PythonTargetError> {
    let Some(os) = os else {
        return Ok((None, None, None));
    };

    match os {
        Os::Linux => Ok((Some("posix".to_string()), Some("linux".to_string()), Some("Linux".to_string()))),
        Os::MacOS => Ok((Some("posix".to_string()), Some("darwin".to_string()), Some("Darwin".to_string()))),
        Os::Windows => Ok((Some("nt".to_string()), Some("win32".to_string()), Some("Windows".to_string()))),
        Os::Current | Os::Other(_) => Err(PythonTargetError::UnsupportedSystemValue {
            field: "os",
            value: os.to_file_string(),
        }),
    }
}

fn python_platform_machine(arch: Option<&Cpu>, os: Option<&Os>) -> Result<Option<String>, PythonTargetError> {
    let Some(arch) = arch else {
        return Ok(None);
    };

    match arch {
        Cpu::X86_64 => Ok(Some("x86_64".to_string())),
        Cpu::Aarch64 if matches!(os, Some(Os::MacOS)) => Ok(Some("arm64".to_string())),
        Cpu::Aarch64 => Ok(Some("aarch64".to_string())),
        Cpu::I386 => Ok(Some("i386".to_string())),
        Cpu::Current | Cpu::Other(_) => Err(PythonTargetError::UnsupportedSystemValue {
            field: "cpu",
            value: arch.to_file_string(),
        }),
    }
}

fn platform_python_implementation(implementation_name: &str) -> String {
    match implementation_name {
        "cpython" => "CPython".to_string(),
        "pypy" => "PyPy".to_string(),
        other => other.to_string(),
    }
}

fn normalize_python_version(version: &str, field: &'static str) -> Result<(String, String), PythonTargetError> {
    let full_version
        = normalize_full_python_version(version, field)?;
    let parsed
        = pep440_rs::Version::from_str(&full_version)
            .map_err(|_| PythonTargetError::InvalidVersion {
                field,
                value: version.to_string(),
            })?;
    let release
        = parsed.release();
    let major
        = release.first()
            .ok_or_else(|| PythonTargetError::InvalidVersion {
                field,
                value: version.to_string(),
            })?;
    let minor
        = release.get(1)
            .ok_or_else(|| PythonTargetError::InvalidVersion {
                field,
                value: version.to_string(),
            })?;

    Ok((format!("{major}.{minor}"), full_version))
}

fn normalize_full_python_version(version: &str, field: &'static str) -> Result<String, PythonTargetError> {
    pep440_rs::Version::from_str(version)
        .map(|version| version.to_string())
        .map_err(|_| PythonTargetError::InvalidVersion {
            field,
            value: version.to_string(),
        })
}

fn write_canonical_field(out: &mut String, name: &str, value: Option<&String>) {
    out.push_str(name);
    out.push('=');

    match value {
        Some(value) => {
            out.push_str(&value.len().to_string());
            out.push(':');
            out.push_str(value);
        },
        None => {
            out.push('-');
        },
    }

    out.push(';');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_target() -> PythonTargetEnv {
        PythonTargetEnv::from_system(
            &System::new(Some(Cpu::X86_64), Some(Os::Linux), None),
            PythonTargetInput {
                version: Some("3.12"),
                ..PythonTargetInput::default()
            },
        ).unwrap()
    }

    #[test]
    fn test_python_target_from_system() {
        let target
            = linux_target();

        assert_eq!(target.python_version, "3.12");
        assert_eq!(target.python_full_version.as_deref(), Some("3.12"));
        assert_eq!(target.os_name.as_deref(), Some("posix"));
        assert_eq!(target.sys_platform.as_deref(), Some("linux"));
        assert_eq!(target.platform_machine.as_deref(), Some("x86_64"));
        assert_eq!(target.platform_python_implementation.as_deref(), Some("CPython"));
        assert_eq!(target.implementation_name.as_deref(), Some("cpython"));
    }

    #[test]
    fn test_python_version_uses_pep508_major_minor() {
        let target
            = PythonTargetEnv::from_system(
                &System::new(Some(Cpu::X86_64), Some(Os::Linux), None),
                PythonTargetInput {
                    version: Some("3.12.2"),
                    ..PythonTargetInput::default()
                },
            ).unwrap();

        assert_eq!(target.python_version, "3.12");
        assert_eq!(target.python_full_version.as_deref(), Some("3.12.2"));
        assert_eq!(target.implementation_version.as_deref(), Some("3.12.2"));
    }

    #[test]
    fn test_marker_evaluation() {
        let marker
            = MarkerExpr::from_pep508_requirement("foo>=1; python_version >= '3.11' and sys_platform == 'linux'").unwrap();

        assert!(marker.evaluate(&linux_target()).unwrap());
    }

    #[test]
    fn test_marker_missing_field_errors() {
        let marker
            = MarkerExpr::from_pep508_requirement("foo; platform_release == 'example'").unwrap();

        assert_eq!(
            marker.evaluate(&linux_target()).unwrap_err(),
            MarkerError::MissingTargetField("platform_release"),
        );
    }

    #[test]
    fn test_fork_id_is_canonical() {
        let a
            = linux_target();
        let b
            = linux_target();

        assert_eq!(a.fork_id(), b.fork_id());
    }
}
