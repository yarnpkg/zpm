use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use zpm_primitives::{PypiSpecifierSet, PypiVersion, PythonTargetEnv};
use zpm_utils::FromFileString;

const DEFAULT_MANYLINUX_MAJOR: u16 = 2;
// supportedTargets currently records the libc family, but not its version. Use a modern
// compatibility ceiling while still rejecting musllinux unless the target asks for musl.
const DEFAULT_MANYLINUX_MINOR: u16 = 40;
const DEFAULT_MUSLLINUX_MAJOR: u16 = 1;
const DEFAULT_MUSLLINUX_MINOR: u16 = 2;

#[derive(Clone, Debug, Deserialize)]
pub struct PypiDistribution {
    #[serde(default)]
    pub filename: String,

    #[serde(default)]
    pub packagetype: String,

    pub url: String,

    #[serde(default)]
    pub upload_time: Option<String>,

    #[serde(default)]
    pub upload_time_iso_8601: Option<String>,

    #[serde(default)]
    pub requires_python: Option<String>,
}

pub fn pypi_registry_base() -> String {
    std::env::var("ZPM_PYPI_REGISTRY")
        .ok()
        .unwrap_or_else(|| "https://pypi.org".to_string())
        .trim_end_matches('/')
        .to_string()
}

pub fn encode_path_segment(segment: &str) -> String {
    url::form_urlencoded::byte_serialize(segment.as_bytes())
        .collect::<String>()
}

pub fn parse_upload_time(distribution: &PypiDistribution) -> Option<DateTime<Utc>> {
    distribution.upload_time_iso_8601.as_ref()
        .or(distribution.upload_time.as_ref())
        .and_then(|value| {
            DateTime::parse_from_rfc3339(value).ok()
                .map(|time| time.with_timezone(&Utc))
                .or_else(|| {
                    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok()
                        .map(|time| time.and_utc())
                })
    })
}

#[derive(Debug)]
struct WheelTags<'a> {
    python: Vec<&'a str>,
    abi: Vec<&'a str>,
    platform: Vec<&'a str>,
    build: Option<BuildTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BuildTag {
    number: u64,
    suffix: String,
}

#[derive(Debug)]
struct TargetTag {
    python: String,
    abi: String,
    platform: String,
}

#[derive(Debug)]
struct TargetTags {
    tags: Vec<TargetTag>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct WheelPriority {
    tag: usize,
    build: Option<BuildTag>,
}

impl TargetTags {
    fn from_target(target: &PythonTargetEnv) -> Option<Self> {
        let (major, minor)
            = parse_python_major_minor(&target.python_version)?;
        let platform_tags
            = compatible_platform_tags(target);

        let mut tags
            = Vec::new();
        let implementation_name
            = target.implementation_name.as_deref().unwrap_or("cpython");

        if implementation_name == "cpython" {
            push_cpython_tags(&mut tags, major, minor, &platform_tags);
        } else {
            push_generic_python_tags(&mut tags, major, minor, &platform_tags);
            push_generic_python_tags(&mut tags, major, minor, &["any".to_string()]);
        }

        Some(Self {
            tags,
        })
    }

    fn priority(&self, wheel_tags: &WheelTags<'_>) -> Option<usize> {
        for (index, target_tag) in self.tags.iter().enumerate() {
            if wheel_tags.python.iter().any(|tag| *tag == target_tag.python.as_str())
                && wheel_tags.abi.iter().any(|tag| *tag == target_tag.abi.as_str())
                && wheel_tags.platform.iter().any(|tag| *tag == target_tag.platform.as_str()) {
                return Some(self.tags.len() - index);
            }
        }

        None
    }
}

fn parse_wheel_tags(filename: &str) -> Option<WheelTags<'_>> {
    let filename
        = filename.strip_suffix(".whl")?;
    let mut parts
        = filename.rsplitn(4, '-');
    let platform_tag
        = parts.next()?;
    let abi_tag
        = parts.next()?;
    let python_tag
        = parts.next()?;
    let prefix
        = parts.next()?;

    let build
        = parse_wheel_build_tag(prefix)?;

    Some(WheelTags {
        python: parse_compressed_tags(python_tag)?,
        abi: parse_compressed_tags(abi_tag)?,
        platform: parse_compressed_tags(platform_tag)?,
        build,
    })
}

fn parse_wheel_build_tag(prefix: &str) -> Option<Option<BuildTag>> {
    let prefix_parts
        = prefix.split('-').collect::<Vec<_>>();

    match prefix_parts.as_slice() {
        [distribution, version] if !distribution.is_empty() && !version.is_empty() => Some(None),
        [distribution, version, build] if !distribution.is_empty() && !version.is_empty() => {
            Some(Some(parse_build_tag(build)?))
        },
        _ => None,
    }
}

fn parse_build_tag(tag: &str) -> Option<BuildTag> {
    let digit_len
        = tag.bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();

    if digit_len == 0 {
        return None;
    }

    let suffix
        = &tag[digit_len..];

    if !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return None;
    }

    Some(BuildTag {
        number: tag[..digit_len].parse().ok()?,
        suffix: suffix.to_string(),
    })
}

fn parse_compressed_tags(tags: &str) -> Option<Vec<&str>> {
    let tags
        = tags.split('.').collect::<Vec<_>>();

    if tags.iter().any(|tag| !is_valid_tag_atom(tag)) {
        return None;
    }

    Some(tags)
}

fn is_valid_tag_atom(tag: &str) -> bool {
    !tag.is_empty()
        && tag.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn parse_python_major_minor(version: &str) -> Option<(u8, u8)> {
    let (major, minor)
        = version.split_once('.')?;

    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn push_tag(tags: &mut Vec<TargetTag>, python: String, abi: String, platform: String) {
    tags.push(TargetTag {
        python,
        abi,
        platform,
    });
}

fn push_tag_for_platforms(tags: &mut Vec<TargetTag>, python: &str, abi: &str, platform_tags: &[String]) {
    for platform_tag in platform_tags {
        push_tag(tags, python.to_string(), abi.to_string(), platform_tag.clone());
    }
}

fn push_cpython_tags(tags: &mut Vec<TargetTag>, major: u8, minor: u8, platform_tags: &[String]) {
    let exact_python
        = format!("cp{major}{minor}");

    for exact_abi in cpython_exact_abis(major, minor) {
        push_tag_for_platforms(tags, &exact_python, &exact_abi, platform_tags);
    }

    if major == 3 {
        for abi_minor in (2..=minor).rev() {
            let abi_python
                = format!("cp{major}{abi_minor}");

            push_tag_for_platforms(tags, &abi_python, "abi3", platform_tags);

            if abi_minor == minor {
                push_tag_for_platforms(tags, &abi_python, "none", platform_tags);
            }
        }
    }

    push_generic_python_tags(tags, major, minor, platform_tags);

    push_tag(tags, exact_python, "none".to_string(), "any".to_string());
    push_generic_python_tags(tags, major, minor, &["any".to_string()]);
}

fn push_generic_python_tags(tags: &mut Vec<TargetTag>, major: u8, minor: u8, platform_tags: &[String]) {
    for python_minor in (0..=minor).rev() {
        let python_tag
            = format!("py{major}{python_minor}");

        push_tag_for_platforms(tags, &python_tag, "none", platform_tags);

        if python_minor == minor {
            let python_tag
                = format!("py{major}");

            push_tag_for_platforms(tags, &python_tag, "none", platform_tags);
        }
    }
}

fn cpython_exact_abis(major: u8, minor: u8) -> Vec<String> {
    let plain
        = format!("cp{major}{minor}");

    if major == 3 && minor <= 7 {
        vec![format!("{plain}m"), plain]
    } else {
        vec![plain]
    }
}

fn compatible_platform_tags(target: &PythonTargetEnv) -> Vec<String> {
    match target.sys_platform.as_deref() {
        Some("linux") => linux_platform_tags(target),
        Some("darwin") => macos_platform_tags(target),
        Some("win32") => windows_platform_tags(target),
        _ => Vec::new(),
    }
}

fn linux_platform_tags(target: &PythonTargetEnv) -> Vec<String> {
    let Some(arch)
        = target.platform_machine.as_deref()
            .and_then(linux_arch_tag) else {
        return Vec::new();
    };

    match target.libc.as_deref() {
        Some("musl") => musllinux_platform_tags(arch),
        Some("glibc") | None => manylinux_platform_tags(arch),
        Some(_) => vec![format!("linux_{arch}")],
    }
}

fn linux_arch_tag(machine: &str) -> Option<&'static str> {
    match machine {
        "aarch64" | "arm64" => Some("aarch64"),
        "i386" | "i686" | "x86" => Some("i686"),
        "x86_64" | "amd64" => Some("x86_64"),
        _ => None,
    }
}

fn manylinux_platform_tags(arch: &str) -> Vec<String> {
    let Some(minimum_minor)
        = minimum_manylinux_minor(arch) else {
        return vec![format!("linux_{arch}")];
    };

    let mut tags
        = Vec::new();

    for minor in (minimum_minor..=DEFAULT_MANYLINUX_MINOR).rev() {
        tags.push(format!("manylinux_{DEFAULT_MANYLINUX_MAJOR}_{minor}_{arch}"));

        if minor == 17 {
            tags.push(format!("manylinux2014_{arch}"));
        }

        if minor == 12 {
            tags.push(format!("manylinux2010_{arch}"));
        }

        if minor == 5 {
            tags.push(format!("manylinux1_{arch}"));
        }
    }

    tags.push(format!("linux_{arch}"));

    tags
}

fn minimum_manylinux_minor(arch: &str) -> Option<u16> {
    match arch {
        "aarch64" => Some(17),
        "i686" | "x86_64" => Some(5),
        _ => None,
    }
}

fn musllinux_platform_tags(arch: &str) -> Vec<String> {
    let mut tags
        = vec![format!("linux_{arch}")];

    for minor in (0..=DEFAULT_MUSLLINUX_MINOR).rev() {
        tags.push(format!("musllinux_{DEFAULT_MUSLLINUX_MAJOR}_{minor}_{arch}"));
    }

    tags
}

fn macos_platform_tags(target: &PythonTargetEnv) -> Vec<String> {
    let Some(arch)
        = target.platform_machine.as_deref()
            .and_then(macos_arch_tag) else {
        return Vec::new();
    };
    let Some((major, minor))
        = macos_target_version(target, arch) else {
        return Vec::new();
    };

    let mut tags
        = Vec::new();

    match arch {
        "arm64" => {
            if major >= 11 {
                for major in (11..=major).rev() {
                    push_macos_tags(&mut tags, major, 0, &["arm64", "universal2"]);
                }
            }

            for minor in (4..=16).rev() {
                push_macos_tags(&mut tags, 10, minor, &["universal2"]);
            }
        },
        "i386" | "x86_64" => {
            let formats
                = macos_binary_formats(arch);

            if major == 10 {
                if minor >= 4 {
                    for minor in (4..=minor).rev() {
                        push_macos_tags(&mut tags, 10, minor, formats);
                    }
                }
            } else if major >= 11 {
                for major in (11..=major).rev() {
                    push_macos_tags(&mut tags, major, 0, formats);
                }

                for minor in (4..=16).rev() {
                    push_macos_tags(&mut tags, 10, minor, formats);
                }
            }
        },
        _ => {},
    }

    tags
}

fn macos_arch_tag(machine: &str) -> Option<&'static str> {
    match machine {
        "arm64" | "aarch64" => Some("arm64"),
        "i386" | "i686" | "x86" => Some("i386"),
        "x86_64" | "amd64" => Some("x86_64"),
        _ => None,
    }
}

fn macos_target_version(target: &PythonTargetEnv, arch: &str) -> Option<(u16, u16)> {
    if let Some(version) = target.platform_release.as_deref()
        .and_then(macos_version_from_darwin_release) {
        return Some(version);
    }

    match arch {
        "arm64" => Some((11, 0)),
        "i386" | "x86_64" => Some((10, 9)),
        _ => None,
    }
}

fn macos_version_from_darwin_release(release: &str) -> Option<(u16, u16)> {
    let darwin_major
        = release.split('.').next()?.parse::<u16>().ok()?;

    if darwin_major >= 20 {
        Some((darwin_major - 9, 0))
    } else if darwin_major >= 4 {
        Some((10, darwin_major - 4))
    } else {
        None
    }
}

fn macos_binary_formats(arch: &str) -> &'static [&'static str] {
    match arch {
        "i386" => &["i386", "intel", "fat32", "fat", "universal"],
        "x86_64" => &["x86_64", "intel", "fat64", "fat32", "universal2", "universal"],
        _ => &[],
    }
}

fn push_macos_tags(tags: &mut Vec<String>, major: u16, minor: u16, formats: &[&str]) {
    for format in formats {
        tags.push(format!("macosx_{major}_{minor}_{format}"));
    }
}

fn windows_platform_tags(target: &PythonTargetEnv) -> Vec<String> {
    let Some(machine)
        = target.platform_machine.as_deref() else {
        return Vec::new();
    };

    match machine {
        "aarch64" | "arm64" => vec!["win_arm64".to_string()],
        "i386" | "i686" | "x86" => vec!["win32".to_string()],
        "x86_64" | "amd64" | "AMD64" => vec!["win_amd64".to_string()],
        _ => Vec::new(),
    }
}

fn distribution_matches_requires_python(distribution: &PypiDistribution, target: Option<&PythonTargetEnv>) -> bool {
    let Some(target) = target else {
        return true;
    };
    let Some(requires_python) = distribution.requires_python.as_ref()
        .filter(|requires_python| !requires_python.is_empty()) else {
        return true;
    };

    let target_version
        = target.python_full_version.as_deref()
            .unwrap_or(&target.python_version);
    let Ok(target_version)
        = PypiVersion::from_file_string(target_version) else {
            return false;
        };
    let Ok(specifier)
        = PypiSpecifierSet::from_file_string(requires_python) else {
            return false;
        };

    target_version.satisfies(&specifier).unwrap_or(false)
}

fn wheel_target_priority(distribution: &PypiDistribution, target_tags: Option<&TargetTags>) -> Option<WheelPriority> {
    let Some(target_tags) = target_tags else {
        return Some(WheelPriority {
            tag: 0,
            build: None,
        });
    };
    let wheel_tags
        = parse_wheel_tags(&distribution.filename)?;

    Some(WheelPriority {
        tag: target_tags.priority(&wheel_tags)?,
        build: wheel_tags.build,
    })
}

pub fn select_best_wheel<'a>(distributions: &'a [PypiDistribution], target: Option<&PythonTargetEnv>) -> Option<&'a PypiDistribution> {
    let target_tags
        = match target {
            Some(target) => Some(TargetTags::from_target(target)?),
            None => None,
        };

    distributions.iter()
        .filter(|distribution| distribution.packagetype == "bdist_wheel")
        .filter(|distribution| distribution_matches_requires_python(distribution, target))
        .filter_map(|distribution| {
            Some((distribution, wheel_target_priority(distribution, target_tags.as_ref())?))
        })
        .max_by(|(a, a_priority), (b, b_priority)| {
            a_priority.cmp(b_priority)
                .then_with(|| parse_upload_time(a).cmp(&parse_upload_time(b)))
                .then_with(|| b.filename.cmp(&a.filename))
        })
        .map(|(distribution, _)| distribution)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(filename: &str, upload_time: &str, requires_python: Option<&str>) -> PypiDistribution {
        PypiDistribution {
            filename: filename.to_string(),
            packagetype: "bdist_wheel".to_string(),
            url: format!("https://example.com/{filename}"),
            upload_time: None,
            upload_time_iso_8601: Some(upload_time.to_string()),
            requires_python: requires_python.map(|value| value.to_string()),
        }
    }

    fn python_312() -> PythonTargetEnv {
        PythonTargetEnv {
            python_version: "3.12".to_string(),
            python_full_version: Some("3.12.2".to_string()),
            os_name: Some("posix".to_string()),
            sys_platform: Some("linux".to_string()),
            platform_machine: Some("x86_64".to_string()),
            platform_system: Some("Linux".to_string()),
            libc: Some("glibc".to_string()),
            platform_release: None,
            platform_version: None,
            platform_python_implementation: Some("CPython".to_string()),
            implementation_name: Some("cpython".to_string()),
            implementation_version: Some("3.12.2".to_string()),
        }
    }

    fn python_312_musl() -> PythonTargetEnv {
        PythonTargetEnv {
            libc: Some("musl".to_string()),
            ..python_312()
        }
    }

    fn python_312_macos_arm64() -> PythonTargetEnv {
        PythonTargetEnv {
            os_name: Some("posix".to_string()),
            sys_platform: Some("darwin".to_string()),
            platform_machine: Some("arm64".to_string()),
            platform_system: Some("Darwin".to_string()),
            libc: None,
            platform_release: Some("23.0.0".to_string()),
            ..python_312()
        }
    }

    fn python_312_windows_x64() -> PythonTargetEnv {
        PythonTargetEnv {
            os_name: Some("nt".to_string()),
            sys_platform: Some("win32".to_string()),
            platform_machine: Some("AMD64".to_string()),
            platform_system: Some("Windows".to_string()),
            libc: None,
            ..python_312()
        }
    }

    fn python_312_pypy() -> PythonTargetEnv {
        PythonTargetEnv {
            platform_python_implementation: Some("PyPy".to_string()),
            implementation_name: Some("pypy".to_string()),
            implementation_version: Some("7.3.15".to_string()),
            ..python_312()
        }
    }

    #[test]
    fn test_select_best_wheel_accepts_universal_py3_wheels_for_targets() {
        let target
            = python_312();
        let distributions
            = vec![wheel("pkg-1.0.0-py3-none-any.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-py3-none-any.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_accepts_universal_py3_wheels_for_non_cpython_targets() {
        let target
            = python_312_pypy();
        let distributions
            = vec![wheel("pkg-1.0.0-py3-none-any.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-py3-none-any.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_accepts_linux_platform_wheels_for_targets() {
        let target
            = python_312();
        let distributions
            = vec![wheel("pkg-1.0.0-cp312-cp312-manylinux_2_28_x86_64.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-cp312-cp312-manylinux_2_28_x86_64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_filters_wrong_platform_wheels_for_targets() {
        let target
            = python_312();
        let distributions
            = vec![wheel("pkg-1.0.0-cp312-cp312-win_amd64.whl", "2024-01-01T00:00:00Z", None)];

        assert!(select_best_wheel(&distributions, Some(&target)).is_none());
    }

    #[test]
    fn test_select_best_wheel_accepts_abi3_wheels_for_newer_cpython_targets() {
        let target
            = python_312();
        let distributions
            = vec![wheel("pkg-1.0.0-cp38-abi3-manylinux2014_x86_64.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-cp38-abi3-manylinux2014_x86_64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_accepts_compressed_platform_tags() {
        let target
            = python_312();
        let distributions
            = vec![wheel("pkg-1.0.0-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_prefers_specific_tags_over_newer_universal_wheels() {
        let target
            = python_312();
        let distributions
            = vec![
                wheel("pkg-1.0.0-py3-none-any.whl", "2024-02-01T00:00:00Z", None),
                wheel("pkg-1.0.0-cp312-cp312-manylinux_2_28_x86_64.whl", "2024-01-01T00:00:00Z", None),
            ];

        assert_eq!(
            Some("pkg-1.0.0-cp312-cp312-manylinux_2_28_x86_64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_prefers_higher_build_tag_for_equal_tags() {
        let target
            = python_312();
        let distributions
            = vec![
                wheel("pkg-1.0.0-1-cp312-cp312-manylinux_2_28_x86_64.whl", "2024-02-01T00:00:00Z", None),
                wheel("pkg-1.0.0-2-cp312-cp312-manylinux_2_28_x86_64.whl", "2024-01-01T00:00:00Z", None),
            ];

        assert_eq!(
            Some("pkg-1.0.0-2-cp312-cp312-manylinux_2_28_x86_64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_uses_libc_when_matching_linux_wheels() {
        let target
            = python_312_musl();
        let distributions
            = vec![
                wheel("pkg-1.0.0-cp312-cp312-manylinux_2_28_x86_64.whl", "2024-02-01T00:00:00Z", None),
                wheel("pkg-1.0.0-cp312-cp312-musllinux_1_2_x86_64.whl", "2024-01-01T00:00:00Z", None),
            ];

        assert_eq!(
            Some("pkg-1.0.0-cp312-cp312-musllinux_1_2_x86_64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_accepts_macos_universal2_wheels() {
        let target
            = python_312_macos_arm64();
        let distributions
            = vec![wheel("pkg-1.0.0-cp312-cp312-macosx_11_0_universal2.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-cp312-cp312-macosx_11_0_universal2.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_accepts_windows_amd64_wheels() {
        let target
            = python_312_windows_x64();
        let distributions
            = vec![wheel("pkg-1.0.0-cp312-cp312-win_amd64.whl", "2024-01-01T00:00:00Z", None)];

        assert_eq!(
            Some("pkg-1.0.0-cp312-cp312-win_amd64.whl"),
            select_best_wheel(&distributions, Some(&target)).map(|distribution| distribution.filename.as_str()),
        );
    }

    #[test]
    fn test_select_best_wheel_filters_requires_python_for_targets() {
        let target
            = python_312();
        let distributions
            = vec![
                wheel("pkg-1.0.0-py3-none-any.whl", "2024-02-01T00:00:00Z", Some(">=3.13")),
                wheel("pkg-1.0.0-py3-none-any.whl", "2024-01-01T00:00:00Z", Some("<3.13")),
            ];

        assert_eq!(
            Some("2024-01-01T00:00:00Z"),
            select_best_wheel(&distributions, Some(&target))
                .and_then(|distribution| distribution.upload_time_iso_8601.as_deref()),
        );
    }
}
