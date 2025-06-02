use std::{collections::BTreeMap, sync::LazyLock};

use bincode::{Decode, Encode};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};
use zpm_utils::ToFileString;

use crate::{build, error::Error, fetchers::PackageData, primitives::Locator, system};

static UNPLUG_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

static UNPLUG_EXT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.(exe|bin|h|hh|hpp|c|cc|cpp|java|jar|node)$").unwrap()
});

/**
 * The package metadata struct contains various fields that instruct the
 * package manager (the linker, mostly) about the content of the package.
 * 
 * We compute this struct the first time the package is fetched and store it
 * inside the install state so we can avoid having to recompute it every time,
 * which would otherwise require to parse the zip archives every time.
 */
#[derive(Clone, Debug, Encode, Decode, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentFlags {
    /**
     * The build scripts that should be run after the package got installed.
     */
    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub build_commands: Vec<build::Command>,

    /**
     * Whether the package requests to be extracted to the filesystem.
     */
    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub prefer_extracted: Option<bool>,

    /**
     * Whether Yarn thinks the package should be extracted, based on its
     * content.
     */
    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub suggest_extracted: bool,
}

impl Default for ContentFlags {
    fn default() -> Self {
        Self {
            build_commands: vec![],
            prefer_extracted: None,
            suggest_extracted: false,
        }
    }
}

#[serde_as]
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(default)]
    r#type: Option<String>,

    #[serde(default)]
    requirements: system::Requirements,

    #[serde(default)]
    prefer_unplugged: Option<bool>,

    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError")]
    scripts: BTreeMap<String, String>,
}

impl ContentFlags {
    pub fn extract(locator: &Locator, package_data: &PackageData) -> Result<Self, Error> {
        let package_bytes = if let PackageData::Zip {archive_path, ..} = &package_data {
            archive_path.fs_read()?
        } else {
            return Ok(Self::default());
        };

        let first_entry = zpm_formats::zip::first_entry_from_zip(&package_bytes)
            .unwrap();

        let meta_manifest = serde_json::from_slice::<Manifest>(&first_entry.data)
            .unwrap();

        let mut build_commands = UNPLUG_SCRIPTS.iter()
            .filter_map(|k| meta_manifest.scripts.get(*k))
            .map(|s| build::Command::Script {script: s.clone()})
            .collect::<Vec<_>>();

        let entries
            = zpm_formats::zip::entries_from_zip(&package_bytes)?;

        if build_commands.is_empty() {
            let binding_gyp_name
                = format!("node_modules/{}/binding.gyp", locator.ident.as_str());
    
            if entries.iter().any(|entry| entry.name == binding_gyp_name) {
                build_commands.push(build::Command::Program {
                    name: "node-gyp".to_string(),
                    args: vec!["rebuild".to_string()],
                });
            }
        }

        let prefer_extracted = meta_manifest.prefer_unplugged;
        let suggest_extracted = entries.iter().any(|entry| UNPLUG_EXT_REGEX.is_match(&entry.name));

        let flags = ContentFlags {
            build_commands,
            prefer_extracted,
            suggest_extracted,
        };

        Ok(flags)
    }
}
