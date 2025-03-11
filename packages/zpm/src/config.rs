use std::sync::{LazyLock, Mutex};

use arca::{Path, ToArcaPath};
use colored::Colorize;
use serde::{Deserialize, Deserializer};
use zpm_utils::{FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, primitives::Ident, settings::{EnvConfig, ProjectConfig, UserConfig}};

pub static CONFIG_PATH: LazyLock<Mutex<Option<Path>>> = LazyLock::new(|| Mutex::new(None));

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

impl<T: ToFileString> ToFileString for StringLikeField<T> {
    fn to_file_string(&self) -> String {
        self.value.to_file_string()
    }
}

impl<T: ToHumanString> ToHumanString for StringLikeField<T> {
    fn to_print_string(&self) -> String {
        self.value.to_print_string()
    }
}

impl<T: FromFileString> FromFileString for StringLikeField<T> where Error: From<<T as FromFileString>::Error> {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        let value = T::from_file_string(raw)?;

        Ok(Self {value, source: SettingSource::Project})
    }
}

impl<'de, T: FromFileString> Deserialize<'de> for StringLikeField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let str = String::deserialize(deserializer)?;

        let value = T::from_file_string(&str)
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

impl FromFileString for BoolField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
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

impl ToFileString for BoolField {
    fn to_file_string(&self) -> String {
        self.value.to_string()
    }
}

impl ToHumanString for BoolField {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(255, 153, 0).to_string()
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

impl ToFileString for UintField {
    fn to_file_string(&self) -> String {
        self.value.to_string()
    }
}

impl ToHumanString for UintField {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(255, 255, 0).to_string()
    }
}

impl FromFileString for UintField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(UintField {value: raw.parse()?, source: SettingSource::Project})
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

impl<T: ToFileString> ToFileString for JsonField<T> {
    fn to_file_string(&self) -> String {
        self.value.to_file_string()
    }
}

impl<T: ToHumanString> ToHumanString for JsonField<T> {
    fn to_print_string(&self) -> String {
        self.value.to_print_string()
    }
}

impl<T: for<'a> Deserialize<'a>> FromFileString for JsonField<T> {
    type Error = sonic_rs::Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        let value = sonic_rs::from_str::<T>(raw)?;

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

impl<T: ToFileString> ToFileString for VecField<T> {
    fn to_file_string(&self) -> String {
        self.value.iter().map(|v| v.to_file_string()).collect::<Vec<_>>().join(",")
    }
}

impl<T: ToHumanString> ToHumanString for VecField<T> {
    fn to_print_string(&self) -> String {
        self.value.iter().map(|v| v.to_print_string()).collect::<Vec<_>>().join(",")
    }
}

impl<T: FromFileString + for<'a> Deserialize<'a>> FromFileString for VecField<T> {
    type Error = sonic_rs::Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        if raw.starts_with('[') {
            let value = sonic_rs::from_str::<Vec<T>>(raw)?;

            Ok(Self {value})  
        } else {
            let value = T::from_file_string(raw)
                .map_err(|_| serde::de::Error::custom("Failed to call FromFileString"))?;

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

impl<T: ToFileString> ToFileString for EnumField<T> {
    fn to_file_string(&self) -> String {
        self.value.to_file_string()
    }
}

impl<T: ToHumanString> ToHumanString for EnumField<T> {
    fn to_print_string(&self) -> String {
        self.value.to_print_string()
    }
}

impl<T: for<'a> Deserialize<'a>> FromFileString for EnumField<T> {
    type Error = sonic_rs::Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        let str = sonic_rs::to_string(&raw)?;
        let value = sonic_rs::from_str::<T>(&str)?;

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

impl ToFileString for PathField {
    fn to_file_string(&self) -> String {
        self.value.to_string()
    }
}

impl ToHumanString for PathField {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(153, 153, 255).to_string()
    }
}

impl FromFileString for PathField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
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

impl ToFileString for Glob {
    fn to_file_string(&self) -> String {
        self.pattern.clone()
    }
}

impl ToHumanString for Glob {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(153, 153, 255).to_string()
    }
}

impl FromFileString for Glob {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
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
    sonic_rs::from_str("{}").unwrap()
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
        let mut user_config = Config::import_config::<UserConfig>(user_yarnrc_path.clone());
        user_config.path = user_yarnrc_path;

        *CONFIG_PATH.lock().unwrap() = project_yarnrc_path.clone();
        let mut project_config = Config::import_config::<ProjectConfig>(project_yarnrc_path.clone());
        project_config.path = project_yarnrc_path;

        *CONFIG_PATH.lock().unwrap() = None;

        Config {
            user: user_config,
            project: project_config,
        }
    }

    pub fn registry_base_for(&self, _ident: &Ident) -> String {
        self.project.npm_registry_server.value.clone()
    }
}
