use std::{str::FromStr, sync::{LazyLock, Mutex}};

use arca::{Path, ToArcaPath};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};

use crate::{error::Error, primitives::{reference, Ident}, settings::{EnvConfig, ProjectConfig, UserConfig}};

pub static CONFIG_PATH: LazyLock<Mutex<Option<Path>>> = LazyLock::new(|| Mutex::new(None));

pub trait FromEnv: Sized {
    type Err;

    fn from_env(raw: &str) -> Result<Self, Self::Err>;
}

impl FromEnv for String {
    type Err = <std::string::String as FromStr>::Err;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        Ok(raw.to_string())
    }
}

#[derive(Debug, Default, Clone)]
pub enum SettingSource {
    #[default]
    Default,
    User,
    Project,
    Env,
}

#[derive(Debug, Clone)]
pub struct StringLikeField<T> {
    pub value: T,
    pub source: SettingSource,
}

impl<T> StringLikeField<T> {
    pub fn new(value: T) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl<T: FromEnv> FromEnv for StringLikeField<T> {
    type Err = T::Err;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        let value = T::from_env(raw)?;

        Ok(Self {value, source: SettingSource::Env})
    }
}

impl<'de, T: FromEnv> Deserialize<'de> for StringLikeField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let str = String::deserialize(deserializer)?;

        let value = T::from_env(&str)
            .map_err(|_| serde::de::Error::custom("Failed to call FromEnv"))?;

        Ok(Self {value, source: SettingSource::Default})
    }
}

#[derive(Debug, Clone)]
pub struct BoolField {
    pub value: bool,
    pub source: SettingSource,
}

impl BoolField {
    pub fn new(value: bool) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl FromEnv for BoolField {
    type Err = <bool as FromStr>::Err;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        let value = match raw {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => panic!("Invalid boolean value"),
        };

        Ok(BoolField {value, source: SettingSource::Env})
    }
}

impl<'de> Deserialize<'de> for BoolField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let value = bool::deserialize(deserializer)?;

        Ok(BoolField {value, source: SettingSource::Default})
    }
}

#[derive(Debug, Clone)]
pub struct UintField {
    pub value: u64,
    pub source: SettingSource,
}

impl UintField {
    pub fn new(value: u64) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl FromEnv for UintField {
    type Err = <u64 as FromStr>::Err;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        Ok(UintField {value: raw.parse()?, source: SettingSource::Env})
    }
}

impl<'de> Deserialize<'de> for UintField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(UintField {value: u64::deserialize(deserializer)?, source: SettingSource::Default})
    }
}

#[derive(Debug, Clone)]
pub struct JsonField<T> {
    pub value: T,
    pub source: SettingSource,
}

impl<T> JsonField<T> {
    pub fn new(value: T) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl<T: DeserializeOwned> FromEnv for JsonField<T> {
    type Err = serde_json::Error;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        let value = serde_json::from_str::<T>(raw)?;

        Ok(JsonField {value, source: SettingSource::Env})
    }
}

impl<'de, T> Deserialize<'de> for JsonField<T> where T: Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let value = T::deserialize(deserializer)?;

        Ok(JsonField {value, source: SettingSource::Default})
    }
}

#[derive(Debug, Clone)]
pub struct VecField<T> {
    pub value: Vec<T>,
}

impl<T> VecField<T> {
    pub fn new(value: Vec<T>) -> Self {
        Self {value}
    }
}

impl<T: DeserializeOwned + FromEnv> FromEnv for VecField<T> {
    type Err = serde_json::Error;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        if raw.starts_with('[') {
            let value = serde_json::from_str::<Vec<T>>(raw)?;

            Ok(Self {value})  
        } else {
            let value = T::from_env(raw)
                .map_err(|_| serde::de::Error::custom("Failed to call FromEnv"))?;

            Ok(Self {value: vec![value]})
        }
    }
}

impl<'de, T> Deserialize<'de> for VecField<T> where T: Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let value = Vec::<T>::deserialize(deserializer)?;

        Ok(VecField {value})
    }
}

#[derive(Debug, Clone)]
pub struct EnumField<T> {
    pub value: T,
    pub source: SettingSource,
}

impl<T> EnumField<T> {
    pub fn new(value: T) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl<T: DeserializeOwned> FromEnv for EnumField<T> {
    type Err = serde_json::Error;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        let str = serde_json::to_string(&raw)?;
        let value = serde_json::from_str::<T>(&str)?;

        Ok(EnumField {value, source: SettingSource::Env})
    }
}

impl<'de, T> Deserialize<'de> for EnumField<T> where T: Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let value = T::deserialize(deserializer)?;

        Ok(EnumField {value, source: SettingSource::Default})
    }
}

#[derive(Debug, Clone)]
pub struct PathField {
    pub value: Path,
    pub source: SettingSource,
}

impl PathField {
    pub fn new(value: Path) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl FromEnv for PathField {
    type Err = Error;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        let mut value = Path::from(raw);

        if !value.is_absolute() {
            value = CONFIG_PATH.lock().unwrap()
                .as_ref().unwrap()
                .dirname().unwrap()
                .with_join(&value);
        }

        Ok(Self {value, source: SettingSource::Env})
    }
}

impl<'de> Deserialize<'de> for PathField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let str = String::deserialize(deserializer)?;

        let value = CONFIG_PATH.lock().unwrap()
            .as_ref().unwrap()
            .dirname().unwrap()
            .with_join_str(&str);

        Ok(PathField {value, source: SettingSource::Default})
    }
}

#[derive(Debug, Clone)]
pub struct Glob {
    pub pattern: String,
}

impl Glob {
    pub fn to_regex_string(&self) -> String {
        wax::Glob::new(&self.pattern)
            .unwrap()
            .to_regex()
            .to_string()
    }
}

impl FromEnv for Glob {
    type Err = Error;

    fn from_env(raw: &str) -> Result<Self, Self::Err> {
        Ok(Glob {pattern: raw.to_string()})
    }
}

pub type StringField = StringLikeField<String>;
pub type GlobField = StringLikeField<Glob>;

impl<'de> Deserialize<'de> for Glob {
    fn deserialize<D>(deserializer: D) -> Result<Glob, D::Error> where D: Deserializer<'de> {
        Ok(Glob { pattern: String::deserialize(deserializer)? })
    }
}

#[derive(Debug)]
pub struct Config {
    pub user: UserConfig,
    pub project: ProjectConfig,
}

pub static ENV_CONFIG: LazyLock<EnvConfig> = LazyLock::new(|| {
    *CONFIG_PATH.lock().unwrap() = None;
    serde_json::from_str("{}").unwrap()
});

impl Config {
    fn import_config<'a, T>(path: Option<Path>) -> T where T: for<'de> Deserialize<'de> {
        let content = path
            .and_then(|path| path.fs_read_text().ok())
            .unwrap_or_default();

        serde_yaml::from_str::<T>(&content)
            .unwrap()
    }

    pub fn new(cwd: Option<Path>) -> Self {
        #[allow(deprecated)]
        let user_yarnrc_path = std::env::home_dir()
            .map(|dir| dir.to_arca().with_join_str(".yarnrc.yml"));

        let project_yarnrc_path = cwd
            .map(|cwd| cwd.with_join_str(".yarnrc.yml"));

        *CONFIG_PATH.lock().unwrap() = user_yarnrc_path.clone();
        let user_config = Config::import_config::<UserConfig>(user_yarnrc_path);

        *CONFIG_PATH.lock().unwrap() = project_yarnrc_path.clone();
        let project_config = Config::import_config::<ProjectConfig>(project_yarnrc_path);

        *CONFIG_PATH.lock().unwrap() = None;

        Config {
            user: user_config,
            project: project_config,
        }
    }

    pub fn registry_url_for(&self, _ident: &Ident) -> String {
        self.project.npm_registry_server.value.clone()
    }

    pub fn registry_url_for_package_data(&self, reference: &reference::RegistryReference) -> String {
        let registry_base = self.registry_url_for(&reference.ident);
        let url = format!("{}/{}/-/{}-{}.tgz", registry_base, reference.ident, reference.ident.name(), reference.version);

        url
    }
}
