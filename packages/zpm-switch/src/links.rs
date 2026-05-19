use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use zpm_parsers::JsonDocument;
use zpm_utils::{DataType, Hash64, IoResultExt, Path, ToFileString, ToHumanString};

use crate::errors::Error;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub project_cwd: Path,
    pub link_target: LinkTarget,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FolderConfig {
    pub project_cwd: Path,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<LinkTarget>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted: Option<bool>,
}

impl FolderConfig {
    pub fn new(project_cwd: Path) -> Self {
        Self {
            project_cwd,
            link_target: None,
            trusted: None,
        }
    }

    pub fn into_link(self) -> Option<Link> {
        Some(Link {
            project_cwd: self.project_cwd,
            link_target: self.link_target?,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.link_target.is_none() && self.trusted.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum LinkTarget {
    Local {
        bin_path: Path,
    },

    Migration,
}

impl ToHumanString for LinkTarget {
    fn to_print_string(&self) -> String {
        match self {
            Self::Local {bin_path}
                => bin_path.to_print_string(),

            Self::Migration
                => DataType::Code.colorize("Project migration"),
        }
    }
}

pub fn links_dir() -> Result<Path, Error> {
    let links_dir = Path::home_dir()?
        .ok_or(Error::MissingHomeFolder)?
        .with_join_str(".yarn/switch/links");

    Ok(links_dir)
}

fn config_path(project_cwd: &Path) -> Result<Path, Error> {
    let hash
        = Hash64::from_data(project_cwd.to_file_string().as_bytes());

    Ok(links_dir()?
        .with_join_str(format!("{}.json", hash.short())))
}

fn save_config(config: &FolderConfig) -> Result<(), Error> {
    let config_path
        = config_path(&config.project_cwd)?;

    if config.is_empty() {
        config_path
            .fs_rm()
            .ok_missing()?;

        return Ok(());
    }

    config_path
        .fs_create_parent()?
        .fs_write(JsonDocument::to_string(config)?)?;

    Ok(())
}

pub fn get_config(project_cwd: &Path) -> Result<Option<FolderConfig>, Error> {
    let config_path
        = config_path(project_cwd)?;

    let config = config_path
        .fs_read_text()
        .ok_missing()?
        .and_then(|config| JsonDocument::hydrate_from_str::<FolderConfig>(&config).ok());

    Ok(config)
}

pub fn set_link(link: &Link) -> Result<(), Error> {
    let mut config
        = get_config(&link.project_cwd)?
            .unwrap_or_else(|| FolderConfig::new(link.project_cwd.clone()));

    config.link_target = Some(link.link_target.clone());

    save_config(&config)
}

pub fn unset_link(project_cwd: &Path) -> Result<(), Error> {
    let mut config
        = get_config(project_cwd)?
            .unwrap_or_else(|| FolderConfig::new(project_cwd.clone()));

    config.link_target = None;

    save_config(&config)
}

pub fn list_configs() -> Result<BTreeSet<FolderConfig>, Error> {
    let links_dir
        = links_dir()?;

    let Some(dir_entries) = links_dir.fs_read_dir().ok_missing()? else {
        return Ok(BTreeSet::new());
    };

    let configs = dir_entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map_or(false, |f| f.is_file()))
        .filter_map(|link_path| Path::try_from(link_path.path()).ok())
        .filter_map(|link_path| link_path.fs_read_text().ok())
        .filter_map(|contents| JsonDocument::hydrate_from_str::<FolderConfig>(&contents).ok())
        .collect::<BTreeSet<_>>();

    Ok(configs)
}

pub fn list_links() -> Result<BTreeSet<Link>, Error> {
    let links = list_configs()?
        .into_iter()
        .filter_map(FolderConfig::into_link)
        .collect::<BTreeSet<_>>();

    Ok(links)
}

pub fn get_link(path: &Path) -> Result<Option<Link>, Error> {
    Ok(get_config(path)?
        .and_then(FolderConfig::into_link))
}

pub fn get_trusted(path: &Path) -> Result<Option<bool>, Error> {
    Ok(get_config(path)?
        .and_then(|config| config.trusted))
}

pub fn set_trusted(project_cwd: &Path, trusted: Option<bool>) -> Result<(), Error> {
    let mut config
        = get_config(project_cwd)?
            .unwrap_or_else(|| FolderConfig::new(project_cwd.clone()));

    config.trusted = trusted;

    save_config(&config)
}
