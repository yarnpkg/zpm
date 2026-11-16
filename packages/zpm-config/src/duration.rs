use serde::{Deserialize, Deserializer, de};
use zpm_utils::{AbstractValue, DurationUnit, parse_duration_with_unit};

use crate::{
    HydrateError,
    Interpolated,
    Partial,
    Setting,
    Source,
};

// Duration settings don't go through `FromFileString` like the other settings:
// a value without unit (`1440` rather than `1440m`) is read in the unit the
// schema declares for each setting (its `unit` property, as in Berry), which
// the generated code passes to the functions below.

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDuration {
    /// Strings (after interpolation) and numbers
    Text(Interpolated<String>),

    /// The `{secs, nanos}` objects that `yarn config set` used to write
    Structured(std::time::Duration),
}

impl RawDuration {
    fn into_duration<E: de::Error>(self, unit: DurationUnit) -> Result<std::time::Duration, E> {
        match self {
            RawDuration::Text(text)
                => parse_duration_with_unit(&text, unit).map_err(de::Error::custom),

            RawDuration::Structured(duration)
                => Ok(duration),
        }
    }
}

pub(crate) trait DurationIntermediate: Sized {
    fn deserialize_with_unit<'de, D: Deserializer<'de>>(deserializer: D, unit: DurationUnit) -> Result<Self, D::Error>;
}

impl DurationIntermediate for Interpolated<std::time::Duration> {
    fn deserialize_with_unit<'de, D: Deserializer<'de>>(deserializer: D, unit: DurationUnit) -> Result<Self, D::Error> {
        RawDuration::deserialize(deserializer)?
            .into_duration(unit)
            .map(Interpolated::new)
    }
}

impl DurationIntermediate for Option<Interpolated<std::time::Duration>> {
    fn deserialize_with_unit<'de, D: Deserializer<'de>>(deserializer: D, unit: DurationUnit) -> Result<Self, D::Error> {
        Option::<RawDuration>::deserialize(deserializer)?
            .map(|raw| raw.into_duration(unit).map(Interpolated::new))
            .transpose()
    }
}

macro_rules! deserialize_in_unit {
    ($name:ident, $unit:expr) => {
        // Only the units that the schema uses are referenced
        #[allow(dead_code)]
        pub(crate) fn $name<'de, D: Deserializer<'de>, T: DurationIntermediate>(deserializer: D) -> Result<Partial<T>, D::Error> {
            T::deserialize_with_unit(deserializer, $unit).map(Partial::Value)
        }
    };
}

deserialize_in_unit!(deserialize_in_milliseconds, DurationUnit::Milliseconds);
deserialize_in_unit!(deserialize_in_seconds, DurationUnit::Seconds);
deserialize_in_unit!(deserialize_in_minutes, DurationUnit::Minutes);
deserialize_in_unit!(deserialize_in_hours, DurationUnit::Hours);
deserialize_in_unit!(deserialize_in_days, DurationUnit::Days);
deserialize_in_unit!(deserialize_in_weeks, DurationUnit::Weeks);

pub(crate) trait DurationSetting: Sized {
    fn from_env_string_with_unit(value: &str, unit: DurationUnit) -> Result<Self, HydrateError>;

    fn hydrate_with_unit(path: &[&str], value_str: &str, unit: DurationUnit) -> Result<AbstractValue<'static>, HydrateError>;
}

impl DurationSetting for Setting<std::time::Duration> {
    fn from_env_string_with_unit(value: &str, unit: DurationUnit) -> Result<Self, HydrateError> {
        let value
            = parse_duration_with_unit(value, unit)
                .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

        Ok(Self {
            value,
            source: Source::Environment,
        })
    }

    fn hydrate_with_unit(path: &[&str], value_str: &str, unit: DurationUnit) -> Result<AbstractValue<'static>, HydrateError> {
        if let Some(key) = path.first() {
            return Err(HydrateError::KeyNotFound(key.to_string()));
        }

        let value
            = parse_duration_with_unit(value_str, unit)
                .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

        Ok(AbstractValue::new(crate::FileString(value)))
    }
}

impl DurationSetting for Setting<Option<std::time::Duration>> {
    fn from_env_string_with_unit(value: &str, unit: DurationUnit) -> Result<Self, HydrateError> {
        let value = match value {
            "null" => None,
            value => Some(parse_duration_with_unit(value, unit).map_err(|e| HydrateError::InvalidValue(e.to_string()))?),
        };

        Ok(Self {
            value,
            source: Source::Environment,
        })
    }

    fn hydrate_with_unit(path: &[&str], value_str: &str, unit: DurationUnit) -> Result<AbstractValue<'static>, HydrateError> {
        if let Some(key) = path.first() {
            return Err(HydrateError::KeyNotFound(key.to_string()));
        }

        let value = match value_str {
            "null" => None,
            value_str => Some(parse_duration_with_unit(value_str, unit).map_err(|e| HydrateError::InvalidValue(e.to_string()))?),
        };

        Ok(AbstractValue::new(value.map(crate::FileString)))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use crate::{
        ConfigurationContext,
        MergeSettings,
        Partial,
        Settings,
        intermediate,
    };

    const MINUTE: Duration = Duration::from_secs(60);

    fn settings_from(text: &str, env: &[(&str, &str)]) -> Settings {
        let context = ConfigurationContext {
            env: env.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect::<BTreeMap<_, _>>(),
            user_cwd: None,
            project_cwd: None,
            package_cwd: None,
        };

        let project
            = serde_yaml::from_str::<intermediate::Settings>(text)
                .expect("The configuration should be valid");

        Settings::merge(
            &context,
            Partial::Missing,
            Partial::Value(project),
            || panic!("No configuration found"),
        ).apply_env_overrides(&context)
            .expect("The environment should be valid")
    }

    #[test]
    fn values_without_unit_should_use_the_setting_unit() {
        let table: &[(&str, Duration)] = &[
            ("npmMinimalAgeGate: 1440", 1440 * MINUTE),
            ("npmMinimalAgeGate: \"1440\"", 1440 * MINUTE),
            ("npmMinimalAgeGate: 1.5", Duration::from_secs(90)),
            ("npmMinimalAgeGate: 0", Duration::ZERO),
            ("npmMinimalAgeGate: 1440s", Duration::from_secs(1440)),
            ("npmMinimalAgeGate: 3h", 180 * MINUTE),
            ("npmMinimalAgeGate: 7d", 7 * 1440 * MINUTE),
            ("npmMinimalAgeGate: {secs: 60, nanos: 0}", MINUTE),
        ];

        for (text, expected) in table {
            assert_eq!(settings_from(text, &[]).npm_minimal_age_gate.value, *expected, "{text}");
        }

        assert_eq!(settings_from("pypiMinimalAgeGate: 1440", &[]).pypi_minimal_age_gate.value, 1440 * MINUTE);
        assert_eq!(settings_from("daemonDefaultWarmupPeriod: 2", &[]).daemon_default_warmup_period.value, Duration::from_secs(2));
        assert_eq!(settings_from("daemonDefaultWarmupPeriod: 500ms", &[]).daemon_default_warmup_period.value, Duration::from_millis(500));

        let http_table: &[(&str, Duration)] = &[
            ("httpTimeout: 60000", MINUTE),
            ("httpTimeout: \"1500\"", Duration::from_millis(1500)),
            ("httpTimeout: 1m", MINUTE),
            ("httpTimeout: 30s", Duration::from_secs(30)),
            ("httpTimeout: 1500ms", Duration::from_millis(1500)),
            ("httpTimeout: 0.5s", Duration::from_millis(500)),
        ];

        for (text, expected) in http_table {
            assert_eq!(settings_from(text, &[]).http_timeout.value, *expected, "{text}");
        }

        assert_eq!(settings_from("slowNetworkTimeout: 2000", &[]).slow_network_timeout.value, Duration::from_secs(2));
        assert_eq!(settings_from("slowNetworkTimeout: 10s", &[]).slow_network_timeout.value, Duration::from_secs(10));
    }

    #[test]
    fn defaults_should_be_unchanged() {
        let settings
            = settings_from("{}", &[]);

        assert_eq!(settings.npm_minimal_age_gate.value, 1440 * MINUTE);
        assert_eq!(settings.pypi_minimal_age_gate.value, Duration::ZERO);
        assert_eq!(settings.daemon_default_warmup_period.value, Duration::from_secs(1));
        assert_eq!(settings.http_timeout.value, MINUTE);
        assert_eq!(settings.slow_network_timeout.value, Duration::from_secs(5));
    }

    #[test]
    fn environment_values_without_unit_should_use_the_setting_unit() {
        let settings
            = settings_from("{}", &[("YARN_NPM_MINIMAL_AGE_GATE", "1440"), ("YARN_PYPI_MINIMAL_AGE_GATE", "90s"), ("YARN_DAEMON_DEFAULT_WARMUP_PERIOD", "3"), ("YARN_HTTP_TIMEOUT", "1500"), ("YARN_SLOW_NETWORK_TIMEOUT", "2s")]);

        assert_eq!(settings.npm_minimal_age_gate.value, 1440 * MINUTE);
        assert_eq!(settings.pypi_minimal_age_gate.value, Duration::from_secs(90));
        assert_eq!(settings.daemon_default_warmup_period.value, Duration::from_secs(3));
        assert_eq!(settings.http_timeout.value, Duration::from_millis(1500));
        assert_eq!(settings.slow_network_timeout.value, Duration::from_secs(2));
    }

    #[test]
    fn nested_values_without_unit_should_use_the_global_setting_unit() {
        let settings = settings_from(r#"
            packageRules:
              - packageFilter: foo
                npmMinimalAgeGate: 1440
                pypiMinimalAgeGate: 60
              - packageFilter: bar
                npmMinimalAgeGate: null
            sourceRules:
              - ecosystemFilter: npm
                npmMinimalAgeGate: 30
                pypiMinimalAgeGate: 2h
        "#, &[]);

        assert_eq!(settings.package_rules[0].npm_minimal_age_gate.value, Some(1440 * MINUTE));
        assert_eq!(settings.package_rules[0].pypi_minimal_age_gate.value, Some(60 * MINUTE));
        assert_eq!(settings.package_rules[1].npm_minimal_age_gate.value, None);
        assert_eq!(settings.source_rules[0].npm_minimal_age_gate.value, Some(30 * MINUTE));
        assert_eq!(settings.source_rules[0].pypi_minimal_age_gate.value, Some(120 * MINUTE));
    }

    #[test]
    fn hydrate_should_use_the_setting_unit() {
        let value
            = <Settings as MergeSettings>::hydrate(&["npmMinimalAgeGate"], "1440").unwrap();

        assert_eq!(value.export(false), zpm_utils::ToHumanString::to_print_string(&(1440 * MINUTE)));
    }

    #[test]
    fn invalid_values_should_be_rejected() {
        for text in ["npmMinimalAgeGate: 5 minutes", "npmMinimalAgeGate: -5", "npmMinimalAgeGate: 1M", "npmMinimalAgeGate: true"] {
            assert!(serde_yaml::from_str::<intermediate::Settings>(text).is_err(), "{text}");
        }
    }
}
