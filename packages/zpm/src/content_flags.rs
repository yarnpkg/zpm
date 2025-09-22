use std::{collections::BTreeMap, sync::LazyLock};

use bincode::{Decode, Encode};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::Path;

use crate::{
    build, error::Error, fetchers::PackageData, manifest::bin::BinField, system
};

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
     * The binaries that should be made available to the package.
     */
    #[serde(default)]
    pub binaries: BTreeMap<String, Path>,

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
            binaries: BTreeMap::new(),
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
    name: Option<Ident>,

    #[serde(default)]
    r#type: Option<String>,

    #[serde(default)]
    bin: Option<BinField>,

    #[serde(default)]
    requirements: system::Requirements,

    #[serde(default)]
    prefer_unplugged: Option<bool>,

    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError")]
    scripts: BTreeMap<String, String>,
}

fn extract_binaries(name: Option<Ident>, bin: Option<BinField>) -> BTreeMap<String, Path> {
    let Some(bin) = bin else {
        return BTreeMap::new();
    };

    match bin {
        BinField::String(path) => name
            .map(|name| BTreeMap::from_iter([(name.name().to_string(), path.path)]))
            .unwrap_or_default(),

        BinField::Map(bins) => bins.into_iter()
            .map(|(name, path)| (name.name().to_string(), path.path))
            .collect(),
    }
}

impl ContentFlags {
    pub fn extract(locator: &Locator, package_data: &PackageData) -> Result<Self, Error> {
        if matches!(locator.reference, Reference::Link(_)) {
            return Ok(Self::default());
        }

        match package_data {
            PackageData::Local {package_directory, is_synthetic_package} if !is_synthetic_package => {
                Self::extract_local(package_directory)
            },

            PackageData::Zip {archive_path, ..} => {
                Self::extract_zip(locator, archive_path)
            },

            _ => {
                Ok(Self::default())
            },
        }
    }

    fn extract_local(package_directory: &Path) -> Result<Self, Error> {
        let manifest_path
            = package_directory.with_join_str("package.json");
        let manifest_bytes
            = manifest_path.fs_read_prealloc()?;
        let manifest
            = sonic_rs::from_slice::<Manifest>(&manifest_bytes)?;

        let build_commands
            = UNPLUG_SCRIPTS.iter()
                .filter_map(|k| manifest.scripts.get(*k).map(|s| (k, s)))
                .map(|(k, s)| build::Command::Script {event: Some(k.to_string()), script: s.to_string()})
                .collect::<Vec<_>>();

        Ok(ContentFlags {
            binaries: extract_binaries(manifest.name, manifest.bin),
            build_commands,
            prefer_extracted: Some(false),
            suggest_extracted: false,
        })
    }

    fn extract_zip(locator: &Locator, archive_path: &Path) -> Result<Self, Error> {
        let package_bytes
            = archive_path.fs_read()?;

        let first_entry = zpm_formats::zip::first_entry_from_zip(&package_bytes)
            .unwrap();

        let meta_manifest = sonic_rs::from_slice::<Manifest>(&first_entry.data)
            .unwrap();

        let mut build_commands = UNPLUG_SCRIPTS.iter()
            .filter_map(|k| meta_manifest.scripts.get(*k).map(|s| (k, s)))
            .map(|(k, s)| build::Command::Script {event: Some(k.to_string()), script: s.to_string()})
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

        let prefer_extracted
            = meta_manifest.prefer_unplugged;
        let suggest_extracted
            = entries.iter().any(|entry| UNPLUG_EXT_REGEX.is_match(&entry.name));

        Ok(ContentFlags {
            binaries: extract_binaries(meta_manifest.name, meta_manifest.bin),
            build_commands,
            prefer_extracted,
            suggest_extracted,
        })
    }
}
