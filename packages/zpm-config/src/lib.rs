use serde::{de, Deserialize, Deserializer};
use zpm_primitives::{Descriptor, PeerRange};
use zpm_semver::RangeKind;
use zpm_utils::{FromFileString, Path};
use std::{collections::BTreeMap, fmt::Display, ops::Deref};

pub struct Context {
    pub env: BTreeMap<String, String>,
    pub user_cwd: Option<Path>,
    pub project_cwd: Option<Path>,
    pub package_cwd: Option<Path>,
}

#[derive(Debug, Default)]
pub enum Source {
    #[default]
    Default,
    User,
    Project,
    Environment,
}

#[derive(Debug, Default)]
pub struct Setting<T> {
    pub value: T,
    pub source: Source,
}

impl<T> Setting<T> {
    pub fn new(value: T, source: Source) -> Self {
        Self {value, source}
    }
}

/**
 * Serde will by default coalesce both a missing value and a null value to `None`. We
 * don't want that (`null` should be its own value), so we instead use the Partial<T>
 * type to present a potentially missing value.
 *
 * The `serde(skip)` attribute prevent serde from turning `null` into `Missing`, and
 * the `untagged` attribute will make it try to assign it to the `T` type instead. If
 * the T type is `Option<Something>`, it'll then be correctly turned into `None`.
 *
 * To recap:
 * - {} -> Missing
 * - {key: null} -> Value(None)
 * - {key: "value"} -> Value(Some("value"))
 *
 * The negative of this is that we have to enable `#[serde(default)]` on all fields
 * using `Partial`, but since we're generating the code, we can easily do that.
 */
#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum Partial<T> {
    #[default]
    #[serde(skip)]
    Missing,
    Value(T),
}

impl<T> Partial<T> where T: Default {
    pub fn unwrap_or_default(self) -> T {
        match self {
            Partial::Missing => T::default(),
            Partial::Value(value) => value,
        }
    }
}

#[derive(Default)]
pub struct Interpolated<T> {
    value: T,
}

impl<T> Interpolated<T> {
    pub fn new(value: T) -> Self {
        Self {value}
    }

    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> Deref for Interpolated<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'de, T: FromFileString + Deserialize<'de>> Deserialize<'de> for Interpolated<T> where <T as FromFileString>::Error: Display {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrAnything<T> {
            String(String),
            Anything(T),
        }

        match StringOrAnything::<T>::deserialize(deserializer)? {
            StringOrAnything::String(s) => {
                let interpolated
                    = shellexpand::env(&s)
                        .map_err(de::Error::custom)?;

                let hydrated
                    = T::from_file_string(&interpolated)
                        .map_err(de::Error::custom)?;

                Ok(Interpolated::new(hydrated))
            },

            StringOrAnything::Anything(anything) => {
                Ok(Interpolated::new(anything))
            },
        }
    }
}

trait MergeSettings {
    type Intermediate;

    fn merge<F: Fn() -> Self>(
        context: &Context,
        prefix: Option<&str>,
        user: Partial<Self::Intermediate>,
        project: Partial<Self::Intermediate>,
        default: F,
    ) -> Self;
}

impl<T: MergeSettings> MergeSettings for BTreeMap<String, T> {
    type Intermediate = BTreeMap<String, T::Intermediate>;

    fn merge<F: FnOnce() -> Self>(context: &Context, prefix: Option<&str>, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, _default: F) -> Self {
        let mut join
            = BTreeMap::new();

        if let Partial::Value(user) = user {
            for (k, v) in user {
                join.insert(k, (Partial::Value(v), Partial::Missing));
            }
        }

        if let Partial::Value(project) = project {
            for (k, v) in project {
                join
                    .entry(k)
                    .or_default()
                    .1 = Partial::Value(v);
            }
        }

        let mut result
            = BTreeMap::new();

        for (k, (user_value, project_value)) in join {
            let next_prefix
                = prefix.map(|p| format!("{}_{}", p, k));

            let hydrated_item = T::merge(
                context,
                next_prefix.as_deref(),
                user_value,
                project_value,
                || unreachable!("We shouldn't reach this place since we insert only if there's a value in either user or project settings"),
            );

            result.insert(k, hydrated_item);
        }

        result
    }
}

impl<T: MergeSettings> MergeSettings for Vec<T> {
    type Intermediate = Vec<T::Intermediate>;

    fn merge<F: FnOnce() -> Self>(context: &Context, _prefix: Option<&str>, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, _default: F) -> Self {
        let mut result
            = Vec::new();

        if let Partial::Value(user) = user {
            result.extend(user.into_iter().map(|v| {
                T::merge(
                    context,
                    None,
                    Partial::Value(v),
                    Partial::Missing,
                    || unreachable!("We shouldn't reach this place since we insert only if there's a value in either user or project settings"),
                )
            }));
        }

        if let Partial::Value(project) = project {
            result.extend(project.into_iter().map(|v| {
                T::merge(
                    context,
                    None,
                    Partial::Missing,
                    Partial::Value(v),
                    || unreachable!("We shouldn't reach this place since we insert only if there's a value in either user or project settings"),
                )
            }));
        }

        result
    }
}

impl MergeSettings for Setting<Path> {
    type Intermediate = Interpolated<Path>;

    fn merge<F: FnOnce() -> Self>(context: &Context, prefix: Option<&str>, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
        if let Some(key) = prefix {
            if let Some(value) = context.env.get(key) {
                let mut path
                    = Path::from_file_string(value).unwrap();

                if path.is_relative() {
                    path = context.project_cwd
                        .as_ref()
                        .expect("A project cwd must be set when assigning a relative path value to a Yarn setting from the environment")
                        .with_join(&path);
                }

                return Self {
                    value: path,
                    source: Source::Environment,
                };
            }
        }

        if let Partial::Value(project_rel_path) = project {
            let path = context
                .package_cwd
                .as_ref()
                .expect("A package directory should be set since we're using the value provided through the project config")
                .with_join(&project_rel_path);

            return Self {
                value: path,
                source: Source::Project,
            };
        }

        if let Partial::Value(user_rel_path) = user {
            let path = context
                .user_cwd
                .as_ref()
                .expect("A project cwd must be set when assigning a relative path value to a Yarn setting from the user config")
                .with_join(&user_rel_path);

            return Self {
                value: path,
                source: Source::User,
            };
        }

        default()
    }
}

macro_rules! merge_settings_impl {
    ($type:ty, $from_str:expr) => {
        impl MergeSettings for Setting<$type> {
            type Intermediate = Interpolated<$type>;

            fn merge<F: FnOnce() -> Self>(context: &Context, key: Option<&str>, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
                if let Some(key) = key {
                    if let Some(value) = context.env.get(key) {
                        return Self {
                            value: $from_str(value),
                            source: Source::Environment,
                        };
                    }
                }

                if let Partial::Value(project) = project {
                    return Self {
                        value: project.into_inner(),
                        source: Source::Project,
                    };
                }

                if let Partial::Value(user) = user {
                    return Self {
                        value: user.into_inner(),
                        source: Source::User,
                    };
                }

                default()
            }
        }

        impl MergeSettings for Setting<Option<$type>> {
            type Intermediate = Option<Interpolated<$type>>;

            fn merge<F: FnOnce() -> Self>(context: &Context, prefix: Option<&str>, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
                if let Partial::Value(user) = user {
                    let inner = user.map(|user| {
                        Setting::<$type>::merge(
                            context, prefix,
                            Partial::Value(user),
                            Partial::Missing,
                            || panic!("We shouldn't reach this place since we insert only if there's a value in either user or project settings")
                        )
                    });

                    return inner.map_or_else(default, |inner| Self {
                        value: Some(inner.value),
                        source: inner.source,
                    });
                }

                if let Partial::Value(project) = project {
                    let inner = project.map(|project| {
                        Setting::<$type>::merge(
                            context, prefix,
                            Partial::Missing,
                            Partial::Value(project),
                            || panic!("We shouldn't reach this place since we insert only if there's a value in either user or project settings")
                        )
                    });

                    return inner.map_or_else(default, |inner| Self {
                        value: Some(inner.value),
                        source: inner.source,
                    });
                }

                default()
            }
        }
    };
}

macro_rules! merge_settings {
    ($type:ty, $from_str:expr) => {
        merge_settings_impl!($type, $from_str);
    };
}

include!(concat!(env!("OUT_DIR"), "/schema.rs"));

pub struct Configuration {
    pub settings: Settings,
    pub user_config_path: Option<Path>,
    pub project_config_path: Option<Path>,
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigurationError {
    #[error(transparent)]
    PathError(#[from] zpm_utils::PathError),

    #[error(transparent)]
    SerdeError(#[from] serde_yaml::Error),
}

pub fn merge(context: &Context, user_config: Option<&str>, project_config: Option<&str>) -> Result<Configuration, ConfigurationError> {
    let user_config_path = user_config
        .map(|config| Path::from_file_string(config))
        .transpose()?
        .map(|path| path.with_join_str(".yarnrc.yml"));

    let project_config_path = project_config
        .map(|config| Path::from_file_string(config))
        .transpose()?
        .map(|path| path.with_join_str(".yarnrc.yml"));

    let intermediate_user_config= user_config_path
        .as_ref()
        .map(|path| path.fs_read_text())
        .transpose()?
        .map(|content| serde_yaml::from_str::<intermediate::Settings>(&content))
        .transpose()?
        .map_or(Partial::Missing, Partial::Value);

    let intermediate_project_config = project_config_path
        .as_ref()
        .map(|path| path.fs_read_text())
        .transpose()?
        .map(|content| serde_yaml::from_str::<intermediate::Settings>(&content))
        .transpose()?
        .map_or(Partial::Missing, Partial::Value);

    let settings = Settings::merge(
        &context,
        Some("YARN"),
        intermediate_user_config,
        intermediate_project_config,
        || panic!("No configuration found")
    );

    Ok(Configuration {
        settings,
        user_config_path,
        project_config_path,
    })
}

pub mod fns;
pub use fns::*;

mod types;
pub use types::*;

// Rust doesn't support specialization, so we can't have a blanket implementation for FromStr
// and a different one for Option<T: FromStr>; instead we manually generate whatever we need.
merge_settings!(String, |s: &str| s.to_string());
merge_settings!(bool, |s: &str| s.parse().unwrap());
merge_settings!(usize, |s: &str| s.parse().unwrap());

merge_settings!(Descriptor, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(Glob, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(PnpFallbackMode, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(NodeLinker, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(PeerRange, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(RangeKind, |s: &str| FromFileString::from_file_string(s).unwrap());
