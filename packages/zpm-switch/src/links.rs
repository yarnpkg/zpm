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

pub fn set_link(link: &Link) -> Result<(), Error> {
    let hash
        = Hash64::from_data(link.project_cwd.to_file_string().as_bytes());

    let link_path = links_dir()?
        .with_join_str(format!("{}.json", hash.short()));

    link_path
        .fs_create_parent()?
        .fs_write(JsonDocument::to_string(link)?)?;

    Ok(())
}

pub fn unset_link(project_cwd: &Path) -> Result<(), Error> {
    let hash
        = Hash64::from_data(project_cwd.to_file_string().as_bytes());

    let link_path = links_dir()?
        .with_join_str(format!("{}.json", hash.short()));

    link_path
        .fs_rm()?;

    Ok(())
}

pub fn list_links() -> Result<BTreeSet<Link>, Error> {
    let links_dir
        = links_dir()?;

    let Some(dir_entries) = links_dir.fs_read_dir().ok_missing()? else {
        return Ok(BTreeSet::new());
    };

    let links = dir_entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map_or(false, |f| f.is_file()))
        .filter_map(|link_path| Path::try_from(link_path.path()).ok())
        .filter_map(|link_path| link_path.fs_read_text().ok())
        .filter_map(|contents| JsonDocument::hydrate_from_str::<Link>(&contents).ok())
        .collect::<BTreeSet<_>>();

    Ok(links)
}

pub fn get_link(path: &Path) -> Result<Option<Link>, Error> {
    let hash
        = Hash64::from_data(path.to_file_string().as_bytes());

    let link_path = links_dir()?
        .with_join_str(format!("{}.json", hash.short()));

    let link = link_path
        .fs_read_text()
        .ok_missing()?
        .and_then(|link| JsonDocument::hydrate_from_str::<Link>(&link).ok());

    Ok(link)
}
