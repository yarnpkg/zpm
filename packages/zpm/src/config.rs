use std::str::FromStr;

use arca::{Path, ToArcaPath};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};

use crate::{error::Error, primitives::Ident, settings::{ProjectConfig, UserConfig}};

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

impl<T: FromStr> FromStr for StringLikeField<T> {
    type Err = T::Err;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let value = T::from_str(raw)?;

        Ok(Self {value, source: SettingSource::Env})
    }
}

impl<'de, T: FromStr> Deserialize<'de> for StringLikeField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let str = String::deserialize(deserializer)?;

        let value = T::from_str(&str)
            .map_err(|_| serde::de::Error::custom("Failed to call FromStr"))?;

        Ok(Self {value, source: SettingSource::Default})
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

impl<'de, T: DeserializeOwned> FromStr for JsonField<T> {
    type Err = serde_json::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
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

impl<'de, T: DeserializeOwned + FromStr> FromStr for VecField<T> {
    type Err = serde_json::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.starts_with('[') {
            let value = serde_json::from_str::<Vec<T>>(raw)?;

            Ok(Self {value})  
        } else {
            let value = T::from_str(raw)
                .map_err(|_| serde::de::Error::custom("Failed to call FromStr"))?;

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

impl<'de, T: DeserializeOwned> FromStr for EnumField<T> {
    type Err = serde_json::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
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

impl FromStr for Glob {
    type Err = Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Ok(Glob {pattern: raw.to_string()})
    }
}

pub type StringField = StringLikeField<String>;
pub type GlobField = StringLikeField<Glob>;
pub type BoolField = JsonField<bool>;

impl<'de> Deserialize<'de> for Glob {
    fn deserialize<D>(deserializer: D) -> Result<Glob, D::Error> where D: Deserializer<'de> {
        Ok(Glob { pattern: String::deserialize(deserializer)? })
    }
}

pub struct Config {
    pub user: UserConfig,
    pub project: ProjectConfig,
}

impl Config {
    fn import_config<'a, T>(path: Option<Path>) -> T where T: for<'de> Deserialize<'de> {
        let content = path
            .and_then(|path| path.fs_read_text().ok())
            .unwrap_or_default();

        serde_yaml::from_str::<T>(&content)
            .unwrap()
    }

    pub fn new(cwd: Option<Path>) -> Self {
        let user_yarnrc_path = std::env::home_dir()
            .map(|dir| dir.to_arca().with_join_str(".yarnrc.yml"));

        let project_yarnrc_path = cwd
            .map(|cwd| cwd.with_join_str(".yarnrc.yml"));

        Config {
            user: Config::import_config::<UserConfig>(user_yarnrc_path),
            project: Config::import_config::<ProjectConfig>(project_yarnrc_path),
        }
    }

    pub fn registry_url_for(&self, _ident: &Ident) -> String {
        self.project.npm_registry_server.value.clone()
    }
}
