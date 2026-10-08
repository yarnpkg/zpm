//! Python target environments: PEP 508 marker evaluation and wheel tag
//! compatibility.
//!
//! An island resolves its PyPI dependencies once for a set of concrete
//! environments (one Python version, several platforms). Markers are
//! evaluated against all of them: a requirement that's true everywhere
//! becomes a regular dependency, one that's false everywhere is dropped,
//! and one that's only true on some platforms is kept with its marker so
//! the venv linker can skip it where it doesn't apply.

use std::{cmp::Ordering, str::FromStr};

use zpm_config::Configuration;
use zpm_utils::{Cpu, Libc, Os, Requirements, System, SystemSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Platform {
    pub os: Os,
    pub cpu: Cpu,
    pub libc: Option<Libc>,
}

impl Platform {
    pub fn current() -> Option<Platform> {
        let system
            = System::from_current();

        let platform = Platform {
            os: system.os?,
            cpu: system.arch?,
            libc: system.libc,
        };

        Some(platform.normalized())
    }

    fn normalized(mut self) -> Platform {
        if self.os != Os::Linux {
            self.libc = None;
        } else if self.libc.is_none() {
            self.libc = Some(Libc::Glibc);
        }

        self
    }

    pub fn to_requirements(&self) -> Requirements {
        System::new(Some(self.cpu.clone()), Some(self.os.clone()), self.libc.clone())
            .to_requirements()
    }

    pub fn matches_set(&self, set: &SystemSet) -> bool {
        fn check<T: PartialEq>(value: Option<&T>, supported: &Option<Vec<T>>) -> bool {
            match (value, supported) {
                (_, None) => true,
                (Some(value), Some(supported)) => supported.contains(value),
                (None, Some(_)) => true,
            }
        }

        check(Some(&self.os), &set.os)
            && check(Some(&self.cpu), &set.arch)
            && check(self.libc.as_ref(), &set.libc)
    }

    pub fn sys_platform(&self) -> &'static str {
        match self.os {
            Os::MacOS => "darwin",
            Os::Windows => "win32",
            _ => "linux",
        }
    }

    pub fn platform_system(&self) -> &'static str {
        match self.os {
            Os::MacOS => "Darwin",
            Os::Windows => "Windows",
            _ => "Linux",
        }
    }

    pub fn os_name(&self) -> &'static str {
        match self.os {
            Os::Windows => "nt",
            _ => "posix",
        }
    }

    pub fn platform_machine(&self) -> &'static str {
        match (&self.os, &self.cpu) {
            (Os::MacOS, Cpu::Aarch64) => "arm64",
            (Os::Windows, Cpu::Aarch64) => "ARM64",
            (Os::Windows, Cpu::X86_64) => "AMD64",
            (_, Cpu::Aarch64) => "aarch64",
            (_, Cpu::I386) => "i686",
            _ => "x86_64",
        }
    }

    /// The python-build-standalone target triple for this platform.
    pub fn standalone_triple(&self) -> Option<&'static str> {
        match (&self.os, &self.cpu, &self.libc) {
            (Os::MacOS, Cpu::Aarch64, _) => Some("aarch64-apple-darwin"),
            (Os::MacOS, Cpu::X86_64, _) => Some("x86_64-apple-darwin"),
            (Os::Linux, Cpu::X86_64, Some(Libc::Musl)) => Some("x86_64-unknown-linux-musl"),
            (Os::Linux, Cpu::Aarch64, Some(Libc::Musl)) => Some("aarch64-unknown-linux-musl"),
            (Os::Linux, Cpu::X86_64, _) => Some("x86_64-unknown-linux-gnu"),
            (Os::Linux, Cpu::Aarch64, _) => Some("aarch64-unknown-linux-gnu"),
            (Os::Windows, Cpu::X86_64, _) => Some("x86_64-pc-windows-msvc"),
            _ => None,
        }
    }
}

/// The platforms Yarn knows how to select wheels for. `supportedArchitectures`
/// is projected on this list to obtain the platforms an island targets.
pub fn known_platforms() -> Vec<Platform> {
    vec![
        Platform {os: Os::MacOS, cpu: Cpu::Aarch64, libc: None},
        Platform {os: Os::MacOS, cpu: Cpu::X86_64, libc: None},
        Platform {os: Os::Linux, cpu: Cpu::X86_64, libc: Some(Libc::Glibc)},
        Platform {os: Os::Linux, cpu: Cpu::Aarch64, libc: Some(Libc::Glibc)},
        Platform {os: Os::Linux, cpu: Cpu::X86_64, libc: Some(Libc::Musl)},
        Platform {os: Os::Linux, cpu: Cpu::Aarch64, libc: Some(Libc::Musl)},
        Platform {os: Os::Windows, cpu: Cpu::X86_64, libc: None},
    ]
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PythonVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: Option<u64>,
}

impl PythonVersion {
    pub fn parse(src: &str) -> Option<PythonVersion> {
        let mut parts
            = src.trim().split('.');

        let major
            = parts.next()?.parse().ok()?;
        let minor
            = parts.next()?.parse().ok()?;
        let patch
            = parts.next().and_then(|patch| patch.parse().ok());

        Some(PythonVersion {major, minor, patch})
    }

    pub fn short(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    pub fn full(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch.unwrap_or(0))
    }

    /// Version string for marker evaluation. When the exact patch is unknown,
    /// assumes a recent patch (999) to avoid incorrectly filtering dependencies.
    pub fn full_for_markers(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch.unwrap_or(999))
    }

    pub fn pep440(&self) -> pep440_rs::Version {
        pep440_rs::Version::from_str(&self.full()).unwrap()
    }
}

/// A concrete environment markers are evaluated against.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PythonEnv {
    pub python: PythonVersion,
    pub platform: Platform,
}

/// The set of environments an island resolves for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonTargets {
    pub python: PythonVersion,
    pub envs: Vec<PythonEnv>,
    pub macos_target: (u64, u64),
    pub manylinux_target: (u64, u64),
}

impl PythonTargets {
    pub fn from_config(config: &Configuration, python_version: Option<&str>) -> PythonTargets {
        let python_version
            = python_version.unwrap_or(&config.settings.python_version.value);

        let python
            = PythonVersion::parse(python_version)
                .unwrap_or(PythonVersion {major: 3, minor: 12, patch: None});

        let systems
            = config.settings.supported_systems();

        let mut platforms
            = known_platforms().into_iter()
                .filter(|platform| systems.iter().any(|set| platform.matches_set(set)))
                .collect::<Vec<_>>();

        // The current platform is always part of the targets; installing
        // on a machine that isn't covered would otherwise be impossible.
        if let Some(current) = Platform::current() {
            if !platforms.contains(&current) {
                platforms.insert(0, current);
            }
        }

        let envs = platforms.into_iter()
            .map(|platform| PythonEnv {python: python.clone(), platform})
            .collect();

        PythonTargets {
            python,
            envs,
            macos_target: parse_two(&config.settings.pypi_macos_target.value).unwrap_or((14, 0)),
            manylinux_target: parse_two(&config.settings.pypi_manylinux_target.value).unwrap_or((2, 34)),
        }
    }

    pub fn current_env(&self) -> PythonEnv {
        let platform
            = Platform::current()
                .unwrap_or_else(|| self.envs[0].platform.clone());

        PythonEnv {python: self.python.clone(), platform}
    }

    /// A stable string describing the targets, used to invalidate locked
    /// island resolutions when the targets change.
    pub fn fingerprint(&self) -> String {
        let platforms = self.envs.iter()
            .map(|env| format!("{}-{}-{}", env.platform.sys_platform(), env.platform.platform_machine(), env.platform.libc.as_ref().map(|libc| zpm_utils::ToFileString::to_file_string(libc)).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(",");

        format!("python={};platforms={};macos={}.{};manylinux={}.{}", self.python.full(), platforms, self.macos_target.0, self.macos_target.1, self.manylinux_target.0, self.manylinux_target.1)
    }
}

fn parse_two(src: &str) -> Option<(u64, u64)> {
    let mut parts
        = src.split('.');

    Some((parts.next()?.parse().ok()?, parts.next().unwrap_or("0").parse().ok()?))
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

/// How a marker evaluates across a set of environments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerOutcome {
    Always,
    Never,
    Sometimes,
}

pub fn evaluate_across(marker: Option<&pep_508::Marker<'_>>, envs: &[PythonEnv], extra: Option<&str>) -> MarkerOutcome {
    let Some(marker) = marker else {
        return MarkerOutcome::Always;
    };

    let results
        = envs.iter()
            .map(|env| evaluate_marker(marker, env, extra))
            .collect::<Vec<_>>();

    if results.iter().all(|result| *result) {
        MarkerOutcome::Always
    } else if results.iter().all(|result| !*result) {
        MarkerOutcome::Never
    } else {
        MarkerOutcome::Sometimes
    }
}

/// Whether the marker references the `extra` variable.
pub fn marker_mentions_extra(marker: &pep_508::Marker<'_>) -> bool {
    match marker {
        pep_508::Marker::And(lhs, rhs) | pep_508::Marker::Or(lhs, rhs)
            => marker_mentions_extra(lhs) || marker_mentions_extra(rhs),

        pep_508::Marker::Operator(lhs, _, rhs)
            => matches!(lhs, pep_508::Variable::Extra) || matches!(rhs, pep_508::Variable::Extra),
    }
}

/// Evaluates a marker. `extra` is the extra being activated (`None` for the
/// base dependencies, in which case `extra == "..."` is always false).
pub fn evaluate_marker(marker: &pep_508::Marker<'_>, env: &PythonEnv, extra: Option<&str>) -> bool {
    match marker {
        pep_508::Marker::And(lhs, rhs)
            => evaluate_marker(lhs, env, extra) && evaluate_marker(rhs, env, extra),

        pep_508::Marker::Or(lhs, rhs)
            => evaluate_marker(lhs, env, extra) || evaluate_marker(rhs, env, extra),

        pep_508::Marker::Operator(lhs, operator, rhs)
            => evaluate_operator(lhs, *operator, rhs, env, extra),
    }
}

/// Evaluates a marker string at link time. The extra comparisons are
/// considered true: the edge was only kept because the extra is active.
pub fn evaluate_marker_str(marker: &str, env: &PythonEnv) -> bool {
    let requirement
        = format!("x ; {}", marker);

    match pep_508::parse(&requirement) {
        Ok(parsed) => parsed.marker.as_ref()
            .map_or(true, |marker| evaluate_marker_with_any_extra(marker, env)),

        Err(_) => true,
    }
}

fn evaluate_marker_with_any_extra(marker: &pep_508::Marker<'_>, env: &PythonEnv) -> bool {
    match marker {
        pep_508::Marker::And(lhs, rhs)
            => evaluate_marker_with_any_extra(lhs, env) && evaluate_marker_with_any_extra(rhs, env),

        pep_508::Marker::Or(lhs, rhs)
            => evaluate_marker_with_any_extra(lhs, env) || evaluate_marker_with_any_extra(rhs, env),

        pep_508::Marker::Operator(lhs, operator, rhs) => {
            if matches!(lhs, pep_508::Variable::Extra) || matches!(rhs, pep_508::Variable::Extra) {
                return true;
            }

            evaluate_operator(lhs, *operator, rhs, env, None)
        },
    }
}

#[derive(Clone, Copy)]
enum MarkerKind {
    Version,
    String,
    Extra,
}

fn variable_value(variable: &pep_508::Variable<'_>, env: &PythonEnv, extra: Option<&str>) -> (String, Option<MarkerKind>) {
    match variable {
        pep_508::Variable::PythonVersion => (env.python.short(), Some(MarkerKind::Version)),
        pep_508::Variable::PythonFullVersion => (env.python.full_for_markers(), Some(MarkerKind::Version)),
        pep_508::Variable::ImplementationVersion => (env.python.full_for_markers(), Some(MarkerKind::Version)),
        pep_508::Variable::OsName => (env.platform.os_name().to_string(), Some(MarkerKind::String)),
        pep_508::Variable::SysPlatform => (env.platform.sys_platform().to_string(), Some(MarkerKind::String)),
        pep_508::Variable::PlatformSystem => (env.platform.platform_system().to_string(), Some(MarkerKind::String)),
        pep_508::Variable::PlatformMachine => (env.platform.platform_machine().to_string(), Some(MarkerKind::String)),
        pep_508::Variable::PlatformPythonImplementation => ("CPython".to_string(), Some(MarkerKind::String)),
        pep_508::Variable::ImplementationName => ("cpython".to_string(), Some(MarkerKind::String)),
        pep_508::Variable::PlatformRelease => (String::new(), Some(MarkerKind::String)),
        pep_508::Variable::PlatformVersion => (String::new(), Some(MarkerKind::String)),
        pep_508::Variable::Extra => (extra.map(zpm_primitives::normalize_pypi_extra).unwrap_or_default(), Some(MarkerKind::Extra)),
        pep_508::Variable::String(value) => (value.to_string(), None),
    }
}

fn evaluate_operator(lhs: &pep_508::Variable<'_>, operator: pep_508::Operator, rhs: &pep_508::Variable<'_>, env: &PythonEnv, extra: Option<&str>) -> bool {
    let (lhs_value, lhs_kind)
        = variable_value(lhs, env, extra);
    let (rhs_value, rhs_kind)
        = variable_value(rhs, env, extra);

    let kind
        = lhs_kind.or(rhs_kind).unwrap_or(MarkerKind::String);

    match kind {
        MarkerKind::Extra => {
            if extra.is_none() {
                return matches!(operator, pep_508::Operator::Comparator(pep_508::Comparator::Ne) | pep_508::Operator::NotIn);
            }

            let lhs_value
                = zpm_primitives::normalize_pypi_extra(&lhs_value);
            let rhs_value
                = zpm_primitives::normalize_pypi_extra(&rhs_value);

            compare_strings(&lhs_value, operator, &rhs_value)
        },

        MarkerKind::Version => {
            compare_versions(&lhs_value, operator, &rhs_value, rhs_kind.is_none())
                .unwrap_or_else(|| compare_strings(&lhs_value, operator, &rhs_value))
        },

        MarkerKind::String => {
            compare_strings(&lhs_value, operator, &rhs_value)
        },
    }
}

fn compare_strings(lhs: &str, operator: pep_508::Operator, rhs: &str) -> bool {
    match operator {
        pep_508::Operator::Comparator(pep_508::Comparator::Eq) => lhs == rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Ae) => lhs == rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Ne) => lhs != rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Lt) => lhs < rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Le) => lhs <= rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Gt) => lhs > rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Ge) => lhs >= rhs,
        pep_508::Operator::Comparator(pep_508::Comparator::Cp) => false,
        pep_508::Operator::In => rhs.contains(lhs),
        pep_508::Operator::NotIn => !rhs.contains(lhs),
    }
}

/// Compares a version variable with a literal. When the variable is on the
/// left (`python_version < "3.12"`) the literal is used as a PEP 440
/// specifier, which gives `==3.12.*` its wildcard semantics.
fn compare_versions(lhs: &str, operator: pep_508::Operator, rhs: &str, variable_on_left: bool) -> Option<bool> {
    let comparator = match operator {
        pep_508::Operator::Comparator(comparator) => comparator,
        pep_508::Operator::In | pep_508::Operator::NotIn => {
            let found = if variable_on_left {
                rhs.split([' ', ',']).any(|candidate| candidate == lhs)
            } else {
                lhs.split([' ', ',']).any(|candidate| candidate == rhs)
            };

            return Some(found == matches!(operator, pep_508::Operator::In));
        },
    };

    let (version, literal, comparator) = if variable_on_left {
        (lhs, rhs, comparator)
    } else {
        (rhs, lhs, flip(comparator))
    };

    let op = match comparator {
        pep_508::Comparator::Lt => "<",
        pep_508::Comparator::Le => "<=",
        pep_508::Comparator::Ne => "!=",
        pep_508::Comparator::Eq => "==",
        pep_508::Comparator::Ge => ">=",
        pep_508::Comparator::Gt => ">",
        pep_508::Comparator::Cp => "~=",
        pep_508::Comparator::Ae => "===",
    };

    let version
        = pep440_rs::Version::from_str(version).ok()?;

    let specifiers: Option<pep440_rs::VersionSpecifiers>
        = pep440_rs::VersionSpecifiers::from_str(&format!("{}{}", op, literal)).ok();

    if let Some(specifiers) = specifiers {
        return Some(specifiers.contains(&version));
    }

    let literal
        = pep440_rs::Version::from_str(literal).ok()?;

    let ordering
        = version.cmp(&literal);

    Some(match comparator {
        pep_508::Comparator::Lt => ordering == Ordering::Less,
        pep_508::Comparator::Le => ordering != Ordering::Greater,
        pep_508::Comparator::Gt => ordering == Ordering::Greater,
        pep_508::Comparator::Ge => ordering != Ordering::Less,
        pep_508::Comparator::Eq | pep_508::Comparator::Ae => ordering == Ordering::Equal,
        pep_508::Comparator::Ne => ordering != Ordering::Equal,
        pep_508::Comparator::Cp => ordering != Ordering::Less,
    })
}

fn flip(comparator: pep_508::Comparator) -> pep_508::Comparator {
    match comparator {
        pep_508::Comparator::Lt => pep_508::Comparator::Gt,
        pep_508::Comparator::Le => pep_508::Comparator::Ge,
        pep_508::Comparator::Gt => pep_508::Comparator::Lt,
        pep_508::Comparator::Ge => pep_508::Comparator::Le,
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Wheel tags
// ---------------------------------------------------------------------------

pub struct WheelTags<'a> {
    pub python: Vec<&'a str>,
    pub abi: Vec<&'a str>,
    pub platform: Vec<&'a str>,
    pub build: Option<u64>,
}

pub fn parse_wheel_tags(filename: &str) -> Option<WheelTags<'_>> {
    let stem
        = filename.strip_suffix(".whl")?;

    let parts
        = stem.split('-').collect::<Vec<_>>();

    let (build, tags) = match parts.len() {
        5 => (None, &parts[2..]),
        6 => (parts[2].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok(), &parts[3..]),
        _ => return None,
    };

    Some(WheelTags {
        python: tags[0].split('.').collect(),
        abi: tags[1].split('.').collect(),
        platform: tags[2].split('.').collect(),
        build,
    })
}

/// Ordered list of `(python, abi, platform)` tags supported by an
/// environment, most preferred first (same order as `packaging.tags`).
pub fn supported_tags(env: &PythonEnv, targets: &PythonTargets) -> Vec<(String, String, String)> {
    let major
        = env.python.major;
    let minor
        = env.python.minor;

    let platforms
        = platform_tags(&env.platform, targets);

    let mut tags
        = Vec::new();

    let cp
        = format!("cp{}{}", major, minor);

    for platform in &platforms {
        tags.push((cp.clone(), cp.clone(), platform.clone()));
    }

    for abi3_minor in (2..=minor).rev() {
        for platform in &platforms {
            tags.push((format!("cp{}{}", major, abi3_minor), "abi3".to_string(), platform.clone()));
        }
    }

    for platform in &platforms {
        tags.push((cp.clone(), "none".to_string(), platform.clone()));
    }

    for py_minor in (0..=minor).rev() {
        for platform in &platforms {
            tags.push((format!("py{}{}", major, py_minor), "none".to_string(), platform.clone()));
        }

        if py_minor == minor {
            for platform in &platforms {
                tags.push((format!("py{}", major), "none".to_string(), platform.clone()));
            }
        }
    }

    tags.push((cp.clone(), "none".to_string(), "any".to_string()));

    for py_minor in (0..=minor).rev() {
        tags.push((format!("py{}{}", major, py_minor), "none".to_string(), "any".to_string()));

        if py_minor == minor {
            tags.push((format!("py{}", major), "none".to_string(), "any".to_string()));
        }
    }

    tags
}

fn platform_tags(platform: &Platform, targets: &PythonTargets) -> Vec<String> {
    match (&platform.os, &platform.libc) {
        (Os::MacOS, _) => macos_platform_tags(&platform.cpu, targets.macos_target),
        (Os::Windows, _) => match platform.cpu {
            Cpu::Aarch64 => vec!["win_arm64".to_string()],
            Cpu::I386 => vec!["win32".to_string()],
            _ => vec!["win_amd64".to_string()],
        },
        (_, Some(Libc::Musl)) => musllinux_platform_tags(platform.platform_machine()),
        _ => manylinux_platform_tags(platform.platform_machine(), targets.manylinux_target),
    }
}

fn manylinux_platform_tags(arch: &str, (_, max_minor): (u64, u64)) -> Vec<String> {
    let min_minor
        = if arch == "aarch64" { 17 } else { 5 };

    let mut tags
        = Vec::new();

    for minor in (min_minor..=max_minor).rev() {
        tags.push(format!("manylinux_2_{}_{}", minor, arch));

        match minor {
            17 => tags.push(format!("manylinux2014_{}", arch)),
            12 => tags.push(format!("manylinux2010_{}", arch)),
            5 => tags.push(format!("manylinux1_{}", arch)),
            _ => {},
        }
    }

    tags.push(format!("linux_{}", arch));
    tags
}

fn musllinux_platform_tags(arch: &str) -> Vec<String> {
    let mut tags
        = Vec::new();

    for minor in (1..=2).rev() {
        tags.push(format!("musllinux_1_{}_{}", minor, arch));
    }

    tags.push(format!("linux_{}", arch));
    tags
}

fn macos_platform_tags(cpu: &Cpu, (target_major, target_minor): (u64, u64)) -> Vec<String> {
    let arch
        = match cpu {
            Cpu::Aarch64 => "arm64",
            _ => "x86_64",
        };

    let formats: &[&str]
        = match cpu {
            Cpu::Aarch64 => &["arm64", "universal2"],
            _ => &["x86_64", "intel", "fat64", "fat32", "universal2", "universal"],
        };

    let mut tags
        = Vec::new();

    for major in (11..=target_major.max(11)).rev() {
        if major > target_major {
            continue;
        }

        let max_minor
            = if major == target_major { target_minor } else { 0 };

        for minor in (0..=max_minor).rev() {
            for format in formats {
                tags.push(format!("macosx_{}_{}_{}", major, minor, format));
            }
        }
    }

    if arch == "x86_64" {
        for minor in (4..=16).rev() {
            for format in formats {
                tags.push(format!("macosx_10_{}_{}", minor, format));
            }
        }
    } else {
        for minor in (4..=16).rev() {
            tags.push(format!("macosx_10_{}_universal2", minor));
        }
    }

    tags
}

/// Returns the priority of the wheel for the environment (lower is
/// better), or `None` if it isn't compatible.
pub fn wheel_priority(filename: &str, supported: &[(String, String, String)]) -> Option<(usize, std::cmp::Reverse<u64>)> {
    let tags
        = parse_wheel_tags(filename)?;

    let index = supported.iter().position(|(python, abi, platform)| {
        tags.python.contains(&python.as_str())
            && tags.abi.contains(&abi.as_str())
            && tags.platform.contains(&platform.as_str())
    })?;

    Some((index, std::cmp::Reverse(tags.build.unwrap_or(0))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: Os, cpu: Cpu, libc: Option<Libc>) -> PythonEnv {
        PythonEnv {
            python: PythonVersion {major: 3, minor: 12, patch: None},
            platform: Platform {os, cpu, libc},
        }
    }

    fn eval(marker: &str, env: &PythonEnv, extra: Option<&str>) -> bool {
        let requirement
            = format!("x ; {}", marker);
        let parsed
            = pep_508::parse(&requirement).unwrap();

        evaluate_marker(parsed.marker.as_ref().unwrap(), env, extra)
    }

    #[test]
    fn markers() {
        let mac
            = env(Os::MacOS, Cpu::Aarch64, None);
        let linux
            = env(Os::Linux, Cpu::X86_64, Some(Libc::Glibc));

        assert!(eval("sys_platform == 'darwin'", &mac, None));
        assert!(!eval("sys_platform == 'darwin'", &linux, None));
        assert!(eval("python_version >= '3.8'", &mac, None));
        assert!(eval("python_version < '3.13'", &mac, None));
        assert!(!eval("python_version < '3.12'", &mac, None));
        assert!(eval("python_full_version == '3.12.*'", &mac, None));
        assert!(eval("'3.10' <= python_version", &mac, None));
        assert!(eval("platform_machine == 'x86_64' and sys_platform == 'linux'", &linux, None));
        assert!(eval("platform_python_implementation != 'PyPy'", &linux, None));
        assert!(!eval("extra == 'foo'", &linux, None));
        assert!(eval("extra == 'foo_bar'", &linux, Some("Foo-Bar")));
        assert!(eval("platform_machine in 'x86_64 aarch64'", &linux, None));
    }

    #[test]
    fn wheels() {
        let targets = PythonTargets {
            python: PythonVersion {major: 3, minor: 12, patch: None},
            envs: vec![],
            macos_target: (14, 0),
            manylinux_target: (2, 34),
        };

        let mac
            = supported_tags(&env(Os::MacOS, Cpu::Aarch64, None), &targets);
        let linux
            = supported_tags(&env(Os::Linux, Cpu::X86_64, Some(Libc::Glibc)), &targets);

        assert!(wheel_priority("foo-1.0-py3-none-any.whl", &mac).is_some());
        assert!(wheel_priority("foo-1.0-cp312-cp312-macosx_11_0_arm64.whl", &mac).is_some());
        assert!(wheel_priority("foo-1.0-cp312-cp312-macosx_15_0_arm64.whl", &mac).is_none());
        assert!(wheel_priority("foo-1.0-cp311-cp311-macosx_11_0_arm64.whl", &mac).is_none());
        assert!(wheel_priority("foo-1.0-cp39-abi3-macosx_10_12_universal2.whl", &mac).is_some());
        assert!(wheel_priority("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl", &linux).is_some());
        assert!(wheel_priority("foo-1.0-cp312-cp312-manylinux_2_39_x86_64.whl", &linux).is_none());
        assert!(wheel_priority("foo-1.0-cp312-cp312-musllinux_1_2_x86_64.whl", &linux).is_none());

        let specific
            = wheel_priority("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl", &linux).unwrap();
        let generic
            = wheel_priority("foo-1.0-py3-none-any.whl", &linux).unwrap();

        assert!(specific < generic);
    }
}
