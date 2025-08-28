use std::sync::{LazyLock, Mutex};

use zpm_utils::{FromFileString, Path, ToFileString, ToHumanString};
use serde::{Deserialize, Serialize};

use crate::{error::Error, primitives::Ident, settings::{EnvConfig, ProjectConfig, UserConfig}};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Password {
    value: String,
}

impl FromFileString for Password {
    type Error = Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(Password {
            value: s.to_string(),
        })
    }
}

impl ToFileString for Password {
    fn to_file_string(&self) -> String {
        self.value.clone()
    }
}

impl ToHumanString for Password {
    fn to_print_string(&self) -> String {
        "[hidden]".to_string()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ConfigPaths {
    pub rc_path: Option<Path>,
    pub project_cwd: Option<Path>,
    pub package_cwd: Option<Path>,
}

pub static CONFIG_PATH: LazyLock<Mutex<Option<ConfigPaths>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
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

    pub fn new(project_cwd: Option<Path>, package_cwd: Option<Path>) -> Result<Self, Error> {
        let user_yarnrc_path = Path::home_dir()?
            .map(|dir| dir.with_join_str(".yarnrc.yml"));

        let project_yarnrc_path = project_cwd.clone()
            .map(|cwd| cwd.with_join_str(".yarnrc.yml"));

        *CONFIG_PATH.lock().unwrap() = Some(ConfigPaths {
            rc_path: user_yarnrc_path.clone(),
            project_cwd: project_cwd.clone(),
            package_cwd: package_cwd.clone(),
        });

        let mut user_config
            = Config::import_config::<UserConfig>(user_yarnrc_path.clone());

        user_config.path = user_yarnrc_path;

        *CONFIG_PATH.lock().unwrap() = Some(ConfigPaths {
            rc_path: project_yarnrc_path.clone(),
            project_cwd: project_cwd.clone(),
            package_cwd: package_cwd.clone(),
        });

        let mut project_config
            = Config::import_config::<ProjectConfig>(project_yarnrc_path.clone());

        project_config.path = project_yarnrc_path;

        *CONFIG_PATH.lock().unwrap() = None;

        Ok(Config {
            user: user_config,
            project: project_config,
        })
    }

    pub fn registry_base_for(&self, _ident: &Ident) -> String {
        self.project.npm_registry_server.value.clone()
    }
}
