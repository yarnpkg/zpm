use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use zpm_config::{Configuration, EcosystemFilter, PackageRule};
use zpm_primitives::Ident;

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

fn package_rule_matches(rule: &PackageRule, ident: &Ident) -> bool {
    rule.ecosystem_filter.value.map_or(true, |filter| filter == EcosystemFilter::Pypi)
        && rule.package_filter.value.as_ref().map_or(true, |filter| filter.check(ident))
}

pub fn get_registry<'a>(config: &'a Configuration, ident: &Ident) -> &'a str {
    let mut registry
        = config.settings.pypi_registry_server.value.as_str();

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, ident) {
            if let Some(value) = rule.pypi_registry_server.value.as_deref() {
                registry = value;
            }
        }
    }

    registry.trim_end_matches('/')
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
