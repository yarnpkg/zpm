use std::{collections::BTreeMap, str::FromStr, sync::LazyLock};

use ini::Ini;
use rkyv::Archive;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::Path;

use crate::{
    build, error::Error, fetchers::PackageData, manifest::bin::BinField
};

static UNPLUG_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

static UNPLUG_EXT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.(exe|bin|h|hh|hpp|c|cc|cpp|java|jar|node)$").unwrap()
});

static PYPI_ENTRY_POINT_VALUE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?P<module>[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)(?:\s*:\s*(?P<object>[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*))?\s*(?P<extras>\[[^\[\]]*\])?\s*$").unwrap()
});

/**
 * The package metadata struct contains various fields that instruct the
 * package manager (the linker, mostly) about the content of the package.
 *
 * We compute this struct the first time the package is fetched and store it
 * inside the install state so we can avoid having to recompute it every time,
 * which would otherwise require to parse the zip archives every time.
 */
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ContentFlags {
    /**
     * The binaries that should be made available to the package.
     */
    #[serde(default)]
    pub binaries: BTreeMap<String, Binary>,

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Binary {
    Node(Path),
    Python {
        module: String,
        object: String,
    },
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
    prefer_unplugged: Option<bool>,

    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError")]
    gypfile: Option<bool>,

    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError")]
    scripts: BTreeMap<String, String>,
}

/**
 * Like npm, packages with a `binding.gyp` file at their root but no install
 * scripts are built by running `node-gyp rebuild`, unless they opt out by
 * setting `gypfile` to `false` (typically because they ship prebuilt
 * binaries).
 */
fn has_implicit_node_gyp_build(manifest: &Manifest, has_binding_gyp: bool) -> bool {
    has_binding_gyp
        && manifest.gypfile != Some(false)
        && !UNPLUG_SCRIPTS.iter().any(|k| manifest.scripts.contains_key(*k))
}

fn extract_binaries(name: Option<Ident>, bin: Option<BinField>) -> BTreeMap<String, Binary> {
    let Some(bin) = bin else {
        return BTreeMap::new();
    };

    match bin {
        BinField::String(path) => name
            .map(|name| BTreeMap::from_iter([(name.name().to_string(), Binary::Node(path.path))]))
            .unwrap_or_default(),

        BinField::Map(bins) => bins.into_iter()
            .map(|(name, path)| (name.name().to_string(), Binary::Node(path.path)))
            .collect(),
    }
}

fn is_valid_pypi_binary_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\')
}

fn parse_pypi_entry_point_value(value: &str) -> Option<(String, String)> {
    let Some(captures)
        = PYPI_ENTRY_POINT_VALUE_REGEX.captures(value) else {
        return None;
    };

    let module_name
        = captures.name("module")
            .map(|capture| capture.as_str())
            .unwrap_or_default();

    let object_name
        = captures.name("object")
            .map(|capture| capture.as_str());

    let Some(object_name) = object_name else {
        return None;
    };

    let extras
        = captures.name("extras")
            .map(|capture| capture.as_str())
            .unwrap_or_default();

    let pep_508_input
        = format!("{}{}", module_name, extras);

    let Ok(parsed)
        = pep_508::parse(&pep_508_input) else {
        return None;
    };

    if parsed.name != module_name || parsed.spec.is_some() || parsed.marker.is_some() {
        return None;
    }

    Some((module_name.to_string(), object_name.to_string()))
}

fn extract_pypi_binaries(package_bytes: &[u8]) -> Result<BTreeMap<String, Binary>, Error> {
    let entries
        = zpm_formats::zip::entries_from_zip(package_bytes)?;

    let entry_points_data
        = entries.into_iter()
            .find(|entry| entry.name.as_str().ends_with(".dist-info/entry_points.txt"))
            .map(|entry| entry.data);

    let Some(entry_points_data) = entry_points_data else {
        return Ok(BTreeMap::new());
    };

    let entry_points_text
        = match String::from_utf8(entry_points_data.to_vec()) {
            Ok(value) => value,
            Err(_) => return Ok(BTreeMap::new()),
        };

    let entry_points
        = match Ini::load_from_str(&entry_points_text) {
            Ok(entry_points) => entry_points,
            Err(_) => return Ok(BTreeMap::new()),
        };

    let Some(console_scripts)
        = entry_points.section(Some("console_scripts")) else {
        return Ok(BTreeMap::new());
    };

    let mut binaries
        = BTreeMap::new();

    for (binary_name, target) in console_scripts.iter() {
        if !is_valid_pypi_binary_name(binary_name) {
            continue;
        }

        let Some((module, object))
            = parse_pypi_entry_point_value(target) else {
            continue;
        };

        binaries.insert(binary_name.to_string(), Binary::Python {
            module,
            object,
        });
    }

    Ok(binaries)
}

impl ContentFlags {
    /**
     * Content flags are cached in the install state and reused by later
     * installs. Bump this version whenever the way they're computed changes,
     * so that the flags computed by older releases get recomputed.
     */
    pub const VERSION: u32 = 1;

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
        let manifest: Manifest
            = JsonDocument::hydrate_from_slice(&manifest_bytes)?;

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

        if matches!(locator.reference, Reference::PypiShorthand(_) | Reference::PypiRegistry(_)) {
            // Source distributions (tarballs, or zips cached as `.src.zip`)
            // only know their console scripts once built, which the venv
            // linker takes care of
            let is_wheel
                = package_bytes.starts_with(b"PK") && !archive_path.as_str().ends_with(".src.zip");

            let binaries = match is_wheel {
                true => extract_pypi_binaries(&package_bytes)?,
                false => BTreeMap::new(),
            };

            return Ok(Self {
                binaries,
                build_commands: vec![],
                prefer_extracted: None,
                suggest_extracted: false,
            });
        }

        let first_entry
            = zpm_formats::zip::first_entry_from_zip(&package_bytes)?;

        let meta_manifest: Manifest
            = JsonDocument::hydrate_from_slice(&first_entry.data)?;

        let mut build_commands = UNPLUG_SCRIPTS.iter()
            .filter_map(|k| meta_manifest.scripts.get(*k).map(|s| (k, s)))
            .map(|(k, s)| build::Command::Script {event: Some(k.to_string()), script: s.to_string()})
            .collect::<Vec<_>>();

        let entries
            = zpm_formats::zip::entries_from_zip(&package_bytes)?;

        let binding_gyp_name
            = Path::from_str(&format!("node_modules/{}/binding.gyp", locator.ident.as_str()))?;
        let has_binding_gyp
            = entries.iter().any(|entry| entry.name == binding_gyp_name);

        if has_implicit_node_gyp_build(&meta_manifest, has_binding_gyp) {
            build_commands.push(build::Command::Program {
                name: "node-gyp".to_string(),
                args: vec!["rebuild".to_string()],
            });
        }

        let prefer_extracted
            = meta_manifest.prefer_unplugged;
        let suggest_extracted
            = entries.iter().any(|entry| UNPLUG_EXT_REGEX.is_match(&entry.name.as_str()));

        Ok(ContentFlags {
            binaries: extract_binaries(meta_manifest.name, meta_manifest.bin),
            build_commands,
            prefer_extracted,
            suggest_extracted,
        })
    }
}

#[cfg(test)]
mod tests {
    use zpm_parsers::JsonDocument;

    use crate::content_flags::{
        Manifest,
        has_implicit_node_gyp_build,
        parse_pypi_entry_point_value,
    };

    fn manifest(json: &str) -> Manifest {
        JsonDocument::hydrate_from_slice(json.as_bytes()).unwrap()
    }

    #[test]
    fn it_builds_packages_with_a_binding_gyp_using_node_gyp() {
        assert!(has_implicit_node_gyp_build(&manifest(r#"{}"#), true));
        assert!(has_implicit_node_gyp_build(&manifest(r#"{"gypfile": true}"#), true));
        assert!(has_implicit_node_gyp_build(&manifest(r#"{"scripts": {"build": "tsc"}}"#), true));
    }

    #[test]
    fn it_doesnt_build_packages_without_a_binding_gyp_using_node_gyp() {
        assert!(!has_implicit_node_gyp_build(&manifest(r#"{}"#), false));
        assert!(!has_implicit_node_gyp_build(&manifest(r#"{"gypfile": true}"#), false));
    }

    #[test]
    fn it_doesnt_build_packages_with_install_scripts_using_node_gyp() {
        assert!(!has_implicit_node_gyp_build(&manifest(r#"{"scripts": {"preinstall": "true"}}"#), true));
        assert!(!has_implicit_node_gyp_build(&manifest(r#"{"scripts": {"install": "true"}}"#), true));
        assert!(!has_implicit_node_gyp_build(&manifest(r#"{"scripts": {"postinstall": "true"}}"#), true));
    }

    #[test]
    fn it_doesnt_build_packages_with_gypfile_false_using_node_gyp() {
        assert!(!has_implicit_node_gyp_build(&manifest(r#"{"gypfile": false}"#), true));
    }

    #[test]
    fn it_only_lets_the_false_boolean_disable_the_node_gyp_build() {
        assert!(has_implicit_node_gyp_build(&manifest(r#"{"gypfile": "false"}"#), true));
        assert!(has_implicit_node_gyp_build(&manifest(r#"{"gypfile": 0}"#), true));
        assert!(has_implicit_node_gyp_build(&manifest(r#"{"gypfile": null}"#), true));
    }

    #[test]
    fn it_accepts_valid_entry_point_values() {
        assert_eq!(
            parse_pypi_entry_point_value("mypackage.tools:main"),
            Some(("mypackage.tools".to_string(), "main".to_string())),
        );

        assert_eq!(
            parse_pypi_entry_point_value("mypackage.tools:cli.main [foo, bar]"),
            Some(("mypackage.tools".to_string(), "cli.main".to_string())),
        );
    }

    #[test]
    fn it_rejects_invalid_entry_point_values() {
        assert_eq!(parse_pypi_entry_point_value("my-package.tools:main"), None);
        assert_eq!(parse_pypi_entry_point_value("mypackage.tools:"), None);
        assert_eq!(parse_pypi_entry_point_value("mypackage.tools"), None);
        assert_eq!(parse_pypi_entry_point_value("mypackage.tools:main [foo?]"), None);
        assert_eq!(parse_pypi_entry_point_value("mypackage.tools:main ; python_version > '3.8'"), None);
    }
}
