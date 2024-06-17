use std::{collections::{BTreeMap, HashMap}, sync::Arc};

use arca::{Path, ToArcaPath};
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use zpm_macros::track_time;

use crate::{error::Error, fetcher::{PackageData, PackageLinking}, install::Install, misc::change_file, primitives::{locator::IdentOrLocator, Ident, Locator, Reference}, project::Project, yarn_serialization_protocol, zip::{entries_from_zip, first_entry_from_zip, Entry}};

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageMeta {
    #[serde(default, skip_serializing_if = "is_default")]
    unplugged: Option<bool>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TopLevelConfiguration {
    #[serde(default)]
    dependencies_meta: Option<HashMap<IdentOrLocator, PackageMeta>>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageInfo {
    #[serde(default)]
    r#type: Option<String>,

    #[serde(default,)]
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

fn check_extract(locator: &Locator, package_meta: Option<&PackageMeta>, package_info: &PackageInfo, entries: &Vec<Entry>) -> bool {
    if let Some(meta) = package_meta {
        if let Some(unplugged) = meta.unplugged {
            return unplugged;
        }
    }

    if let Some(prefer_unplugged) = package_info.prefer_unplugged {
        return prefer_unplugged;
    }

    if check_build(package_info) {
        return true;
    }

    let binding_gyp_name = format!("node_modules/{}/binding.gyp", locator.ident.as_str());
    let has_unpluggable_files = entries.iter().any(|entry| entry.name == binding_gyp_name || UNPLUG_EXT_REGEX.is_match(&entry.name));

    if has_unpluggable_files {
        return true;
    }

    false
}

// fn get_pnp_linker_data(locator: &Locator, data: &PackageData) -> Result<PnpLinkerData, Error> {
//     match data {
//         PackageData::Zip {data, ..} => {
//             let first_entry = first_entry_from_zip(&data);
//             let manifest = first_entry
//                 .and_then(|entry|
//                     serde_json::from_slice::<PackageInfo>(&entry.data)
//                         .map_err(Arc::new)
//                         .map_err(Error::InvalidJsonData)
//                 )?;
        
//             let enable_build = check_build(&manifest);
//             let enable_esm = manifest.r#type.as_ref().is_some_and(|v| v == "module");
//             let enable_extract = check_extract(locator, &manifest);

//             Ok(PnpLinkerData {
//                 enable_build,
//                 enable_esm,
//                 enable_extract,
//             })
//         }

//         _ => {
//             Ok(PnpLinkerData {
//                 enable_build: false,
//                 enable_esm: false,
//                 enable_extract: false,
//             })
//         }
//     }
// }

fn get_package_info(package_data: &PackageData) -> Result<PackageInfo, Error> {
    match package_data {
        PackageData::Zip {data, ..} => {
            let first_entry = first_entry_from_zip(&data)?;

            serde_json::from_slice::<PackageInfo>(&first_entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
       }

        _ => {
            Ok(PackageInfo::default())
        },
    }
}

fn remove_nm(nm_path: Path) {
    if let Ok(entries) = nm_path.fs_read_dir() {
        let mut has_dot_entries = false;

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path()
                    .to_arca();

                let basename = path.basename()
                    .unwrap();

                if basename.starts_with(".") && basename != ".bin" && path.fs_is_dir() {
                    has_dot_entries = true;
                    continue;
                }

                path.fs_rm()
                    .unwrap();
            }
        }

        if !has_dot_entries {
            nm_path.fs_rm()
                .unwrap();
        }
    }
}

fn extract_archive(project_root: &Path, locator: &Locator, package_data: &PackageData, data: &[u8]) -> Result<Path, Error> {
    let extract_path = project_root
        .with_join_str(".yarn/unplugged")
        .with_join_str(locator.slug());

    let ready_path = extract_path
        .with_join_str(".ready");

    if ready_path.fs_exists() {
        return Ok(extract_path);
    }

    for entry in crate::zip::entries_from_zip(data)? {
        let target_path = extract_path
            .with_join(&Path::from(&entry.name));

        std::fs::create_dir_all(target_path.dirname().unwrap().to_path_buf())
            .map_err(Arc::new)?;

        std::fs::write(target_path.to_path_buf(), entry.data)
            .map_err(Arc::new)?;
    }

    ready_path.fs_write(&vec![])
        .map_err(Arc::new)?;

    let package_subpath = package_data.package_subpath();
    let package_directory = extract_path
        .with_join(&package_subpath);

    Ok(package_directory)
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PnpReference(Locator);

yarn_serialization_protocol!(PnpReference, "", {
    serialize(&self) {
        match &self.0.parent {
            Some(parent) => format!("{}::parent={}", self.0.reference, parent),
            None => self.0.reference.to_string(),
        }
    }
});

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum PnpDependencyTarget {
    Simple(PnpReference),
    Alias((Ident, PnpReference)),
    Missing(Option<()>),
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
    package_registry_data: BTreeMap<Option<Ident>, BTreeMap<Option<PnpReference>, PnpPackageInformation>>,
    dependency_tree_roots: Vec<PnpDependencyTreeRoot>,
}

#[track_time]
pub async fn link_project<'a>(project: &'a Project, install: &'a Install) -> Result<(), Error> {
    let tree = &install.resolution_tree;
    let nm_path = project.root.with_join_str("node_modules");

    remove_nm(nm_path);

    let dependencies_meta = project.manifest_path()
        .if_exists()
        .and_then(|path| std::fs::read_to_string(path.to_path_buf()).ok())
        .and_then(|data| serde_json::from_str::<TopLevelConfiguration>(&data).ok())
        .and_then(|config| config.dependencies_meta)
        .unwrap_or_default();

    let mut package_registry_data: BTreeMap<_, BTreeMap<_, _>> = BTreeMap::new();
    let mut dependency_tree_roots = Vec::new();

    for (locator, resolution) in &tree.locator_resolutions {
        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator()));

        let mut package_dependencies: BTreeMap<Ident, PnpDependencyTarget> = resolution.dependencies.iter().map(|(ident, descriptor)| {
            let dependency_resolution = tree.descriptor_to_locator.get(descriptor)
                .expect("Failed to find dependency resolution");

            let dependency_target = if &dependency_resolution.ident == ident {
                PnpDependencyTarget::Simple(PnpReference(dependency_resolution.clone()))
            } else {
                PnpDependencyTarget::Alias((dependency_resolution.ident.clone(), PnpReference(dependency_resolution.clone())))
            };

            (ident.clone(), dependency_target)
        }).collect();

        for peer in &resolution.missing_peer_dependencies {
            package_dependencies.entry(peer.clone())
                .or_insert(PnpDependencyTarget::Missing(None));
        }

        package_dependencies.entry(locator.ident.clone())
            .or_insert(PnpDependencyTarget::Simple(PnpReference(locator.clone())));

        let mut package_peers = resolution.peer_dependencies.keys()
            .cloned()
            .collect::<Vec<_>>();

        package_peers.sort();

        let virtual_dir = Path::from(match &locator.reference {
            Reference::Virtual(_, hash) => format!("__virtual__/{}/0/", hash),
            _ => "".to_string(),
        });

        let rel_path = physical_package_data.package_directory()
            .relative_to(physical_package_data.data_root());

        let mut package_location_abs = physical_package_data.data_root()
            .with_join(&virtual_dir)
            .with_join(&rel_path)
            .clone();

        let package_info = get_package_info(&physical_package_data)?;
        let package_meta = dependencies_meta
            .get(&IdentOrLocator::Locator(locator.clone()))
            .or_else(|| dependencies_meta.get(&IdentOrLocator::Ident(locator.ident.clone())));

        if let PackageData::Zip {data, ..} = physical_package_data {
            let entries = entries_from_zip(data)?;

            if check_extract(&locator, package_meta, &package_info, &entries) {
                package_location_abs = extract_archive(&project.root, locator, physical_package_data, data)?;
            }
        }

        let mut package_location = package_location_abs
            .relative_to(&project.root)
            .to_string();

        if package_location.len() == 0 {
            package_location = "./".to_string();
        }

        if !package_location.ends_with('/') {
            package_location.push('/');
        }

        if !package_location.starts_with("./") && !package_location.starts_with("../") {
            package_location.insert_str(0, "./");
        }

        let discard_from_lookup = match physical_package_data {
            PackageData::Local {discard_from_lookup, ..} => *discard_from_lookup,
            _ => false,
        };

        package_registry_data.entry(Some(locator.ident.clone()))
            .or_default()
            .insert(Some(PnpReference(locator.clone())), PnpPackageInformation {
                package_location,
                package_dependencies,
                package_peers,
                link_type: physical_package_data.link_type(),
                discard_from_lookup,
            });
    }

    for workspace in project.workspaces.values().sorted_by_cached_key(|w| w.descriptor()) {
        let locator = workspace.locator();

        if workspace.path == project.root {
            let entry = package_registry_data
                .get(&Some(locator.ident.clone()))
                .expect("Failed to find workspace entry")
                .get(&Some(PnpReference(locator.clone())))
                .expect("Failed to find workspace entry")
                .clone();

            package_registry_data
                .entry(None)
                .or_default()
                .entry(None)
                .or_insert(entry);
        }

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

    let script = vec![
        "#!/usr/bin/env node\n",
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

    change_file(project.pnp_path().to_path_buf(), script, 0o755)
        .map_err(Arc::new)?;

    change_file(project.pnp_loader_path().to_path_buf(), std::include_str!("pnp.loader.mjs"), 0o644)
        .map_err(Arc::new)?;

    Ok(())
}

