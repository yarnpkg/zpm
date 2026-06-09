use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use zpm_primitives::{PypiSpecifierSet, PypiVersion, PythonTargetEnv};
use zpm_utils::FromFileString;

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

fn parse_wheel_tags(filename: &str) -> Option<(&str, &str, &str)> {
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
    parts.next()?;

    Some((python_tag, abi_tag, platform_tag))
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

fn wheel_matches_target(distribution: &PypiDistribution, target: Option<&PythonTargetEnv>) -> bool {
    let Some(target) = target else {
        return true;
    };
    let Some((python_tag, abi_tag, platform_tag)) = parse_wheel_tags(&distribution.filename) else {
        return false;
    };

    if abi_tag != "none" || platform_tag != "any" {
        return false;
    }

    let Some(major) = target.python_version.split('.').next() else {
        return false;
    };
    let expected_python_tag
        = format!("py{major}");

    python_tag.split('.').any(|tag| tag == expected_python_tag)
}

pub fn select_best_wheel<'a>(distributions: &'a [PypiDistribution], target: Option<&PythonTargetEnv>) -> Option<&'a PypiDistribution> {
    distributions.iter()
        .filter(|distribution| distribution.packagetype == "bdist_wheel")
        .filter(|distribution| distribution_matches_requires_python(distribution, target))
        .filter(|distribution| wheel_matches_target(distribution, target))
        .max_by(|a, b| {
            parse_upload_time(a).cmp(&parse_upload_time(b))
                .then_with(|| b.filename.cmp(&a.filename))
        })
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
            platform_release: None,
            platform_version: None,
            platform_python_implementation: Some("CPython".to_string()),
            implementation_name: Some("cpython".to_string()),
            implementation_version: Some("3.12.2".to_string()),
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
    fn test_select_best_wheel_filters_platform_wheels_for_targets() {
        let target
            = python_312();
        let distributions
            = vec![wheel("pkg-1.0.0-cp312-cp312-manylinux_2_28_x86_64.whl", "2024-01-01T00:00:00Z", None)];

        assert!(select_best_wheel(&distributions, Some(&target)).is_none());
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
