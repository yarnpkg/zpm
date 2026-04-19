use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

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

pub fn select_best_wheel(distributions: &[PypiDistribution]) -> Option<&PypiDistribution> {
    distributions.iter()
        .filter(|distribution| distribution.packagetype == "bdist_wheel")
        .max_by(|a, b| {
            parse_upload_time(a).cmp(&parse_upload_time(b))
                .then_with(|| b.filename.cmp(&a.filename))
        })
}
