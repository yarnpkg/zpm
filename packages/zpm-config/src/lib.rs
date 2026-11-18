use std::{cell::Cell, collections::{BTreeMap, BTreeSet}, fmt::Display, ops::Deref, sync::Arc, time::UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use zpm_utils::{AbstractValue, Container, Cpu, DataType, FromFileString, LastModifiedAt, Libc, Os, Path, RawString, Serialized, System, SystemSet, ToFileString, ToHumanString, tree};

#[derive(Debug, Clone)]
pub struct ConfigurationContext {
    pub env: BTreeMap<String, String>,
    pub user_cwd: Option<Path>,
    pub project_cwd: Option<Path>,
    pub package_cwd: Option<Path>,
}

impl ConfigurationContext {
    /// Most-specific cwd available: project, falling back to package.
    pub fn preferred_cwd(&self) -> Option<&Path> {
        self.project_cwd.as_ref().or(self.package_cwd.as_ref())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Source {
    #[default]
    Default,
    User,
    Project,
    Environment,
    Cli,
    Hardened,
    Mixed,
}

impl Source {
    /// Berry-compatible label for `yarn config --json` output.
    pub fn label(&self) -> &'static str {
        match self {
            Source::Default => "<default>",
            Source::User => "<user>",
            Source::Project => "<project>",
            Source::Environment => "<environment>",
            Source::Cli => "<cli>",
            Source::Hardened => "<hardened>",
            Source::Mixed => "<mixed>",
        }
    }
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

    /// Overrides the setting with `value` and stamps it with `source`.
    pub fn force(&mut self, value: T, source: Source) {
        self.value = value;
        self.source = source;
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

/// Serializes a value through its file string rather than its serde
/// representation, so that `yarn config get --json` and `yarn config set`
/// produce values that can be written back into a configuration file
/// (`std::time::Duration` would otherwise serialize as `{secs, nanos}`).
#[derive(Debug, Clone)]
struct FileString<T>(T);

impl<T: ToFileString> Serialize for FileString<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        serializer.serialize_str(&self.0.to_file_string())
    }
}

impl<T: ToHumanString> ToHumanString for FileString<T> {
    fn to_print_string(&self) -> String {
        self.0.to_print_string()
    }
}

fn serialize_setting_as_file_string<T: ToFileString, S: Serializer>(setting: &Setting<T>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&setting.value.to_file_string())
}

fn serialize_optional_setting_as_file_string<T: ToFileString, S: Serializer>(setting: &Setting<Option<T>>, serializer: S) -> Result<S::Ok, S::Error> {
    match &setting.value {
        Some(value) => serializer.serialize_str(&value.to_file_string()),
        None => serializer.serialize_none(),
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

/// Deserializes a list setting that also accepts a single item, which is then
/// treated as a list of one. Used by settings such as `supportedArchitectures`,
/// which historically only accepted a single entry.
fn deserialize_one_or_many<'de, D, T>(deserializer: D) -> Result<Partial<Vec<T>>, D::Error>
    where D: Deserializer<'de>, T: Deserialize<'de>
{
    struct OneOrManyVisitor<T> {
        marker: std::marker::PhantomData<T>,
    }

    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for OneOrManyVisitor<T> {
        type Value = Partial<Vec<T>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a single entry or a list of entries")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Partial::Value(Vec::new()))
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Partial::Value(Vec::new()))
        }

        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut values
                = Vec::new();

            while let Some(value) = seq.next_element::<T>()? {
                values.push(value);
            }

            Ok(Partial::Value(values))
        }

        fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            let value
                = T::deserialize(de::value::MapAccessDeserializer::new(map))?;

            Ok(Partial::Value(vec![value]))
        }
    }

    deserializer.deserialize_any(OneOrManyVisitor {marker: std::marker::PhantomData})
}

impl<T> Partial<T> where T: Default {
    pub fn unwrap_or_default(self) -> T {
        match self {
            Partial::Missing => T::default(),
            Partial::Value(value) => value,
        }
    }
}

/**
 * Some settings have neither a default nor a null value (for example the
 * `linker` field of `unstableIslands` entries), so the rc file declaring their
 * parent must set them. We check that right after reading each rc file, which
 * lets us report the file and the setting path at fault rather than discover
 * it while merging the settings.
 *
 * The implementations for the intermediate structs are generated by build.rs.
 */
trait FindMissingSetting {
    /// Returns the path of the first required setting that isn't set.
    fn find_missing_setting(&self) -> Option<String>;
}

fn join_setting_path(parent: &str, child: &str) -> String {
    if child.starts_with('[') {
        format!("{parent}{child}")
    } else {
        format!("{parent}.{child}")
    }
}

impl<T> FindMissingSetting for Interpolated<T> {
    fn find_missing_setting(&self) -> Option<String> {
        None
    }
}

impl<T: FindMissingSetting> FindMissingSetting for Partial<T> {
    fn find_missing_setting(&self) -> Option<String> {
        match self {
            Partial::Missing
                => None,

            Partial::Value(value)
                => value.find_missing_setting(),
        }
    }
}

impl<T: FindMissingSetting> FindMissingSetting for Option<T> {
    fn find_missing_setting(&self) -> Option<String> {
        self.as_ref()
            .and_then(|value| value.find_missing_setting())
    }
}

impl<T: FindMissingSetting> FindMissingSetting for Vec<T> {
    fn find_missing_setting(&self) -> Option<String> {
        self.iter().enumerate().find_map(|(index, value)| {
            value.find_missing_setting()
                .map(|path| join_setting_path(&format!("[{index}]"), &path))
        })
    }
}

impl<K: ToFileString, T: FindMissingSetting> FindMissingSetting for BTreeMap<K, T> {
    fn find_missing_setting(&self) -> Option<String> {
        self.iter().find_map(|(key, value)| {
            value.find_missing_setting()
                .map(|path| join_setting_path(&key.to_file_string(), &path))
        })
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

thread_local! {
    static REQUIRES_TRUST: Cell<bool> = const { Cell::new(false) };
}

fn reset_requires_trust() {
    REQUIRES_TRUST.with(|requires_trust| {
        requires_trust.set(false);
    });
}

fn mark_requires_trust() {
    REQUIRES_TRUST.with(|requires_trust| {
        requires_trust.set(true);
    });
}

fn take_requires_trust() -> bool {
    REQUIRES_TRUST.with(|requires_trust| {
        let value
            = requires_trust.get();

        requires_trust.set(false);

        value
    })
}

fn hydrate_interpolated<T: FromFileString, E: de::Error>(value: &str) -> Result<Interpolated<T>, E> where <T as FromFileString>::Error: Display {
    let hydrated
        = T::from_file_string(value)
            .map_err(de::Error::custom)?;

    Ok(Interpolated::new(hydrated))
}

/// We use a visitor rather than an untagged enum, as the latter would replace
/// the actual error (for example "invalid type: sequence, expected a string")
/// by a generic "data did not match any variant" message. Raising the errors
/// from within the visitor also lets the YAML deserializer attach the location
/// of the faulty value to them.
struct InterpolatedVisitor<T> {
    marker: std::marker::PhantomData<T>,
}

impl<'de, T: FromFileString + Deserialize<'de>> de::Visitor<'de> for InterpolatedVisitor<T> where <T as FromFileString>::Error: Display {
    type Value = Interpolated<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a configuration value")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        let interpolated
            = shellexpand::env(value)
                .map_err(de::Error::custom)?;

        if interpolated.as_ref() != value {
            // If the interpolated value is different from the original, that means it may leak CI secrets,
            // for example with `npmRegistryServer: https://malicious.com/${GITHUB_TOKEN}`.
            mark_requires_trust();
        }

        hydrate_interpolated(&interpolated)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        hydrate_interpolated(&value.to_string())
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        hydrate_interpolated(&value.to_string())
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        hydrate_interpolated(&value.to_string())
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        T::deserialize(de::value::BoolDeserializer::new(value))
            .map(Interpolated::new)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        T::deserialize(de::value::UnitDeserializer::new())
            .map(Interpolated::new)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        T::deserialize(de::value::SeqAccessDeserializer::new(seq))
            .map(Interpolated::new)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        T::deserialize(de::value::MapAccessDeserializer::new(map))
            .map(Interpolated::new)
    }

    fn visit_enum<A: de::EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        T::deserialize(de::value::EnumAccessDeserializer::new(data))
            .map(Interpolated::new)
    }
}

impl<'de, T: FromFileString + Deserialize<'de>> Deserialize<'de> for Interpolated<T> where <T as FromFileString>::Error: Display {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(InterpolatedVisitor {marker: std::marker::PhantomData})
    }
}

/// Container settings (lists, maps, records) can't be parsed from a plain
/// string, so we read them as JSON documents, like with `--json`. Their
/// content is validated when the configuration file gets loaded again.
fn hydrate_container(value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
    let value: serde_yaml::Value
        = zpm_parsers::JsonDocument::hydrate_from_str(value_str)
            .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

    Ok(AbstractValue::new_container(Container::new(value)))
}

trait MergeSettings: Sized {
    type Intermediate;

    fn from_env_string(
        context: &ConfigurationContext,
        value: &str,
        from_config: Option<Self>,
    ) -> Result<Self, HydrateError>;

    /// Parses `value_str` as the value of the setting at `path`. This doesn't
    /// look at the current configuration, so that `yarn config set` can create
    /// entries in maps (`npmScopes.foo.npmAlwaysAuth`) that don't exist yet.
    fn hydrate(
        path: &[&str],
        value_str: &str,
    ) -> Result<AbstractValue<'static>, HydrateError>;

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

    fn from_env_string(_context: &ConfigurationContext, _value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
        unimplemented!("Configuration maps cannot be returned directly just yet");
    }

    fn hydrate(path: &[&str], value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
        let Some(key_str) = path.first() else {
            return hydrate_container(value_str);
        };

        if K::from_file_string(key_str).is_err() {
            return Err(HydrateError::InvalidKey(key_str.to_string()));
        }

        // Maps accept arbitrary keys, so the entry doesn't have to exist yet
        T::hydrate(&path[1..], value_str)
    }

    fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        let Some(key_str) = path.first() else {
            return Ok(ConfigurationEntry {
                value: AbstractValue::new_container(Container::new(self)),
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

    fn from_env_string(context: &ConfigurationContext, value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
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
                = T::from_env_string(context, item_str, None)?;

            result.push(value);
        }

        Ok(result)
    }

    fn hydrate(path: &[&str], value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
        let Some(key_str) = path.first() else {
            return hydrate_container(value_str);
        };

        // The rc file editor doesn't support editing list items in place
        Err(HydrateError::ListItem(key_str.to_string()))
    }

    fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        let Some(key_str) = path.first() else {
            return Ok(ConfigurationEntry {
                value: AbstractValue::new_container(Container::new(self)),
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

    fn from_env_string(context: &ConfigurationContext, value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
        let value
            = Path::from_file_string(value)
                .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

        // Values from the rc files are relative to the folder of the file
        // declaring them (see `merge`); values from the environment are
        // relative to the project, or to the current directory outside of one.
        let value = if value.is_relative() {
            let base = match context.preferred_cwd() {
                Some(cwd) => cwd.clone(),
                None => Path::current_dir().map_err(|e| HydrateError::InvalidValue(e.to_string()))?,
            };

            base.with_join(&value)
        } else {
            value
        };

        Ok(Self {
            value,
            source: Source::Environment,
        })
    }

    fn hydrate(path: &[&str], value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
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
                .project_cwd
                .as_ref()
                .expect("A project directory should be set since we're using the value provided through the project config")
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
    ($type:ty, $wrap:expr) => {
        impl MergeSettings for Setting<$type> {
            type Intermediate = Interpolated<$type>;

            fn from_env_string(_context: &ConfigurationContext, value: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
                let value
                    = <$type as FromFileString>::from_file_string(value)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(Self {
                    value,
                    source: Source::Environment,
                })
            }

            fn hydrate(path: &[&str], value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
                if let Some(key) = path.first() {
                    return Err(HydrateError::KeyNotFound(key.to_string()));
                }

                let value
                    = <$type as FromFileString>::from_file_string(value_str)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(AbstractValue::new(($wrap)(value)))
            }

            fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
                if let Some(key) = path.first() {
                    return Err(GetError::KeyNotFound(key.to_string()));
                }

                Ok(ConfigurationEntry {
                    value: AbstractValue::new(($wrap)(self.value.clone())),
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
                    value: Some(AbstractValue::new(($wrap)(self.value.clone()))),
                    children: None,
                });

                tree::Node {
                    label,
                    value: None,
                    children: Some(tree::TreeNodeChildren::Map(fields)),
                }
            }
        }

    };
}

macro_rules! merge_optional_settings_impl {
    ($type:ty, $wrap:expr) => {
        impl MergeSettings for Setting<Option<$type>> {
            type Intermediate = Option<Interpolated<$type>>;

            fn from_env_string(context: &ConfigurationContext, value_str: &str, _from_config: Option<Self>) -> Result<Self, HydrateError> {
                let value
                    = <Option<$type> as FromFileString>::from_file_string(value_str)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                // Non-null values must get the same treatment as when the setting
                // isn't nullable (relative paths get resolved, for instance)
                let value = match value {
                    Some(_) => Some(Setting::<$type>::from_env_string(context, value_str, None)?.value),
                    None => None,
                };

                Ok(Self {
                    value,
                    source: Source::Environment,
                })
            }

            fn hydrate(path: &[&str], value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
                if let Some(key) = path.first() {
                    return Err(HydrateError::KeyNotFound(key.to_string()));
                }

                let value
                    = <Option<$type> as FromFileString>::from_file_string(value_str)
                        .map_err(|e| HydrateError::InvalidValue(e.to_string()))?;

                Ok(AbstractValue::new(value.map($wrap)))
            }

            fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
                if !path.is_empty() {
                    return Err(GetError::KeyNotFound(path.join(".").to_string()));
                }

                Ok(ConfigurationEntry {
                    value: AbstractValue::new(self.value.clone().map($wrap)),
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

                    return inner.map_or_else(
                        || Self {
                            value: None,
                            source: Source::User,
                        },
                        |inner| Self {
                            value: Some(inner.value),
                            source: inner.source,
                        }
                    );
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

                    return inner.map_or_else(
                        || Self {
                            value: None,
                            source: Source::Project,
                        },
                        |inner| Self {
                            value: Some(inner.value),
                            source: inner.source,
                        }
                    );
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
                    value: Some(AbstractValue::new(self.value.clone().map($wrap))),
                    children: None,
                });

                tree::Node {
                    label,
                    value: None,
                    children: Some(tree::TreeNodeChildren::Map(fields)),
                }
            }
        }
    };
}

// The `file_string` variants are for types whose serde representation can't
// be written back into a configuration file (they must also be listed in
// `FILE_STRING_SERIALIZED_TYPES` in `build.rs`).
macro_rules! merge_settings {
    ($type:ty, $from_str:expr) => {
        merge_settings_impl!($type, std::convert::identity);
    };

    ($type:ty, $from_str:expr, file_string) => {
        merge_settings_impl!($type, FileString);
    };
}

macro_rules! merge_optional_settings {
    ($type:ty) => {
        merge_optional_settings_impl!($type, std::convert::identity);
    };

    ($type:ty, file_string) => {
        merge_optional_settings_impl!($type, FileString);
    };
}

include!(concat!(env!("OUT_DIR"), "/schema.rs"));

impl PackageRule {
    fn has_filter(&self) -> bool {
        self.ecosystem_filter.value.is_some()
            || self.package_filter.value.is_some()
    }
}

impl SourceRule {
    fn has_filter(&self) -> bool {
        self.ecosystem_filter.value.is_some()
            || self.registry_filter.value.is_some()
    }
}

impl Settings {
    /// The systems we need to download packages for. Each entry of
    /// `supportedArchitectures` yields one set, and a package is kept as soon
    /// as it's compatible with at least one of them.
    pub fn supported_systems(&self) -> Vec<SystemSet> {
        // An empty list would mean "no architecture at all", which is never
        // what the user wants; we fallback on the current architecture instead
        // (which is also what happens when the setting isn't set at all).
        if self.supported_architectures.is_empty() {
            return vec![SystemSet::from_current()];
        }

        self.supported_architectures.iter()
            .map(|entry| entry.to_system_set())
            .collect()
    }

    pub fn disable_age_gate(&mut self) {
        self.npm_minimal_age_gate.force(std::time::Duration::ZERO, Source::Cli);

        for scope in self.npm_scopes.values_mut() {
            scope.npm_minimal_age_gate.value = None;
        }
        for rule in &mut self.source_rules {
            rule.npm_minimal_age_gate.value = None;
        }
        for rule in &mut self.package_rules {
            rule.npm_minimal_age_gate.value = None;
        }
    }

    fn validate(&self) -> Result<(), ConfigurationError> {
        for (index, rule) in self.package_rules.iter().enumerate() {
            if !rule.has_filter() {
                return Err(ConfigurationError::ValidationError(format!(
                    "packageRules[{index}] must define at least one filter"
                )));
            }
        }

        for (index, rule) in self.source_rules.iter().enumerate() {
            if !rule.has_filter() {
                return Err(ConfigurationError::ValidationError(format!(
                    "sourceRules[{index}] must define at least one filter"
                )));
            }
        }

        Ok(())
    }
}

fn partial_option_present<T>(value: &Partial<Option<T>>) -> bool {
    matches!(value, Partial::Value(Some(_)))
}

fn validate_intermediate_settings(settings: &intermediate::Settings) -> Result<(), ConfigurationError> {
    if let Partial::Value(package_rules) = &settings.package_rules {
        for (index, rule) in package_rules.iter().enumerate() {
            if !partial_option_present(&rule.ecosystem_filter)
                && !partial_option_present(&rule.package_filter)
            {
                return Err(ConfigurationError::ValidationError(format!(
                    "packageRules[{index}] must define at least one filter"
                )));
            }
        }
    }

    if let Partial::Value(source_rules) = &settings.source_rules {
        for (index, rule) in source_rules.iter().enumerate() {
            if !partial_option_present(&rule.ecosystem_filter)
                && !partial_option_present(&rule.registry_filter)
            {
                return Err(ConfigurationError::ValidationError(format!(
                    "sourceRules[{index}] must define at least one filter"
                )));
            }
        }
    }

    Ok(())
}

/// Replaces the `current` placeholders by the values of the system we're
/// currently running on. Placeholders without a current value (the libc on
/// systems that don't have one, for instance) are simply removed.
fn resolve_current<T: PartialEq + Clone>(values: &ArchitectureFilter<T>, current: Option<&T>, placeholder: &T) -> Option<Vec<T>> {
    let values
        = values.as_list()?;

    let resolved = values.iter()
        .flat_map(|value| if value == placeholder {
            current.cloned()
        } else {
            Some(value.clone())
        })
        .collect();

    Some(resolved)
}

impl SupportedArchitectures {
    /// The set of systems covered by this entry. Each field is matched
    /// independently, so the entry covers the cross product of its fields.
    pub fn to_system_set(&self) -> SystemSet {
        let current
            = System::current();

        SystemSet {
            arch: resolve_current(&self.cpu.value, current.arch.as_ref(), &Cpu::Current),
            os: resolve_current(&self.os.value, current.os.as_ref(), &Os::Current),
            libc: resolve_current(&self.libc.value, current.libc.as_ref(), &Libc::Current),
        }
    }
}

pub struct Configuration {
    pub settings: Settings,
    pub user_config_path: Option<Path>,
    pub project_config_path: Option<Path>,
    pub requires_trust: bool,
    pub env_files: BTreeMap<String, String>,
    /// Retained so downstream predicates can re-evaluate without
    /// recomputing the env/cwd snapshot.
    pub context: ConfigurationContext,
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

    #[error("Environment file not found: {0}")]
    EnvironmentFileNotFound(String),

    #[error("Invalid environment file line: {0}")]
    InvalidEnvironmentFileLine(String),

    #[error("Invalid configuration: {0}")]
    ValidationError(String),

    #[error("Couldn't find a home directory to compute the default global folder; set the globalFolder setting (or the YARN_GLOBAL_FOLDER environment variable)")]
    MissingGlobalFolder,

    #[error("{}", format_invalid_source(.origin, .location.as_ref(), .setting.as_deref(), .message))]
    InvalidSource {
        origin: ConfigurationOrigin,
        location: Option<ConfigurationLocation>,
        setting: Option<String>,
        message: String,
    },

    #[error("{}: {setting}: Missing required setting", DataType::Path.colorize(&file.to_file_string()))]
    MissingSetting {
        file: Path,
        setting: String,
    },
}

/// Where a configuration value comes from, so that errors can point the user
/// to the place they need to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationOrigin {
    File(Path),
    EnvironmentVariable(String),
}

impl Display for ConfigurationOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // We print the absolute path (rather than one relative to the home
            // directory) so that it can be copied as-is
            ConfigurationOrigin::File(path)
                => write!(f, "{}", DataType::Path.colorize(&path.to_file_string())),

            ConfigurationOrigin::EnvironmentVariable(name)
                => write!(f, "environment variable {}", name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigurationLocation {
    pub line: usize,
    pub column: usize,
}

impl From<serde_yaml::Location> for ConfigurationLocation {
    fn from(location: serde_yaml::Location) -> Self {
        Self {line: location.line(), column: location.column()}
    }
}

fn format_invalid_source(origin: &ConfigurationOrigin, location: Option<&ConfigurationLocation>, setting: Option<&str>, message: &str) -> String {
    let mut result
        = origin.to_string();

    if let Some(location) = location {
        result.push_str(&format!(":{}:{}", location.line, location.column));
    }

    if let Some(setting) = setting {
        result.push_str(&format!(": {}", setting));
    }

    result.push_str(&format!(": {}", message));
    result
}

impl ConfigurationError {
    fn with_origin(self, origin: ConfigurationOrigin) -> Self {
        match self {
            ConfigurationError::InvalidSource {..}
                => self,

            error
                => ConfigurationError::InvalidSource {origin, location: None, setting: None, message: error.to_string()},
        }
    }
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

    #[error("List items can't be set individually ({0}); set the whole list with --json instead")]
    ListItem(String),
}

impl HydrateError {
    /// The error message without the generic prefix, for when the context
    /// already says that the value is invalid.
    fn into_message(self) -> String {
        match self {
            HydrateError::InvalidValue(message)
                => message,

            error
                => error.to_string(),
        }
    }
}

/// Wraps the error returned when an environment variable fails to override a
/// setting, so that it says which variable is at fault.
fn env_override_error(env_name: &str, setting: &str, error: HydrateError) -> ConfigurationError {
    ConfigurationError::InvalidSource {
        origin: ConfigurationOrigin::EnvironmentVariable(env_name.to_string()),
        location: None,
        setting: Some(setting.to_string()),
        message: error.into_message(),
    }
}

struct RcFile {
    path: Path,
    text: Option<String>,
}

fn rc_filename() -> String {
    std::env::var("YARN_RC_FILENAME")
        .unwrap_or_else(|_| ".yarnrc.yml".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConflictMode {
    Extend,
    Reset,
}

fn parse_conflict_mode(value: &serde_yaml::Value) -> Option<ConflictMode> {
    let serde_yaml::Value::String(value) = value else {
        return None;
    };

    match value.as_str() {
        "extend" => Some(ConflictMode::Extend),
        "reset" => Some(ConflictMode::Reset),
        _ => None,
    }
}

fn normalize_conflict_metadata(value: &mut serde_yaml::Value) -> (Option<ConflictMode>, BTreeMap<String, ConflictMode>) {
    let serde_yaml::Value::Mapping(mapping) = value else {
        return (None, BTreeMap::new());
    };

    let on_conflict_key
        = serde_yaml::Value::String("onConflict".to_string());
    let value_key
        = serde_yaml::Value::String("value".to_string());

    let root_mode
        = mapping.remove(&on_conflict_key)
            .as_ref()
            .and_then(parse_conflict_mode);

    let mut field_modes
        = BTreeMap::new();

    let mut fields_to_remove
        = Vec::new();

    for (key, field_value) in mapping.iter_mut() {
        let serde_yaml::Value::String(key) = key else {
            continue;
        };

        let serde_yaml::Value::Mapping(field_mapping) = field_value else {
            continue;
        };

        let Some(mode) = field_mapping
            .remove(&on_conflict_key)
            .as_ref()
            .and_then(parse_conflict_mode) else {
                continue;
            };

        field_modes.insert(key.clone(), mode);

        if let Some(value) = field_mapping.remove(&value_key) {
            *field_value = value;
        } else if field_mapping.is_empty() {
            fields_to_remove.push(key.clone());
        }
    }

    for key in fields_to_remove {
        mapping.remove(serde_yaml::Value::String(key));
    }

    (root_mode, field_modes)
}

fn retain_user_conflict_fields(value: &mut serde_yaml::Value, root_mode: Option<ConflictMode>, field_modes: &BTreeMap<String, ConflictMode>) {
    let serde_yaml::Value::Mapping(mapping) = value else {
        return;
    };

    let retained_fields = field_modes
        .iter()
        .filter_map(|(key, mode)| (*mode == ConflictMode::Extend).then_some(key.as_str()))
        .collect::<BTreeSet<_>>();

    if root_mode == Some(ConflictMode::Reset) {
        mapping.retain(|key, _| {
            let serde_yaml::Value::String(key) = key else {
                return false;
            };

            retained_fields.contains(key.as_str())
        });
    } else {
        for (key, mode) in field_modes {
            if *mode == ConflictMode::Reset {
                mapping.remove(serde_yaml::Value::String(key.clone()));
            }
        }
    }
}

/// Turns the path reported by serde into the name of the setting at fault;
/// `None` if the error is about the document itself.
fn setting_from_path(path: &serde_path_to_error::Path) -> Option<String> {
    if path.iter().next().is_none() {
        return None;
    }

    Some(path.to_string())
}

/// The values we deserialize have been normalized beforehand (the conflict
/// metadata got removed), so they don't remember where they were in the file.
/// To still point at the right line we replay the deserialization on the raw
/// text, and use its location if it fails on the same setting.
fn locate_setting_error(text: &str, path: &serde_path_to_error::Path) -> Option<ConfigurationLocation> {
    let deserializer
        = serde_yaml::Deserializer::from_str(text);

    let Err(error) = serde_path_to_error::deserialize::<_, intermediate::Settings>(deserializer) else {
        return None;
    };

    if error.path().to_string() != path.to_string() {
        return None;
    }

    error.inner().location()
        .map(ConfigurationLocation::from)
}

fn deserialize_intermediate_settings(rc: &RcFile, value: Option<serde_yaml::Value>) -> Result<(Partial<intermediate::Settings>, bool), ConfigurationError> {
    let Some(value) = value else {
        return Ok((Partial::Missing, false));
    };

    reset_requires_trust();

    let settings: intermediate::Settings = serde_path_to_error::deserialize(value).map_err(|error| {
        let location = rc.text.as_deref()
            .and_then(|text| locate_setting_error(text, error.path()));

        ConfigurationError::InvalidSource {
            origin: ConfigurationOrigin::File(rc.path.clone()),
            location,
            setting: setting_from_path(error.path()),
            message: error.into_inner().to_string(),
        }
    })?;

    let requires_trust
        = take_requires_trust();

    validate_intermediate_settings(&settings)
        .map_err(|error| error.with_origin(ConfigurationOrigin::File(rc.path.clone())))?;

    Ok((Partial::Value(settings), requires_trust))
}

fn parse_rc_value(rc: Option<&RcFile>) -> Result<Option<serde_yaml::Value>, ConfigurationError> {
    let Some(rc) = rc else {
        return Ok(None);
    };

    let Some(text) = &rc.text else {
        return Ok(None);
    };

    let value
        = serde_yaml::from_str::<serde_yaml::Value>(text)
            .map_err(|error| ConfigurationError::from(error).with_origin(ConfigurationOrigin::File(rc.path.clone())))?;

    Ok(Some(value))
}

fn deserialize_rc_pair(user_rc: Option<&RcFile>, project_rc: Option<&RcFile>) -> Result<(Partial<intermediate::Settings>, Partial<intermediate::Settings>, bool), ConfigurationError> {
    let mut user_value
        = parse_rc_value(user_rc)?;
    let mut project_value
        = parse_rc_value(project_rc)?;

    let user_field_modes = user_value
        .as_mut()
        .map(normalize_conflict_metadata);

    if let Some((root_mode, field_modes)) = user_field_modes {
        retain_user_conflict_fields(user_value.as_mut().unwrap(), root_mode, &field_modes);
    }

    let (project_root_mode, project_field_modes) = project_value
        .as_mut()
        .map(normalize_conflict_metadata)
        .unwrap_or((None, BTreeMap::new()));

    if let Some(user_value) = user_value.as_mut() {
        retain_user_conflict_fields(user_value, project_root_mode, &project_field_modes);
    }

    // The values can only be set if their rc file exists, but we need the
    // rc file itself to report errors
    let (user, _user_config_requires_trust) = match user_rc {
        Some(user_rc) => deserialize_intermediate_settings(user_rc, user_value)?,
        None => (Partial::Missing, false),
    };

    let (project, requires_trust) = match project_rc {
        Some(project_rc) => deserialize_intermediate_settings(project_rc, project_value)?,
        None => (Partial::Missing, false),
    };

    check_required_settings(user_rc, &user)?;
    check_required_settings(project_rc, &project)?;

    Ok((user, project, requires_trust))
}

fn check_required_settings(rc: Option<&RcFile>, settings: &Partial<intermediate::Settings>) -> Result<(), ConfigurationError> {
    let (Some(rc), Some(setting)) = (rc, settings.find_missing_setting()) else {
        return Ok(());
    };

    Err(ConfigurationError::MissingSetting {
        file: rc.path.clone(),
        setting,
    })
}

impl RcFile {
    fn try_read(dir: Option<&Path>, rc_filename: &str, last_modified_at: &mut LastModifiedAt) -> Result<Option<Self>, ConfigurationError> {
        let Some(dir) = dir else {
            return Ok(None);
        };

        let path
            = dir.with_join_str(rc_filename);

        let metadata
            = path.fs_metadata_if_exists()?;

        let Some(metadata) = metadata else {
            return Ok(Some(RcFile {path, text: None}));
        };

        let changed_at
            = metadata.modified()?
                .duration_since(UNIX_EPOCH).unwrap()
                .as_nanos();

        last_modified_at.update(changed_at);

        let text
            = path.fs_read_text_with_size(metadata.len())?;

        Ok(Some(RcFile {path, text: Some(text)}))
    }

    /// Extract the `injectEnvironmentFiles` value from the raw YAML text.
    /// Uses a minimal struct to avoid full deserialization, which would fail
    /// if config values reference env vars not yet loaded from .env files.
    fn extract_inject_environment_files(&self) -> Result<Option<Vec<String>>, ConfigurationError> {
        let Some(text) = &self.text else {
            return Ok(None);
        };

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PartialSettings {
            #[serde(default)]
            inject_environment_files: Option<Vec<String>>,
        }

        let partial: PartialSettings
            = serde_yaml::from_str(text)?;

        Ok(partial.inject_environment_files)
    }
}

impl Configuration {
    pub fn tree_node(&self) -> tree::Node<'_> {
        self.settings.tree_node(None, None)
    }

    /// Checks that `text` would be a valid content for the rc file at `path`.
    pub fn validate(path: &Path, text: &str) -> Result<(), ConfigurationError> {
        let rc
            = RcFile {path: path.clone(), text: Some(text.to_string())};

        let value
            = parse_rc_value(Some(&rc))?;

        let (project, _)
            = deserialize_intermediate_settings(&rc, value)?;

        if let Some(setting) = project.find_missing_setting() {
            return Err(ConfigurationError::ValidationError(format!("{setting}: Missing required setting")));
        }

        Ok(())
    }

    pub fn hydrate(&self, path: &[&str], value_str: &str) -> Result<AbstractValue<'static>, HydrateError> {
        Settings::hydrate(path, value_str)
    }

    pub fn get(&self, path: &[&str]) -> Result<ConfigurationEntry<'_>, GetError> {
        self.settings.get(path)
    }

    fn load_env_files(
        project_cwd: &Path,
        env_file_paths: &[String],
    ) -> Result<BTreeMap<String, String>, ConfigurationError> {
        let mut env_vars: BTreeMap<String, String>
            = BTreeMap::new();

        for path_str in env_file_paths {
            let (actual_path, optional) = if let Some(stripped) = path_str.strip_suffix('?') {
                (stripped, true)
            } else {
                (path_str.as_str(), false)
            };

            let full_path
                = project_cwd.with_join_str(actual_path);

            let metadata
                = full_path.fs_metadata_if_exists()?;

            match metadata {
                Some(metadata) => {
                    let content
                        = full_path
                            .fs_read_text_with_size(metadata.len())?;

                    for item in dotenvy::from_read_iter(content.as_bytes()) {
                        let (key, value)
                            = item
                                .map_err(|e| ConfigurationError::InvalidEnvironmentFileLine(e.to_string()))?;

                        env_vars.insert(key, value);
                    }
                },

                None => {
                    if !optional {
                        return Err(ConfigurationError::EnvironmentFileNotFound(path_str.clone()));
                    }
                },
            }
        }

        Ok(env_vars)
    }

    pub fn load(context: &ConfigurationContext, last_modified_at: &mut LastModifiedAt) -> Result<Configuration, ConfigurationError> {
        let project_cwd
            = context.project_cwd.as_ref();

        let rc_filename
            = rc_filename();

        // Read both rc files upfront (once each)
        let user_rc
            = RcFile::try_read(context.user_cwd.as_ref(), &rc_filename, last_modified_at)?;
        let project_rc
            = RcFile::try_read(project_cwd, &rc_filename, last_modified_at)?;

        // Phase 1: Extract injectEnvironmentFiles from the raw YAML text.
        // We check the project rc first, falling back to the user rc, then
        // to the default. This uses a minimal parse that tolerates config
        // values referencing env vars that don't exist yet.
        let inject_environment_files = project_rc.as_ref()
            .and_then(|rc| rc.extract_inject_environment_files().ok().flatten())
            .or_else(|| user_rc.as_ref()
                .and_then(|rc| rc.extract_inject_environment_files().ok().flatten()))
            .unwrap_or_else(|| vec![".env.yarn?".to_string()]);

        // Phase 2: Load .env files and collect variables
        let env_files = match project_cwd {
            Some(project_cwd) => Self::load_env_files(
                project_cwd,
                &inject_environment_files,
            )?,
            None => BTreeMap::new(),
        };

        // Phase 3: Set env file variables in the process environment so that
        // shellexpand::env() (used by the Interpolated deserializer) can
        // resolve them when deserializing config values like ${VAR}.
        let mut enriched_context
            = context.clone();

        for (key, value) in &env_files {
            // SAFETY: Configuration loading happens during startup before any
            // threads are spawned, so concurrent access to the environment is
            // not a concern.
            unsafe { std::env::set_var(key, value); }
            enriched_context.env.insert(key.clone(), value.clone());
        }

        // Phase 4: Deserialize the already-read rc files (no re-read)
        let user_config_path
            = user_rc.as_ref()
                .map(|rc| rc.path.clone());

        let project_config_path
            = project_rc.as_ref()
                .map(|rc| rc.path.clone());

        let (intermediate_user_config, intermediate_project_config, requires_trust)
            = deserialize_rc_pair(user_rc.as_ref(), project_rc.as_ref())?;

        let settings = Settings::merge(
            &enriched_context,
            intermediate_user_config,
            intermediate_project_config,
            || panic!("No configuration found")
        );

        let mut settings
            = settings.apply_env_overrides(&enriched_context)?;

        settings.catalogs.entry("default".to_string())
            .or_default()
            .extend(std::mem::take(&mut settings.catalog));

        settings.validate()?;

        // The default is empty when we couldn't find a home directory to
        // compute it from; it's only an error if nothing overrode it.
        if settings.global_folder.value.is_empty() {
            return Err(ConfigurationError::MissingGlobalFolder);
        }

        apply_hardened_mode(&mut settings);

        Ok(Configuration {
            settings,
            user_config_path,
            project_config_path,
            requires_trust,
            env_files,
            context: enriched_context,
        })
    }
}

/// Cascades stricter defaults (immutable installs, lockfile refresh)
/// when hardened mode is on. Only overrides settings still at
/// `Source::Default`; explicit user values keep precedence. Cascaded
/// overrides are stamped `Source::Hardened` for `yarn config`.
fn apply_hardened_mode(settings: &mut Settings) {
    if !settings.enable_hardened_mode.value {
        return;
    }

    if matches!(settings.enable_immutable_installs.source, Source::Default) {
        settings.enable_immutable_installs.force(true, Source::Hardened);
    }
}

mod duration;

mod fns;
pub use fns::*;

mod types;
pub use types::*;

#[cfg(test)]
#[path = "./roundtrip.test.rs"]
mod roundtrip_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_from_yaml(text: &str) -> Settings {
        let context = ConfigurationContext {
            env: BTreeMap::new(),
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
        )
    }

    fn settings_from_user_and_project_yaml(user_text: &str, project_text: &str) -> Settings {
        let context = ConfigurationContext {
            env: BTreeMap::new(),
            user_cwd: None,
            project_cwd: None,
            package_cwd: None,
        };

        let user
            = serde_yaml::from_str::<intermediate::Settings>(user_text)
                .expect("The configuration should be valid");

        let project
            = serde_yaml::from_str::<intermediate::Settings>(project_text)
                .expect("The configuration should be valid");

        Settings::merge(
            &context,
            Partial::Value(user),
            Partial::Value(project),
            || panic!("No configuration found"),
        )
    }

    fn supported_systems(text: &str) -> Vec<SystemSet> {
        settings_from_yaml(text).supported_systems()
    }

    fn cpu(values: &[&str]) -> Option<Vec<Cpu>> {
        Some(values.iter().map(|value| Cpu::from_file_string(value).unwrap()).collect())
    }

    fn os(values: &[&str]) -> Option<Vec<Os>> {
        Some(values.iter().map(|value| Os::from_file_string(value).unwrap()).collect())
    }

    fn libc(values: &[&str]) -> Option<Vec<Libc>> {
        Some(values.iter().map(|value| Libc::from_file_string(value).unwrap()).collect())
    }

    #[test]
    fn supported_architectures_should_support_the_legacy_object_form() {
        let sets = supported_systems(r#"
            supportedArchitectures:
              os: [darwin, linux]
              cpu: [arm64, x64]
              libc: [glibc]
        "#);

        assert_eq!(sets, vec![SystemSet {
            arch: cpu(&["arm64", "x64"]),
            os: os(&["darwin", "linux"]),
            libc: libc(&["glibc"]),
        }]);
    }

    #[test]
    fn supported_architectures_should_support_a_list_of_entries() {
        let sets = supported_systems(r#"
            supportedArchitectures:
              - os: darwin
                cpu: arm64
                libc: musl
              - os: linux
                cpu: x64
                libc: glibc
        "#);

        assert_eq!(sets, vec![SystemSet {
            arch: cpu(&["arm64"]),
            os: os(&["darwin"]),
            libc: libc(&["musl"]),
        }, SystemSet {
            arch: cpu(&["x64"]),
            os: os(&["linux"]),
            libc: libc(&["glibc"]),
        }]);
    }

    #[test]
    fn supported_architectures_should_default_the_fields_that_arent_set_on_an_entry() {
        let sets = supported_systems(r#"
            supportedArchitectures:
              - os: linux
        "#);

        let current
            = System::from_current();

        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].os, os(&["linux"]));
        assert_eq!(sets[0].arch, Some(current.arch.into_iter().collect::<Vec<_>>()));
    }

    #[test]
    fn supported_architectures_should_preserve_null_fields_inside_an_entry() {
        let sets = supported_systems(r#"
            supportedArchitectures:
              - os: foo
                cpu: [x64, ia32]
                libc: null
        "#);

        assert_eq!(sets, vec![SystemSet {
            arch: cpu(&["x64", "ia32"]),
            os: os(&["foo"]),
            libc: None,
        }]);
    }

    #[test]
    fn supported_architectures_should_fallback_on_the_current_architecture_when_the_list_is_empty() {
        let sets = supported_systems(r#"
            supportedArchitectures: []
        "#);

        assert_eq!(sets, vec![SystemSet::from_current()]);
    }

    #[test]
    fn supported_architectures_should_fallback_on_the_current_architecture_when_unset() {
        let sets = supported_systems(r#"
            enableGlobalCache: true
        "#);

        assert_eq!(sets, vec![SystemSet::from_current()]);
    }

    #[test]
    fn supported_architectures_entries_are_matched_independently() {
        let sets = supported_systems(r#"
            supportedArchitectures:
              - os: foo
                cpu: x64
                libc: glibc
              - os: bar
                cpu: ia32
                libc: musl
        "#);

        let foo_x64 = System::new(cpu(&["x64"]).unwrap().pop(), os(&["foo"]).unwrap().pop(), None)
            .to_requirements();
        let foo_ia32 = System::new(cpu(&["ia32"]).unwrap().pop(), os(&["foo"]).unwrap().pop(), None)
            .to_requirements();

        assert!(foo_x64.validate_any(&sets));

        // The cross product of both entries would have allowed it, but each
        // entry has to match on its own.
        assert!(!foo_ia32.validate_any(&sets));
    }

    fn missing_setting_error(user_text: &str, project_text: &str) -> Option<String> {
        let user_rc = RcFile {
            path: Path::from_file_string("/home/.yarnrc.yml").unwrap(),
            text: Some(user_text.to_string()),
        };

        let project_rc = RcFile {
            path: Path::from_file_string("/project/.yarnrc.yml").unwrap(),
            text: Some(project_text.to_string()),
        };

        deserialize_rc_pair(Some(&user_rc), Some(&project_rc))
            .err()
            .map(|error| error.to_string())
    }

    #[test]
    fn required_settings_should_be_reported_with_their_path() {
        assert_eq!(
            missing_setting_error("enableTelemetry: false\n", "unstableIslands:\n  x: {}\n"),
            Some("/project/.yarnrc.yml: unstableIslands.x.linker: Missing required setting".to_string()),
        );

        assert_eq!(
            missing_setting_error("enableTelemetry: false\n", "unstableIslands:\n  x:\n    workspaces: [foo]\n"),
            Some("/project/.yarnrc.yml: unstableIslands.x.linker: Missing required setting".to_string()),
        );

        assert_eq!(
            missing_setting_error("enableTelemetry: false\n", "logFilters:\n  - text: foo\n    level: info\n  - text: bar\n"),
            Some("/project/.yarnrc.yml: logFilters[1].level: Missing required setting".to_string()),
        );
    }

    #[test]
    fn required_settings_should_be_reported_in_the_home_configuration() {
        assert_eq!(
            missing_setting_error("unstableIslands:\n  x: {}\n", "enableTelemetry: false\n"),
            Some("/home/.yarnrc.yml: unstableIslands.x.linker: Missing required setting".to_string()),
        );
    }

    #[test]
    fn required_settings_should_be_accepted_when_set() {
        assert_eq!(
            missing_setting_error("enableTelemetry: false\n", "unstableIslands:\n  x:\n    linker: node-modules\nlogFilters:\n  - text: foo\n    level: info\n"),
            None,
        );
    }

    #[test]
    fn required_settings_should_be_checked_before_writing_a_configuration() {
        let error
            = Configuration::validate(&Path::from_file_string("/project/.yarnrc.yml").unwrap(), "unstableIslands:\n  x: {}\n")
                .unwrap_err();

        assert_eq!(error.to_string(), "Invalid configuration: unstableIslands.x.linker: Missing required setting");
    }

    fn context_with_project_cwd(project_cwd: Option<&str>) -> ConfigurationContext {
        ConfigurationContext {
            env: BTreeMap::new(),
            user_cwd: None,
            project_cwd: project_cwd.map(|path| Path::from_file_string(path).unwrap()),
            package_cwd: None,
        }
    }

    #[test]
    fn relative_paths_from_the_environment_should_be_resolved_from_the_project() {
        let context
            = context_with_project_cwd(Some("/project"));
        let expected
            = Path::from_file_string("/project/foo/bar").unwrap();

        assert_eq!(Setting::<Path>::from_env_string(&context, "foo/bar", None).unwrap().value, expected);
        assert_eq!(Setting::<Path>::from_env_string(&context, "./foo/bar", None).unwrap().value, expected);
        assert_eq!(Setting::<Option<Path>>::from_env_string(&context, "foo/bar", None).unwrap().value, Some(expected));
        assert_eq!(Setting::<Option<Path>>::from_env_string(&context, "null", None).unwrap().value, None);
    }

    #[test]
    fn relative_paths_from_the_environment_should_be_resolved_from_the_current_directory_outside_of_a_project() {
        let context
            = context_with_project_cwd(None);
        let expected
            = Path::current_dir().unwrap().with_join_str("foo/bar");

        assert_eq!(Setting::<Path>::from_env_string(&context, "foo/bar", None).unwrap().value, expected);
    }

    #[test]
    fn absolute_paths_from_the_environment_should_be_kept_as_is() {
        let context
            = context_with_project_cwd(Some("/project"));
        let absolute
            = Path::from_file_string("/elsewhere/foo/bar").unwrap();

        assert_eq!(Setting::<Path>::from_env_string(&context, "/elsewhere/foo/bar", None).unwrap().value, absolute);
    }

    #[test]
    fn supported_architectures_should_be_replaced_by_the_project_configuration() {
        let settings = settings_from_user_and_project_yaml(r#"
            supportedArchitectures:
              os: [darwin]
              cpu: [arm64]
        "#, r#"
            supportedArchitectures:
              os: [linux]
              cpu: [x64]
        "#);

        // The project configuration must replace the user one, not extend it;
        // otherwise a project couldn't narrow down a user-level architecture set.
        let sets = settings.supported_systems();

        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].os, os(&["linux"]));
        assert_eq!(sets[0].arch, cpu(&["x64"]));
    }

    fn rc_file(path: &str, text: &str) -> RcFile {
        RcFile {
            path: Path::from_file_string(path).unwrap(),
            text: Some(text.to_string()),
        }
    }

    fn rc_pair_error(user_text: &str, project_text: &str) -> ConfigurationError {
        let user_rc
            = rc_file("/home/.yarnrc.yml", user_text);
        let project_rc
            = rc_file("/project/.yarnrc.yml", project_text);

        let Err(error) = deserialize_rc_pair(Some(&user_rc), Some(&project_rc)) else {
            panic!("The configuration should be invalid");
        };

        error
    }

    fn invalid_source(origin: &str, location: Option<(usize, usize)>, setting: Option<&str>, message: &str) -> ConfigurationError {
        ConfigurationError::InvalidSource {
            origin: ConfigurationOrigin::File(Path::from_file_string(origin).unwrap()),
            location: location.map(|(line, column)| ConfigurationLocation {line, column}),
            setting: setting.map(|setting| setting.to_string()),
            message: message.to_string(),
        }
    }

    fn assert_error_eq(actual: ConfigurationError, expected: ConfigurationError) {
        assert_eq!(format!("{:?}", actual), format!("{:?}", expected));
    }

    #[test]
    fn errors_should_report_the_file_containing_malformed_yaml() {
        let error
            = rc_pair_error("npmScopes:\n  @foo:\n    npmAlwaysAuth: true\n", "enableScripts: false\n");

        let ConfigurationError::InvalidSource {origin, setting, message, ..} = error else {
            panic!("Unexpected error: {error:?}");
        };

        assert_eq!(origin, ConfigurationOrigin::File(Path::from_file_string("/home/.yarnrc.yml").unwrap()));
        assert_eq!(setting, None);
        assert!(message.contains("at line 2 column 3"), "Unexpected message: {message}");
    }

    #[test]
    fn errors_should_report_the_setting_with_an_invalid_scalar() {
        assert_error_eq(
            rc_pair_error("enableTelemetry: false\n", "enableTelemetry: false\nenableScripts: maybe\n"),
            invalid_source("/project/.yarnrc.yml", Some((2, 16)), Some("enableScripts"), "Invalid value: maybe"),
        );
    }

    #[test]
    fn errors_should_report_the_path_of_an_invalid_nested_setting() {
        assert_error_eq(
            rc_pair_error("enableTelemetry: false\n", "npmScopes:\n  foo:\n    npmAlwaysAuth: 12\n"),
            invalid_source("/project/.yarnrc.yml", Some((3, 20)), Some("npmScopes.foo.npmAlwaysAuth"), "Invalid value: 12"),
        );
    }

    #[test]
    fn errors_should_report_the_type_mismatch_of_an_interpolated_setting() {
        assert_error_eq(
            rc_pair_error("enableTelemetry: false\n", "npmRegistryServer: [foo]\n"),
            invalid_source("/project/.yarnrc.yml", Some((1, 20)), Some("npmRegistryServer"), "invalid type: sequence, expected a string"),
        );
    }

    #[test]
    fn errors_should_report_invalid_enum_values() {
        assert_error_eq(
            rc_pair_error("enableTelemetry: false\n", "nodeLinker: foo\n"),
            invalid_source("/project/.yarnrc.yml", Some((1, 13)), Some("nodeLinker"), "Invalid enum value (foo)"),
        );
    }

    #[test]
    fn errors_should_report_the_home_configuration_when_it_is_the_one_at_fault() {
        assert_error_eq(
            rc_pair_error("enableScripts: maybe\n", "enableScripts: true\n"),
            invalid_source("/home/.yarnrc.yml", Some((1, 16)), Some("enableScripts"), "Invalid value: maybe"),
        );
    }

    #[test]
    fn errors_should_report_the_index_of_invalid_list_entries() {
        assert_error_eq(
            rc_pair_error("enableTelemetry: false\n", "packageRules:\n  - npmMinimalAgeGate: 1d\n"),
            invalid_source("/project/.yarnrc.yml", None, None, "Invalid configuration: packageRules[0] must define at least one filter"),
        );
    }

    #[test]
    fn errors_should_report_the_environment_variable_at_fault() {
        let mut env
            = BTreeMap::new();

        env.insert("YARN_ENABLE_SCRIPTS".to_string(), "maybe".to_string());

        let context = ConfigurationContext {
            env,
            user_cwd: None,
            project_cwd: None,
            package_cwd: None,
        };

        let Err(error) = settings_from_yaml("enableScripts: true\n").apply_env_overrides(&context) else {
            panic!("The configuration should be invalid");
        };

        assert_error_eq(error, ConfigurationError::InvalidSource {
            origin: ConfigurationOrigin::EnvironmentVariable("YARN_ENABLE_SCRIPTS".to_string()),
            location: None,
            setting: Some("enableScripts".to_string()),
            message: "Invalid value: maybe".to_string(),
        });
    }

    #[test]
    fn errors_should_be_formatted_on_a_single_line() {
        let error
            = invalid_source("/project/.yarnrc.yml", Some((3, 20)), Some("npmScopes.foo.npmAlwaysAuth"), "Invalid value: 12");

        assert_eq!(error.to_string(), "/project/.yarnrc.yml:3:20: npmScopes.foo.npmAlwaysAuth: Invalid value: 12");
    }

    #[test]
    fn supported_architectures_should_use_the_user_configuration_when_the_project_doesnt_set_it() {
        let settings = settings_from_user_and_project_yaml(r#"
            supportedArchitectures:
              os: [darwin]
              cpu: [arm64]
        "#, r#"
            enableTelemetry: false
        "#);

        let sets = settings.supported_systems();

        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].os, os(&["darwin"]));
        assert_eq!(sets[0].arch, cpu(&["arm64"]));
    }
}

// Rust doesn't support specialization, so we can't have a blanket implementation for FromStr
// and a different one for Option<T: FromStr>; instead we manually generate whatever we need.
merge_settings!(std::time::Duration, |s: &str| FromFileString::from_file_string(s).unwrap(), file_string);
merge_optional_settings!(std::time::Duration, file_string);

merge_settings!(String, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(bool, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(usize, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(u64, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(String);
merge_optional_settings!(bool);
merge_optional_settings!(usize);
merge_optional_settings!(u64);

merge_settings!(zpm_formats::CompressionAlgorithm, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(zpm_formats::CompressionAlgorithm);

merge_settings!(zpm_primitives::Descriptor, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::VersionFilter, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Ident, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::IdentGlob, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Locator, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::PeerRange, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Range, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_primitives::Reference, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(zpm_primitives::Descriptor);
merge_optional_settings!(zpm_primitives::VersionFilter);
merge_optional_settings!(zpm_primitives::Ident);
merge_optional_settings!(zpm_primitives::IdentGlob);
merge_optional_settings!(zpm_primitives::Locator);
merge_optional_settings!(zpm_primitives::PeerRange);
merge_optional_settings!(zpm_primitives::Range);
merge_optional_settings!(zpm_primitives::Reference);

merge_settings!(zpm_semver::Range, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(zpm_semver::Range);

merge_settings!(zpm_semver::RangeKind, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(zpm_semver::RangeKind);

merge_settings!(zpm_utils::Cpu, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Glob, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Libc, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Os, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(zpm_utils::Secret<String>, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(zpm_utils::Cpu);
merge_optional_settings!(zpm_utils::Glob);
merge_optional_settings!(zpm_utils::Libc);
merge_optional_settings!(zpm_utils::Os);
merge_optional_settings!(zpm_utils::Secret<String>);

merge_settings!(crate::types::ArchitectureFilter<zpm_utils::Cpu>, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::ArchitectureFilter<zpm_utils::Libc>, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::ArchitectureFilter<zpm_utils::Os>, |s: &str| FromFileString::from_file_string(s).unwrap());

merge_settings!(crate::types::NodeLinker, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::NodePackageMapType, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::IslandLinker, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::PythonPreference, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::LazyInstallMode, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::PnpFallbackMode, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::NmHoistingLimits, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::NmMode, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::WinLinkType, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::LogLevel, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::NpmPublishAccess, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_settings!(crate::types::EcosystemFilter, |s: &str| FromFileString::from_file_string(s).unwrap());
merge_optional_settings!(crate::types::NodeLinker);
merge_optional_settings!(crate::types::NodePackageMapType);
merge_optional_settings!(crate::types::IslandLinker);
merge_optional_settings!(crate::types::LazyInstallMode);
merge_optional_settings!(crate::types::PnpFallbackMode);
merge_optional_settings!(crate::types::NmHoistingLimits);
merge_optional_settings!(crate::types::NmMode);
merge_optional_settings!(crate::types::WinLinkType);
merge_optional_settings!(crate::types::LogLevel);
merge_optional_settings!(crate::types::NpmPublishAccess);
merge_optional_settings!(crate::types::EcosystemFilter);
merge_optional_settings!(Path);
