use std::{collections::{BTreeMap, HashMap}, io::Read, sync::Arc};

use arca::Path;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use zip::ZipArchive;

use crate::{error::Error, fetcher::{PackageData, PackageLinking}, primitives::{Ident, Locator, Reference}, project};

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct PnpLinkerData {
    #[serde(default, skip_serializing_if = "is_default")]
    enable_build: bool,

    #[serde(default, skip_serializing_if = "is_default")]
    enable_esm: bool,

    #[serde(default, skip_serializing_if = "is_default")]
    enable_extract: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "linkerType")]
pub enum LinkerData {
    Pnp(PnpLinkerData),
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Serialize)]
struct PackageInfo {
    #[serde(default)]
    r#type: Option<String>,

    #[serde(default, rename = "preferUnplugged")]
    prefer_unplugged: Option<bool>,

    #[serde(default)]
    scripts: HashMap<String, String>,
}

const UNPLUG_SCRIPTS: &'static [&'static str] = &["preinstall", "install", "postinstall"];

static UNPLUG_EXT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\.(exe|bin|h|hh|hpp|c|cc|cpp|java|jar|node)$").unwrap()
});

fn check_build(package_info: &PackageInfo) -> bool {
    UNPLUG_SCRIPTS.iter().any(|k| package_info.scripts.contains_key(*k))
}

fn check_extract<T: std::io::Read + std::io::Seek>(locator: &Locator, package_info: &PackageInfo, zip: &mut ZipArchive<T>) -> bool {
    if let Some(prefer_unplugged) = package_info.prefer_unplugged {
        return prefer_unplugged;
    }

    if check_build(package_info) {
        return true;
    }

    let binding_gyp_name = format!("node_modules/{}/binding.gyp", locator.ident.as_str());
    let has_binding_gyp = zip.by_name(&binding_gyp_name).is_ok();

    if has_binding_gyp {
        return true;
    }

    let has_unpluggable_files = zip.file_names()
        .any(|name| UNPLUG_EXT_REGEX.is_match(name));

    if has_unpluggable_files {
        return true;
    }

    false
}

fn get_linker_data(locator: &Locator, data: &PackageData) -> LinkerData {
    match data {
        _ => {
            LinkerData::Pnp(PnpLinkerData {
                enable_build: false,
                enable_esm: false,
                enable_extract: false,
            })
        }

        PackageData::Zip(_, data, _) => {
            let reader = std::io::Cursor::new(data);
            let mut zip = zip::read::ZipArchive::new(reader)
                .unwrap();

            let package_json_name = format!("node_modules/{}/package.json", locator.ident.as_str());
            let mut package_json = zip.by_name(&package_json_name)
                .expect("Failed to find package.json");

            let mut package_json_data = String::new();
            package_json.read_to_string(&mut package_json_data).unwrap();
            let package_info = serde_json::from_str(&package_json_data).unwrap();

            drop(package_json);

            let enable_build = check_build(&package_info);
            let enable_esm = package_info.r#type.as_ref().is_some_and(|v| v == "module");
            let enable_extract = check_extract(locator, &package_info, &mut zip);

            LinkerData::Pnp(PnpLinkerData {
                enable_build,
                enable_esm,
                enable_extract,
            })
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum PnpDependencyTarget {
    Simple(Reference),
    Alias((Ident, Reference)),
    Missing,
}

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpPackageInformation {
    package_location: String,
    #[serde_as(as = "Vec<(_, _)>")]
    package_dependencies: BTreeMap<Ident, PnpDependencyTarget>,
    package_peers: Vec<Ident>,
    link_type: PackageLinking,
    discard_from_lookup: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PnpDependencyTreeRoot {
    name: Ident,
    reference: Reference,
}

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpState {
    enable_top_level_fallback: bool,
    fallback_exclusion_list: Vec<()>,
    fallback_pool: Vec<()>,

    ignore_pattern_data: Option<String>,

    #[serde_as(as = "Vec<(_, Vec<(_, _)>)>")]
    package_registry_data: BTreeMap<Ident, BTreeMap<Reference, PnpPackageInformation>>,
    dependency_tree_roots: Vec<PnpDependencyTreeRoot>,
}

pub async fn link_project() -> Result<(), Error> {
    let project_root = project::root()?;
    let pnp_path = project_root.with_join_str(".pnp.cjs");

    let cache
        = project::cache().await?;

    let linker_data = cache.iter().map(|(locator, data)| {
        (locator.clone(), get_linker_data(&locator, data))
    }).collect::<HashMap<_, _>>();

    let tree = project::tree().await?;

    let mut package_registry_data: BTreeMap<Ident, BTreeMap<Reference, PnpPackageInformation>> = BTreeMap::new();
    let mut dependency_tree_roots = Vec::new();

    for (locator, resolution) in tree.locator_resolutions {
        let physical_package_data = cache.get(&locator.physical_locator())
            .expect("Failed to find physical package data");

        let package_dependencies = resolution.dependencies.iter().map(|(ident, descriptor)| {
            let dependency_resolution = tree.descriptor_to_locator.get(descriptor)
                .expect("Failed to find dependency resolution");

            let dependency_target = if &dependency_resolution.ident == ident {
                PnpDependencyTarget::Simple(dependency_resolution.reference.clone())
            } else {
                PnpDependencyTarget::Alias((dependency_resolution.ident.clone(), dependency_resolution.reference.clone()))
            };

            (ident.clone(), dependency_target)
        }).collect();

        let package_peers = resolution.peer_dependencies.keys()
            .cloned()
            .sorted()
            .collect::<Vec<_>>();

        let rel_source_path = project_root.relative_to(&physical_package_data.source_path(&locator));

        let package_location = if rel_source_path != Path::from(".") {
            format!("./{}/", rel_source_path.to_string())
        } else {
            "./".to_string()
        };

        package_registry_data.entry(locator.ident)
            .or_default()
            .insert(locator.reference, PnpPackageInformation {
                package_location,
                package_dependencies,
                package_peers,
                link_type: physical_package_data.link_type(),
                discard_from_lookup: false,
            });
    }

    for workspace in project::workspaces()?.values() {
        let locator = workspace.locator();

        dependency_tree_roots.push(PnpDependencyTreeRoot {
            name: locator.ident,
            reference: locator.reference,
        });
    }

    let state = PnpState {
        enable_top_level_fallback: false,
        fallback_exclusion_list: Vec::new(),
        fallback_pool: Vec::new(),

        ignore_pattern_data: None,

        package_registry_data,
        dependency_tree_roots,
    };


    // `const RAW_RUNTIME_STATE =\n`,
    // `${generateStringLiteral(generatePrettyJson(data))};\n\n`,
    // `function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n`,
    // `  return hydrateRuntimeState(JSON.parse(RAW_RUNTIME_STATE), {basePath: basePath || __dirname});\n`,
    // `}\n`,

    let script = vec![
        "/* eslint-disable */\n",
        "// @ts-nocheck\n",
        "\"use strict\";\n",
        "\n",
        "const RAW_RUNTIME_STATE =\n",
        &serde_json::to_string(&serde_json::to_string(&state).unwrap()).unwrap(), ";\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  return hydrateRuntimeState(JSON.parse(RAW_RUNTIME_STATE), {basePath: basePath || __dirname});\n",
        "}\n",
        std::include_str!("pnp.tpl.cjs"),
    ].join("");

    std::fs::write(pnp_path.to_path_buf(), script)
        .map_err(Arc::new)?;

    Ok(())
}

