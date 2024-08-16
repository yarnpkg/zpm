use std::{collections::{BTreeMap, BTreeSet, HashMap, HashSet}, sync::{Arc, LazyLock}};

use arca::{Path, ToArcaPath};
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::{build::{self, BuildRequests}, error::{self, Error}, fetcher::{PackageData, PackageLinking}, install::Install, misc::change_file, primitives::{locator::IdentOrLocator, Descriptor, Ident, Locator, Reference}, project::Project, resolver::Resolution, settings, yarn_serialization_protocol, zip::{entries_from_zip, first_entry_from_zip, Entry}};

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
    built: Option<bool>,

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

static UNPLUG_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

static UNPLUG_EXT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.(exe|bin|h|hh|hpp|c|cc|cpp|java|jar|node)$").unwrap()
});

fn check_build(locator: &Locator, package_meta: Option<&PackageMeta>, package_info: &PackageInfo, entries: &[Entry]) -> Vec<build::Command> {
    let built = package_meta
        .and_then(|meta| meta.built)
        .unwrap_or(true);

    if !built {
        return vec![];
    }

    let binding_gyp_name
        = format!("node_modules/{}/binding.gyp", locator.ident.as_str());

    let build_scripts = UNPLUG_SCRIPTS.iter()
        .filter_map(|k| package_info.scripts.get(*k))
        .map(|v| build::Command::Script(v.clone()))
        .collect::<Vec<_>>();

    if !build_scripts.is_empty() {
        return build_scripts;
    }

    if entries.iter().any(|entry| entry.name == binding_gyp_name) {
        return vec![build::Command::Program("node-gyp".to_string(), vec!["rebuild".to_string()])];
    }

    vec![]
}

fn check_extract(package_meta: Option<&PackageMeta>, package_info: &PackageInfo, build_commands: &[build::Command], entries: &[Entry]) -> bool {
    if let Some(meta) = package_meta {
        if let Some(unplugged) = meta.unplugged {
            return unplugged;
        }
    }

    if let Some(prefer_unplugged) = package_info.prefer_unplugged {
        return prefer_unplugged;
    }

    if !build_commands.is_empty() {
        return true;
    }

    if entries.iter().any(|entry| UNPLUG_EXT_REGEX.is_match(&entry.name)) {
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
            let first_entry = first_entry_from_zip(data)?;

            Ok(serde_json::from_slice::<PackageInfo>(&first_entry.data)?)
        },

        PackageData::Local {package_directory, ..} => {
            let manifest_text = package_directory
                .with_join_str("package.json")
                .fs_read_text()?;

            Ok(serde_json::from_str::<PackageInfo>(&manifest_text)?)
        },
    }
}

fn remove_nm(nm_path: Path) -> Result<(), Error> {
    let entries = nm_path.fs_read_dir();

    match entries {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
            => return Ok(()),

        Err(error)
            => return Err(error.into()),

        Ok(entries) => {
            let mut has_dot_entries = false;

            for entry in entries.flatten() {
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
        
            if !has_dot_entries {
                nm_path.fs_rm()?;
            }
        
            Ok(())
        },
    }
}

fn extract_archive(project_root: &Path, locator: &Locator, package_data: &PackageData, data: &[u8]) -> Result<Path, Error> {
    let extract_path = project_root
        .with_join_str(".yarn/unplugged")
        .with_join_str(locator.slug());

    let package_subpath = package_data.package_subpath();
    let package_directory = extract_path
        .with_join(&package_subpath);
    
    let ready_path = extract_path
        .with_join_str(".ready");

    if !ready_path.fs_exists() {
        for entry in crate::zip::entries_from_zip(data)? {
            let target_path = extract_path
                .with_join(&Path::from(&entry.name));

            target_path
                .fs_create_parent()?
                .fs_write(&entry.data)?;
        }

        ready_path
            .fs_write(&vec![])?;
    }

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
    fallback_pool: Vec<()>,

    #[serde_as(as = "Vec<(_, _)>")]
    fallback_exclusion_list: BTreeMap<Ident, BTreeSet<PnpReference>>,

    ignore_pattern_data: Option<Vec<String>>,

    #[serde_as(as = "Vec<(_, Vec<(_, _)>)>")]
    package_registry_data: BTreeMap<Option<Ident>, BTreeMap<Option<PnpReference>, PnpPackageInformation>>,
    dependency_tree_roots: Vec<PnpDependencyTreeRoot>,
}

fn generate_inline_files(project: &Project, state: &PnpState) -> Result<(), Error> {
    let script = vec![
        project.config.project.pnp_shebang.value.as_str(), "\n",
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

    change_file(project.pnp_path().to_path_buf(), script, 0o755)?;

    Ok(())
}

fn generate_split_setup(project: &Project, state: &PnpState) -> Result<(), Error> {
    let script = vec![
        project.config.project.pnp_shebang.value.as_str(), "\n",
        "/* eslint-disable */\n",
        "// @ts-nocheck\n",
        "\"use strict\";\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  const fs = require('fs');\n",
        "  const path = require('path');\n",
        "  const pnpDataFilepath = path.resolve(__dirname, '.pnp.data.json');\n",
        "  return hydrateRuntimeState(JSON.parse(fs.readFileSync(pnpDataFilepath, 'utf8')), {basePath: basePath || __dirname});\n",
        "}\n",
        std::include_str!("pnp.tpl.cjs"),
    ].join("");

    change_file(project.pnp_path().to_path_buf(), script, 0o755)?;
    change_file(project.pnp_data_path().to_path_buf(), serde_json::to_string(&state).unwrap(), 0o644)?;

    Ok(())
}

fn populate_build_entry_dependencies(package_build_entries: &HashMap<Locator, usize>, locator_resolutions: &HashMap<Locator, Resolution>, descriptor_to_locator: &HashMap<Descriptor, Locator>) -> Result<HashMap<usize, HashSet<usize>>, Error> {
    let mut package_build_dependencies = HashMap::new();

    for locator in package_build_entries.keys() {
        let mut build_dependencies = HashSet::new();

        let mut queue = vec![locator.clone()];
        let mut seen = HashSet::new();

        while let Some(locator) = queue.pop() {
            let resolution = locator_resolutions.get(&locator)
                .expect("Failed to find locator resolution");

            for dependency in resolution.dependencies.values() {
                let dependency_locator = descriptor_to_locator.get(dependency)
                    .expect("Failed to find dependency locator");

                if !seen.insert(locator.clone()) {
                    continue;
                }

                if dependency_locator == &locator {
                    return Err(Error::CircularBuildDependency(locator));
                }

                if let Some(dependency_entry_idx) = package_build_entries.get(dependency_locator) {
                    build_dependencies.insert(*dependency_entry_idx);
                }

                queue.push(dependency_locator.clone());
            }
        }

        let entry_idx = package_build_entries.get(locator)
            .expect("Failed to find build entry index");

        package_build_dependencies.insert(*entry_idx, build_dependencies);
    }

    Ok(package_build_dependencies)
}

pub async fn link_project<'a>(project: &'a mut Project, install: &'a mut Install) -> Result<BuildRequests, Error> {
    let tree = &install.install_state.resolution_tree;
    let nm_path = project.project_cwd.with_join_str("node_modules");

    remove_nm(nm_path)?;

    let dependencies_meta = project.manifest_path()
        .if_exists()
        .and_then(|path| path.fs_read_text().ok())
        .and_then(|data| serde_json::from_str::<TopLevelConfiguration>(&data).ok())
        .and_then(|config| config.dependencies_meta)
        .unwrap_or_default();

    let mut package_registry_data: BTreeMap<_, BTreeMap<_, _>> = BTreeMap::new();
    let mut dependency_tree_roots = Vec::new();

    let mut all_build_entries = Vec::new();
    let mut package_build_entries = HashMap::new();

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
            .with_join(&rel_path);

        let package_info = get_package_info(physical_package_data)?;
        let package_meta = dependencies_meta
            .get(&IdentOrLocator::Locator(locator.clone()))
            .or_else(|| dependencies_meta.get(&IdentOrLocator::Ident(locator.ident.clone())));

        let relevant_build_entries = match physical_package_data {
            PackageData::Zip {data, ..} => entries_from_zip(data)?,
            PackageData::Local {..} => vec![],
        };

        let build_commands
            = check_build(locator, package_meta, &package_info, &relevant_build_entries);

        if let PackageData::Zip {data, ..} = physical_package_data {
            if check_extract(package_meta, &package_info, &build_commands, &relevant_build_entries) {
                package_location_abs = extract_archive(&project.project_cwd, locator, physical_package_data, data)?;
            }
        }

        let package_location_rel = package_location_abs
            .relative_to(&project.project_cwd);

        install.install_state.packages_by_location.insert(package_location_rel.clone(), locator.clone());
        install.install_state.locations_by_package.insert(locator.clone(), package_location_rel.clone());

        let mut package_location = package_location_rel
            .to_string();

        if package_location.is_empty() {
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

        if !build_commands.is_empty() {
            package_build_entries.insert(locator.clone(), all_build_entries.len());
            all_build_entries.push(build::BuildRequest {
                cwd: package_location_rel,
                locator: locator.clone(),
                commands: build_commands,
                allowed_to_fail: install.install_state.resolution_tree.optional_builds.contains(locator),
            });
        }
    }

    for workspace in project.workspaces.values().sorted_by_cached_key(|w| w.descriptor()) {
        let locator = workspace.locator();

        if workspace.path == project.project_cwd {
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

    let enable_top_level_fallback = project.config.project.pnp_fallback_mode.value != settings::PnpFallbackMode::None;

    let mut fallback_exclusion_list: BTreeMap<Ident, BTreeSet<PnpReference>> = BTreeMap::new();
    let fallback_pool = vec![];

    if project.config.project.pnp_fallback_mode.value == settings::PnpFallbackMode::DependenciesOnly {
        for locator in tree.locator_resolutions.keys() {
            if let Reference::Workspace(_) = locator.physical_locator().reference {
                fallback_exclusion_list.entry(locator.ident.clone())
                    .or_default()
                    .insert(PnpReference(locator.clone()));
            }
        }
    }

    let ignore_pattern_data = project.config.project.pnp_ignore_patterns.value
        .iter()
        .map(|pattern| pattern.value.to_regex_string())
        .collect::<Vec<String>>();

    let state = PnpState {
        enable_top_level_fallback,
        fallback_exclusion_list,
        fallback_pool,

        ignore_pattern_data: match ignore_pattern_data.is_empty() {
            true => None,
            false => Some(ignore_pattern_data),
        },

        package_registry_data,
        dependency_tree_roots,
    };

    if project.config.project.pnp_enable_inlining.value {
        generate_inline_files(project, &state)?;
    } else {
        generate_split_setup(project, &state)?;
    }

    change_file(project.pnp_loader_path().to_path_buf(), std::include_str!("pnp.loader.mjs"), 0o644)?;

    let package_build_dependencies = populate_build_entry_dependencies(
        &package_build_entries,
        &tree.locator_resolutions,
        &tree.descriptor_to_locator,
    );

    Ok(build::BuildRequests {
        entries: all_build_entries,
        dependencies: package_build_dependencies?,
    })
}

