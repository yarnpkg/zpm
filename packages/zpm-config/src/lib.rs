use std::{collections::BTreeMap, fmt::Display, ops::Deref, sync::Arc, time::UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use zpm_utils::{AbstractValue, Container, Cpu, DataType, FromFileString, IoResultExt, LastModifiedAt, Libc, Os, Path, RawString, Serialized, System, ToFileString, ToHumanString, tree};

#[derive(Debug, Clone)]
pub struct ConfigurationContext {
    pub env: BTreeMap<String, String>,
    pub user_cwd: Option<Path>,
    pub project_cwd: Option<Path>,
    pub package_cwd: Option<Path>,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Source {
    #[default]
    Default,
    User,
    Project,
    Environment,
    Cli,
    Mixed,
}

#[derive(Debug, Clone, Default)]
pub struct Setting<T> {
    pub value: T,
    pub source: Source,
}

impl<T> Setting<T> {
    pub fn new(value: T, source: Source) -> Self {
        Self {value, source}
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Setting<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {value: T::deserialize(deserializer)?, source: Source::Default})
    }
}

impl<T: Serialize> Serialize for Setting<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        self.value.serialize(serializer)
    }
}

/**
 * Serde will by default coalesce both a missing value and a null value to `None`. We
 * don't want that (`null` should be its own value), so we instead use the Partial<T>
 * type to present a potentially missing value.
 *
 * We implement custom Deserialize instead of using `#[serde(untagged)]` because the
 * untagged attribute swallows the actual deserialization errors and replaces them
 * with a generic "did not match any variant of untagged enum" message.
 *
 * The `#[serde(default)]` attribute on fields using `Partial` ensures that missing
 * fields return `Partial::Missing` (via the Default trait). When a field is present,
 * our custom deserialize directly attempts to deserialize into `T`, properly
 * propagating any errors that occur.
 *
 * To recap:
 * - {} -> Missing
 * - {key: null} -> Value(None)
 * - {key: "value"} -> Value(Some("value"))
 *
 * The negative of this is that we have to enable `#[serde(default)]` on all fields
 * using `Partial`, but since we're generating the code, we can easily do that.
 */
#[derive(Debug, Default)]
enum Partial<T> {
    #[default]
    Missing,
    Value(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Partial<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Partial::Value)
    }
}

impl<T> Partial<T> where T: Default {
    pub fn unwrap_or_default(self) -> T {
        match self {
            Partial::Missing => T::default(),
            Partial::Value(value) => value,
        }
    }
}

#[derive(Debug, Default)]
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

trait MergeSettings: Sized {
    type Intermediate;

    fn from_env_string(
        value: &str,
        from_config: Option<Self>,
    ) -> Result<Self, HydrateError>;

    fn hydrate(
        &self,
        path: &[&str],
        value_str: &str,
    ) -> Result<AbstractValue<'_>, HydrateError>;

    fn get(
        &self,
        path: &[&str],
    ) -> Result<ConfigurationEntry<'_>, GetError>;

    fn merge<F: Fn() -> Self>(
        context: &ConfigurationContext,
        user: Partial<Self::Intermediate>,
        project: Partial<Self::Intermediate>,
        default: F,
    ) -> Self;

    fn tree_node(
        &self,
        label: Option<String>,
        description: Option<String>,
    ) -> tree::Node<'_>;
}

impl<K: Ord + ToFileString + ToHumanString + FromFileString + Serialize + std::fmt::Debug, T: MergeSettings + Serialize + std::fmt::Debug> MergeSettings for BTreeMap<K, T> {
    type Intermediate = BTreeMap<K, T::Intermediate>;

    fn from_env_string(_value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
        unimplemented!("Configuration maps cannot be returned directly just yet");
    }

    fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'_>, HydrateError> {
        let Some(key_str) = path.first() else {
            unimplemented!("Configuration maps cannot be returned directly just yet");
        };

        let Ok(key) = K::from_file_string(key_str) else {
            return Err(HydrateError::InvalidKey(key_str.to_string()));
        };

        let Some(entry) = self.get(&key) else {
            return Err(HydrateError::KeyNotFound(key_str.to_string()));
        };

        entry.hydrate(&path[1..], value_str)
    }

    fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        let Some(key_str) = path.first() else {
            return Ok(ConfigurationEntry {
                value: AbstractValue::new(Container::new(self)),
                source: Source::Mixed,
            });
        };

        let Ok(key) = K::from_file_string(key_str) else {
            return Err(GetError::InvalidKey(key_str.to_string()));
        };

        let Some(entry) = self.get(&key) else {
            return Err(GetError::KeyNotFound(key_str.to_string()));
        };

        entry.get(&path[1..])
    }

    fn merge<F: FnOnce() -> Self>(context: &ConfigurationContext, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, _default: F) -> Self {
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
            let hydrated_item = T::merge(
                context,
                user_value,
                project_value,
                || unreachable!("We shouldn't reach this place since we insert only if there's a value in either user or project settings"),
            );

            result.insert(k, hydrated_item);
        }

        result
    }

    fn tree_node(&self, label: Option<String>, description: Option<String>) -> tree::Node<'_> {
        let mut children
            = tree::Map::new();

        for (k, v) in self {
            children.insert(Serialized::new(k).to_print_string(), v.tree_node(Some(k.to_print_string()), None));
        }

        if let Some(description) = description {
            let mut fields
                = tree::Map::new();

            fields.insert("description".to_string(), tree::Node {
                label: Some("Description".to_string()),
                value: Some(AbstractValue::new(RawString::new(description))),
                children: None,
            });

            fields.insert("entries".to_string(), tree::Node {
                label: Some("Entries".to_string()),
                value: None,
                children: Some(tree::TreeNodeChildren::Map(children)),
            });

            tree::Node {
                label,
                value: None,
                children: Some(tree::TreeNodeChildren::Map(fields)),
            }
        } else {
            tree::Node {
                label,
                value: None,
                children: Some(tree::TreeNodeChildren::Map(children)),
            }
        }
    }
}

impl<T: std::fmt::Debug + Serialize + MergeSettings> MergeSettings for Vec<T> {
    type Intermediate = Vec<T::Intermediate>;

    fn from_env_string(value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
        // An empty string means an explicitly empty array
        if value.is_empty() {
            return Ok(Vec::new());
        }

        // When an env var is set, it replaces the config entirely (not appends)
        let mut result
            = Vec::new();

        let items = value
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        for item_str in items {
            let value
                = T::from_env_string(item_str, None)
                    .map_err(|e| HydrateError::InvalidValue(e.to_string()))
                        .unwrap();

            result.push(value);
        }

        Ok(result)
    }

    fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'_>, HydrateError> {
        let Some(key_str) = path.first() else {
            unimplemented!("Configuration lists cannot be returned directly just yet");
        };

        let Ok(key) = usize::from_file_string(key_str) else {
            return Err(HydrateError::InvalidKey(key_str.to_string()));
        };

        if key >= self.len() {
            return Err(HydrateError::KeyNotFound(key_str.to_string()));
        };

        self[key].hydrate(&path[1..], value_str)
    }

    fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        let Some(key_str) = path.first() else {
            return Ok(ConfigurationEntry {
                value: AbstractValue::new(Container::new(self)),
                source: Source::Mixed,
            });
        };

        let Ok(key) = usize::from_file_string(key_str) else {
            return Err(GetError::InvalidKey(key_str.to_string()));
        };

        if key >= self.len() {
            return Err(GetError::KeyNotFound(key_str.to_string()));
        };

        self[key].get(&path[1..])
    }

    fn merge<F: FnOnce() -> Self>(context: &ConfigurationContext, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
        let mut result
            = Vec::new();

        if matches!(user, Partial::Missing) && matches!(project, Partial::Missing) {
            return default();
        }

        if let Partial::Value(user) = user {
            result.extend(user.into_iter().map(|v| {
                T::merge(
                    context,
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
                    Partial::Missing,
                    Partial::Value(v),
                    || unreachable!("We shouldn't reach this place since we insert only if there's a value in either user or project settings"),
                )
            }));
        }

        result
    }

    fn tree_node(&self, label: Option<String>, description: Option<String>) -> tree::Node<'_> {
        let mut children
            = Vec::new();

        for (i, v) in self.iter().enumerate() {
            children.push(v.tree_node(Some(DataType::Number.colorize(&i.to_string())), None));
        }

        if let Some(description) = description {
            let mut fields
                = tree::Map::new();

            fields.insert("description".to_string(), tree::Node {
                label: Some("Description".to_string()),
                value: Some(AbstractValue::new(RawString::new(description))),
                children: None,
            });

            fields.insert("entries".to_string(), tree::Node {
                label: Some("Entries".to_string()),
                value: None,
                children: Some(tree::TreeNodeChildren::Vec(children)),
            });

            tree::Node {
                label,
                value: None,
                children: Some(tree::TreeNodeChildren::Map(fields)),
            }
        } else {
            tree::Node {
                label,
                value: None,
                children: Some(tree::TreeNodeChildren::Vec(children)),
            }
        }
    }
}

impl MergeSettings for Setting<Path> {
    type Intermediate = Interpolated<Path>;

    fn from_env_string(value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
        let value
            = Path::from_file_string(value)
                .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

        Ok(Self {
            value,
            source: Source::Environment,
        })
    }

    fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'_>, HydrateError> {
        if let Some(key) = path.first() {
            return Err(HydrateError::KeyNotFound(key.to_string()));
        }

        let value
            = Path::from_file_string(value_str)
                .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

        Ok(AbstractValue::new(value))
    }

    fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        if let Some(key) = path.first() {
            return Err(GetError::KeyNotFound(key.to_string()));
        }

        Ok(ConfigurationEntry {
            value: AbstractValue::new(self.value.clone()),
            source: self.source,
        })
    }

    fn merge<F: FnOnce() -> Self>(context: &ConfigurationContext, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
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

    fn tree_node(&self, label: Option<String>, description: Option<String>) -> tree::Node<'_> {
        let mut fields
            = tree::Map::new();

        if let Some(description) = description {
            fields.insert("description".to_string(), tree::Node {
                label: Some("Description".to_string()),
                value: Some(AbstractValue::new(RawString::new(description))),
                children: None,
            });
        }

        fields.insert("value".to_string(), tree::Node {
            label: Some("Value".to_string()),
            value: Some(AbstractValue::new(self.value.clone())),
            children: None,
        });

        tree::Node {
            label,
            value: None,
            children: Some(tree::TreeNodeChildren::Map(fields)),
        }
    }
}

macro_rules! merge_settings_impl {
    ($type:ty, $from_str:expr) => {
        impl MergeSettings for Setting<$type> {
            type Intermediate = Interpolated<$type>;

            fn from_env_string(value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
                let value
                    = <$type as FromFileString>::from_file_string(value)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(Self {
                    value,
                    source: Source::Environment,
                })
            }

            fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'_>, HydrateError> {
                if let Some(key) = path.first() {
                    return Err(HydrateError::KeyNotFound(key.to_string()));
                }

                let value
                    = <$type as FromFileString>::from_file_string(value_str)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(AbstractValue::new(value))
            }

            fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
                if let Some(key) = path.first() {
                    return Err(GetError::KeyNotFound(key.to_string()));
                }

                Ok(ConfigurationEntry {
                    value: AbstractValue::new(self.value.clone()),
                    source: self.source,
                })
            }

            fn merge<F: FnOnce() -> Self>(_context: &ConfigurationContext, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
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

            fn tree_node(&self, label: Option<String>, description: Option<String>) -> tree::Node<'_> {
                let mut fields
                    = tree::Map::new();

                if let Some(description) = description {
                    fields.insert("description".to_string(), tree::Node {
                        label: Some("Description".to_string()),
                        value: Some(AbstractValue::new(RawString::new(description))),
                        children: None,
                    });
                }

                fields.insert("value".to_string(), tree::Node {
                    label: Some("Value".to_string()),
                    value: Some(AbstractValue::new(self.value.clone())),
                    children: None,
                });

                tree::Node {
                    label,
                    value: None,
                    children: Some(tree::TreeNodeChildren::Map(fields)),
                }
            }
        }

        impl MergeSettings for Setting<Option<$type>> {
            type Intermediate = Option<Interpolated<$type>>;

            fn from_env_string(value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
                let value
                    = <Option<$type> as FromFileString>::from_file_string(value)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(Self {
                    value,
                    source: Source::Environment,
                })
            }

            fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'_>, HydrateError> {
                if let Some(key) = path.first() {
                    return Err(HydrateError::KeyNotFound(key.to_string()));
                }

                let value
                    = <Option<$type> as FromFileString>::from_file_string(value_str)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(AbstractValue::new(value))
            }

            fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
                if !path.is_empty() {
                    return Err(GetError::KeyNotFound(path.join(".").to_string()));
                }

                Ok(ConfigurationEntry {
                    value: AbstractValue::new(self.value.clone()),
                    source: self.source,
                })
            }

            fn merge<F: FnOnce() -> Self>(context: &ConfigurationContext, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, default: F) -> Self {
                if let Partial::Value(user) = user {
                    let inner = user.map(|user| {
                        Setting::<$type>::merge(
                            context,
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
                            context,
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

            fn tree_node(&self, label: Option<String>, description: Option<String>) -> tree::Node<'_> {
                let mut fields
                    = tree::Map::new();

                if let Some(description) = description {
                    fields.insert("description".to_string(), tree::Node {
                        label: Some("Description".to_string()),
                        value: Some(AbstractValue::new(RawString::new(description))),
                        children: None,
                    });
                }

                fields.insert("value".to_string(), tree::Node {
                    label: Some("Value".to_string()),
                    value: Some(AbstractValue::new(self.value.clone())),
                    children: None,
                });

                tree::Node {
                    label,
                    value: Some(AbstractValue::new(self.value.clone())),
                    children: None,
                }
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

impl SupportedArchitectures {
    pub fn to_systems(&self) -> Vec<System> {
        let mut systems
            = Vec::new();

        let current
            = System::from_current();

        let cpus = if self.cpu.is_empty() {
            vec![&Cpu::Current]
        } else {
            self.cpu.iter().map(|c| &c.value).collect()
        };

        let os = if self.os.is_empty() {
            vec![&Os::Current]
        } else {
            self.os.iter().map(|o| &o.value).collect()
        };

        let libc = if self.libc.is_empty() {
            vec![&Libc::Current]
        } else {
            self.libc.iter().map(|l| &l.value).collect()
        };

        for &cpu in &cpus {
            for &os in &os {
                for &libc in &libc {
                    let arch = if cpu == &Cpu::Current {
                        current.arch.clone()
                    } else {
                        Some(cpu.clone())
                    };

                    let os = if os == &Os::Current {
                        current.os.clone()
                    } else {
                        Some(os.clone())
                    };

                    let libc = if libc == &Libc::Current {
                        current.libc.clone()
                    } else {
                        Some(libc.clone())
                    };

                    systems.push(System {
                        arch,
                        os,
                        libc,
                    });
                }
            }
        }

        systems
    }
}

pub struct Configuration {
    pub settings: Settings,
    pub user_config_path: Option<Path>,
    pub project_config_path: Option<Path>,
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum ConfigurationError {
    #[error(transparent)]
    IoError(Arc<std::io::Error>),

    #[error("Invalid enum value ({0})")]
    EnumError(String),

    #[error(transparent)]
    PathError(#[from] zpm_utils::PathError),

    #[error(transparent)]
    SerdeError(#[from] Arc<serde_yaml::Error>),
}

impl From<std::io::Error> for ConfigurationError {
    fn from(error: std::io::Error) -> Self {
        ConfigurationError::IoError(Arc::new(error))
    }
}

impl From<serde_yaml::Error> for ConfigurationError {
    fn from(error: serde_yaml::Error) -> Self {
        ConfigurationError::SerdeError(Arc::new(error))
    }
}

pub struct ConfigurationEntry<'a> {
    pub value: AbstractValue<'a>,
    pub source: Source,
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum GetError {
    #[error("Configuration key not found ({0})")]
    KeyNotFound(String),

    #[error("Invalid configuration key ({0})")]
    InvalidKey(String),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum HydrateError {
    #[error("Configuration key not found ({0})")]
    KeyNotFound(String),

    #[error("Invalid configuration key ({0})")]
    InvalidKey(String),

    #[error("Invalid configuration value ({0})")]
    InvalidValue(String),
}

impl Configuration {
    pub fn tree_node(&self) -> tree::Node<'_> {
        self.settings.tree_node(None, None)
    }

    pub fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'_>, HydrateError> {
        self.settings.hydrate(path, value_str)
    }

    pub fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        self.settings.get(path)
    }

    pub fn load(context: &ConfigurationContext, last_modified_at: &mut LastModifiedAt) -> Result<Configuration, ConfigurationError> {
        let rc_filename
            = std::env::var("YARN_RC_FILENAME")
                .unwrap_or_else(|_| ".yarnrc.yml".to_string());

        let user_config_path = context.user_cwd
            .as_ref()
            .map(|path| path.with_join_str(&rc_filename));

        let project_config_path = context.project_cwd
            .as_ref()
            .map(|path| path.with_join_str(&rc_filename));

        let mut intermediate_user_config
            = Partial::Missing;
        let mut intermediate_project_config
            = Partial::Missing;

        if let Some(user_config_path) = user_config_path.as_ref() {
            let metadata
                = user_config_path.fs_metadata()
                    .ok_missing()?;

            if let Some(metadata) = metadata {
                let user_last_changed_at
                    = metadata.modified()?
                        .duration_since(UNIX_EPOCH).unwrap()
                        .as_nanos();

                last_modified_at.update(user_last_changed_at);

                let user_config_text
                    = user_config_path
                        .fs_read_text_with_size(metadata.len())?;

                let user_config: intermediate::Settings
                    = serde_yaml::from_str(&user_config_text)?;

                intermediate_user_config = Partial::Value(user_config);
            }
        }

        if let Some(project_config_path) = project_config_path.as_ref() {
            let metadata
                = project_config_path.fs_metadata()
                    .ok_missing()?;

            if let Some(metadata) = metadata {
                let project_last_changed_at
                    = metadata.modified()?
                        .duration_since(UNIX_EPOCH).unwrap()
                        .as_nanos();

                last_modified_at.update(project_last_changed_at);

                let project_config_text
                    = project_config_path
                        .fs_read_text_with_size(metadata.len())?;

                let project_config: intermediate::Settings
                    = serde_yaml::from_str(&project_config_text)?;

                intermediate_project_config = Partial::Value(project_config);
            }
        }

        let mut settings = Settings::merge(
            &context,
            intermediate_user_config,
            intermediate_project_config,
            || panic!("No configuration found")
        );

        settings.catalogs.entry("default".to_string())
            .or_default()
            .extend(std::mem::take(&mut settings.catalog));

        Ok(Configuration {
            settings,
            user_config_path,
            project_config_path,
        })
    }
}

mod fns;
pub use fns::*;

mod types;
pub use types::*;

// Rust doesn't support specialization, so we can't have a blanket implementation for FromStr
// and a different one for Option<T: FromStr>; instead we manually generate whatever we need.
merge_settings!(std::time::Duration, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(String, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(bool, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(usize, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(u64, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(zpm_formats::CompressionAlgorithm, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(zpm_primitives::Descriptor, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::FilterDescriptor, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Ident, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::IdentGlob, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Locator, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::PeerRange, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Range, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Reference, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(zpm_semver::RangeKind, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(zpm_utils::Cpu, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Glob, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Libc, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Os, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Secret<String>, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(crate::types::NodeLinker, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::PnpFallbackMode, |s: &str| FromFileString::from_file_string(s).unwrap());
