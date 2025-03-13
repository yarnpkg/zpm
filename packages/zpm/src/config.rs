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
    Unknown,
    Default,
    User,
    Project,
    Env,
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
