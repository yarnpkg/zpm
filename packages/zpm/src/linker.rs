use std::{collections::{BTreeMap, BTreeSet}, fs::Permissions, os::unix::fs::PermissionsExt, vec};

use arca::{Path, ToArcaPath};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::{build::{self, BuildRequests}, error::Error, fetchers::{PackageData, PackageLinking}, formats, install::Install, primitives::{range::PackageSelector, Descriptor, Ident, Locator, Reference}, project::Project, resolvers::Resolution, settings, yarn_serialization_protocol};

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageMeta {
    #[serde(default, skip_serializing_if = "is_default")]
    built: Option<bool>,

    #[serde(default, skip_serializing_if = "is_default")]
    unplugged: Option<bool>,
}

#[serde_as]
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TopLevelConfiguration {
    #[serde(default)]
    #[serde_as(as = "BTreeMap<_, _>")]
    dependencies_meta: Vec<(PackageSelector, PackageMeta)>,
}

fn remove_nm(nm_path: Path) -> Result<(), Error> {
    let entries = nm_path.fs_read_dir();

    match entries {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
            => Ok(()),

        Err(error)
            => Err(error.into()),

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

fn extract_archive(project_root: &Path, locator: &Locator, package_data: &PackageData) -> Result<(Path, bool), Error> {
    let extract_path = project_root
        .with_join_str(".yarn/unplugged")
        .with_join_str(locator.slug());

    let package_subpath = package_data.package_subpath();
    let package_directory = extract_path
        .with_join(&package_subpath);
    
    let ready_path = extract_path
        .with_join_str(".ready");

    if !ready_path.fs_exists() && !matches!(package_data, &PackageData::MissingZip {..}) {
        let package_bytes = match package_data {
            PackageData::Zip {archive_path, ..} => archive_path.fs_read()?,
            _ => panic!("Expected a zip archive"),
        };

        for entry in formats::zip::entries_from_zip(&package_bytes)? {
            let target_path = extract_path
                .with_join(&Path::from(&entry.name));

            target_path
                .fs_create_parent()?
                .fs_write(&entry.data)?
                .fs_set_permissions(Permissions::from_mode(entry.mode as u32))?;
        }

        ready_path
            .fs_write(vec![])?;

        Ok((package_directory, true))
    } else {
        Ok((package_directory, false))
    }
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

    project.pnp_path()
        .fs_change(script, Permissions::from_mode(0o755))?;

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

    project.pnp_path()
        .fs_change(script, Permissions::from_mode(0o755))?;

    project.pnp_data_path()
        .fs_change(serde_json::to_string(&state).unwrap(), Permissions::from_mode(0o644))?;

    Ok(())
}

fn populate_build_entry_dependencies(package_build_entries: &BTreeMap<Locator, usize>, locator_resolutions: &BTreeMap<Locator, Resolution>, descriptor_to_locator: &BTreeMap<Descriptor, Locator>) -> Result<BTreeMap<usize, BTreeSet<usize>>, Error> {
    let mut package_build_dependencies = BTreeMap::new();

    for locator in package_build_entries.keys() {
        let mut build_dependencies = BTreeSet::new();

        let mut queue = vec![locator.clone()];
        let mut seen: BTreeSet<_> = BTreeSet::new();

        seen.insert(locator.clone());

        while let Some(locator) = queue.pop() {
            let resolution = locator_resolutions.get(&locator)
                .expect("Failed to find locator resolution");

            for dependency in resolution.dependencies.values() {
                let dependency_locator = descriptor_to_locator.get(dependency)
                    .expect("Failed to find dependency locator");

                if !seen.insert(dependency_locator.clone()) {
                    continue;
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
        .and_then(|path| path.fs_read_text().ok()).map(|data| serde_json::from_str::<TopLevelConfiguration>(&data).unwrap().dependencies_meta)
        .unwrap_or_default()
        .into_iter()
        .map(|(selector, meta)| (selector.ident().clone(), (selector, meta)))
        .into_group_map();

    let mut package_registry_data: BTreeMap<_, BTreeMap<_, _>> = BTreeMap::new();
    let mut dependency_tree_roots = Vec::new();

    let mut all_build_entries = Vec::new();
    let mut package_build_entries = BTreeMap::new();

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
            Reference::Virtual(params) => format!("__virtual__/{}/0/", params.hash),
            _ => "".to_string(),
        });

        let rel_path = physical_package_data.package_directory()
            .relative_to(physical_package_data.data_root());

        let mut package_location_abs = physical_package_data.data_root()
            .with_join(&virtual_dir)
            .with_join(&rel_path);

        let discard_from_lookup = match physical_package_data {
            PackageData::Local {discard_from_lookup, ..} => *discard_from_lookup,
            _ => false,
        };

        // The package meta is based on the top-level configuration extracted
        // from the `dependenciesMeta` field.
        //
        let mut package_meta = dependencies_meta
            .get(&locator.ident)
            .and_then(|meta_list| {
                meta_list.iter().find_map(|(selector, meta)| match selector {
                    PackageSelector::Range(params) => params.range.check(&resolution.version).then_some(meta),
                    PackageSelector::Ident(_) => Some(meta),
                })
            })
            .cloned()
            .unwrap_or_default();

        // The package flags are based on the actual package content. The flags
        // should always be the same for the same package, so we keep them in
        // the install state so we don't have to recompute them at every install.
        //
        let package_flags = install.install_state.package_flags
            .get(&locator.physical_locator())
            .expect("Expected package flags to be set");

        // Optional dependencies are always unplugged, as we have no way to
        // know whether they would be unplugged if we were to download them
        // (this may change depending on the package's files).
        //
        if install.install_state.resolution_tree.optional_builds.contains(locator) {
            package_meta.unplugged = Some(true);
        }

        // If the builds are disabled by default, let's reflect that in the
        // package's meta
        if !project.config.project.enable_scripts.value && package_meta.built.is_none() {
            package_meta.built = Some(false);
        }

        // We don't need to run the build if the package was marked as
        // incompatible with the current system (even if the package isn't
        // marked as optional).
        //
        if !package_flags.is_compatible {
            package_meta.built = Some(false);
        }

        let mut is_physically_on_disk = true;
        let mut is_freshly_unplugged = false;

        if package_flags.prefer_extracted.unwrap_or_else(|| package_meta.built == Some(true) || package_flags.suggest_extracted) {
            (package_location_abs, is_freshly_unplugged) = extract_archive(&project.project_cwd, locator, physical_package_data)?;
        } else {
            is_physically_on_disk = false;
        }

        let package_location_rel = package_location_abs
            .relative_to(&project.project_cwd);

        if !matches!(physical_package_data, PackageData::MissingZip {..}) {
            install.install_state.packages_by_location.insert(package_location_rel.clone(), locator.clone());
            install.install_state.locations_by_package.insert(locator.clone(), package_location_rel.clone());
        }

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

        package_registry_data.entry(Some(locator.ident.clone()))
            .or_default()
            .insert(Some(PnpReference(locator.clone())), PnpPackageInformation {
                package_location,
                package_dependencies,
                package_peers,
                link_type: physical_package_data.link_type(),
                discard_from_lookup,
            });

        if package_flags.build_commands.len() > 0 && package_meta.built.unwrap_or(true) {
            let build_cwd = match is_physically_on_disk {
                true => package_location_rel.clone(),
                false => {
                    let build_dir_pattern
                        = format!("zpm/{}/build/<>", locator.slug());

                    Path::temp_dir_pattern(&build_dir_pattern)?
                        .relative_to(&project.project_cwd)
                },
            };

            package_build_entries.insert(locator.clone(), all_build_entries.len());
            all_build_entries.push(build::BuildRequest {
                cwd: build_cwd,
                locator: locator.clone(),
                commands: package_flags.build_commands.clone(),
                allowed_to_fail: install.install_state.resolution_tree.optional_builds.contains(locator),
                force_rebuild: is_freshly_unplugged,
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

    project.pnp_loader_path()
        .fs_change(std::include_str!("pnp.loader.mjs"), Permissions::from_mode(0o644))?;

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

