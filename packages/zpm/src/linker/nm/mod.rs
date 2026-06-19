use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{VersionFilter, Ident, Locator, Reference};
use zpm_sync::{SyncItem, SyncTemplate, SyncTree};
use zpm_utils::{FromFileString, IoResultExt, Path, ToHumanString};

use crate::{
    build::{self, BuildRequest, BuildRequests}, content_flags, error::Error, fetchers::PackageData, install::Install, linker::{self, LinkResult, helpers::PackageMeta, nm::hoist::{Hoister, WorkTree}, package_map::{NodeModulesPackageMapBuilder, persist_package_map, persist_package_map_at}}, project::Project
};

pub mod hoist;

const EXPECT_CHILDREN: &str = "All nodes should be expanded by the end of the hoisting process";

fn collect_binaries_from_dependencies(install: &Install, children: &BTreeMap<Ident, usize>, work_tree: &WorkTree) -> BTreeMap<String, (Ident, Path)> {
    let mut binaries
        = BTreeMap::new();

    for (ident, child_idx) in children {
        let child_node
            = &work_tree.nodes[*child_idx];

        let physical_locator
            = child_node.locator.physical_locator();

        if let Some(content_flags) = install.install_state.content_flags.get(&physical_locator) {
            for (bin_name, binary) in &content_flags.binaries {
                if let content_flags::Binary::Node(bin_path) = binary {
                    binaries.insert(bin_name.clone(), (ident.clone(), bin_path.clone()));
                }
            }
        }
    }

    binaries
}

fn collect_workspace_binaries(install: &Install, workspace_node: &hoist::WorkNode) -> BTreeMap<String, (Ident, Path)> {
    let mut binaries
        = BTreeMap::new();

    let physical_locator
        = workspace_node.locator.physical_locator();

    if let Some(content_flags) = install.install_state.content_flags.get(&physical_locator) {
        for (bin_name, binary) in &content_flags.binaries {
            if let content_flags::Binary::Node(bin_path) = binary {
                binaries.insert(bin_name.clone(), (workspace_node.locator.ident.clone(), bin_path.clone()));
            }
        }
    }

    binaries
}

/// Registers bin symlinks in the sync tree for a given node_modules subfolder.
/// `node_rel_path` is the path within node_modules where dependencies are located
/// (e.g., "" for top-level, or "foo/node_modules" for nested).
fn register_bin_symlinks_at_path(workspace_nm_tree: &mut SyncTree, node_rel_path: &Path, binaries: &BTreeMap<String, (Ident, Path)>) -> Result<(), Error> {
    for (bin_name, (dep_ident, bin_path)) in binaries {
        // The symlink will be at <node_rel_path>/.bin/<bin_name>
        // It needs to point to ../<dep_name>/<bin_path>
        let bin_symlink_path = node_rel_path
            .with_join_str(".bin")
            .with_join_str(bin_name);

        let target_path = Path::new()
            .with_join_str("..")
            .with_join_str(&dep_ident.as_str())
            .with_join(bin_path);

        workspace_nm_tree.register_entry(bin_symlink_path, SyncItem::Symlink {
            target_path,
        })?;
    }

    Ok(())
}

/// Registers `<host>/node_modules/<workspace-name>` symlinks for
/// workspaces with `selfReferences` enabled so they can be required
/// by name. Skipped on name collisions (dep wins), anonymous
/// workspaces, and resolutions dropped from the tree (e.g. via
/// `workspaces focus`).
fn register_workspace_symlinks_at(
    project: &Project,
    install: &Install,
    workspace_nm_tree: &mut SyncTree,
    mut package_map_builder: Option<&mut NodeModulesPackageMapBuilder>,
    host_node: &hoist::WorkNode,
    host_abs_path: &Path,
    candidate_workspaces: impl IntoIterator<Item = (Ident, Path)>,
) -> Result<(), Error> {
    let global_default = project.config.settings.nm_self_references.value;
    let host_children = host_node.children.as_ref();

    for (workspace_ident, workspace_dir) in candidate_workspaces {
        if host_children.map_or(false, |children| children.contains_key(&workspace_ident)) {
            continue;
        }

        let target_workspace = project.workspace_by_ident(&workspace_ident).ok();

        let Some(target_workspace) = target_workspace else {
            continue;
        };

        if target_workspace.manifest.name.is_none() {
            continue;
        }

        // Skip workspaces excluded by `workspaces focus` so their
        // incomplete state doesn't leak into the project root.
        // `project.install_state` isn't attached yet, so consult the
        // Install we were handed.
        let target_locator = target_workspace.locator();
        if !install.install_state.resolution_tree.locator_resolutions.contains_key(&target_locator) {
            continue;
        }

        let per_workspace = target_workspace.manifest.install_config
            .as_ref()
            .and_then(|cfg| cfg.self_references);

        if !per_workspace.unwrap_or(global_default) {
            continue;
        }

        // Hoisting-limited workspaces shouldn't surface in the project
        // root either — their packages stay inside their own border.
        let install_limit = target_workspace.manifest.install_config
            .as_ref()
            .and_then(|cfg| cfg.hoisting_limits)
            .map(zpm_config::NmHoistingLimits::from)
            .unwrap_or(project.config.settings.nm_hoisting_limits.value);

        if install_limit != zpm_config::NmHoistingLimits::None
            && target_workspace.rel_path != Path::new()
        {
            continue;
        }

        let symlink_path
            = Path::new()
                .with_join_str(workspace_ident.as_str());

        let target_path
            = workspace_dir
                .relative_to(&host_abs_path.with_join(&symlink_path).dirname().unwrap_or_default());

        let symlink_location
            = host_abs_path.with_join(&symlink_path);

        workspace_nm_tree.register_entry(symlink_path, SyncItem::Symlink {
            target_path,
        })?;

        if let Some(package_map_builder) = package_map_builder.as_deref_mut() {
            package_map_builder.register_package(
                symlink_location,
                workspace_dir,
                &target_locator,
            );
        }
    }

    Ok(())
}

fn register_workspace_bin_symlinks(workspace_nm_tree: &mut SyncTree, workspace_path: &Path, binaries: &BTreeMap<String, (Ident, Path)>) -> Result<(), Error> {
    for (bin_name, (_ident, bin_path)) in binaries {
        let target_abs_path
            = workspace_path
                .with_join(bin_path);

        if !target_abs_path.fs_exists() {
            continue;
        }

        let bin_symlink_path = Path::new()
            .with_join_str(".bin")
            .with_join_str(bin_name);

        let target_path = Path::new()
            .with_join_str("..")
            .with_join_str("..")
            .with_join(bin_path);

        workspace_nm_tree.register_entry(bin_symlink_path, SyncItem::Symlink {
            target_path,
        })?;
    }

    Ok(())
}

fn generate_workspace_node_modules(
    project: &Project,
    install: &Install,
    work_tree: &WorkTree,
    workspace_node_idx: usize,
    package_map_builder: Option<&mut NodeModulesPackageMapBuilder>,
    packages_by_location: &mut BTreeMap<Path, zpm_primitives::Locator>,
    canonical_build_locations: &mut BTreeMap<Locator, Path>,
    force_rebuild_locators: &mut BTreeSet<Locator>,
    cas_extractions: &mut Vec<(Path, Locator)>,
) -> Result<(), Error> {
    let mut package_map_builder
        = package_map_builder;

    let hardlinks_mode = matches!(
        project.config.settings.nm_mode.value,
        zpm_config::NmMode::HardlinksLocal | zpm_config::NmMode::HardlinksGlobal,
    );

    let nm_mode_changed = nm_mode_transitioned(project);
    let force_wipe_before_extract = nm_mode_changed
        && project.config.settings.nm_mode.value == zpm_config::NmMode::Classic;

    let workspace_node
        = &work_tree.nodes[workspace_node_idx];

    let workspace
        = project.workspace_by_locator(&workspace_node.locator)?;

    packages_by_location.insert(workspace.rel_path.clone(), workspace_node.locator.clone());

    let workspace_dir
        = project.project_cwd
            .with_join(&workspace.rel_path);

    if let Some(package_map_builder) = package_map_builder.as_deref_mut() {
        package_map_builder.register_package(
            workspace_dir.clone(),
            workspace_dir.clone(),
            &workspace_node.locator,
        );
    }

    let workspace_abs_path
        = workspace_dir
            .with_join_str("node_modules");

    let mut workspace_nm_tree
        = SyncTree::new();

    workspace_nm_tree.dry_run = false;

    let workspace_binaries
        = collect_workspace_binaries(install, &work_tree.nodes[workspace_node_idx]);

    register_workspace_bin_symlinks(&mut workspace_nm_tree, &workspace_dir, &workspace_binaries)?;

    // Berry's `nmSelfReferences` puts every workspace into the
    // project root's node_modules. Nested workspaces don't get a
    // self-symlink in their own node_modules (no circular refs).
    let is_root_workspace
        = workspace.rel_path == Path::new();

    if is_root_workspace {
        // Only top-level workspaces self-reference: nested ones
        // resolve peers through their parent's node_modules, so
        // surfacing them at the root would mislead consumers.
        let non_root_workspace_paths: std::collections::BTreeSet<&Path> = project.workspaces.iter()
            .map(|ws| &ws.rel_path)
            .filter(|rel_path| !rel_path.is_empty())
            .collect();

        let candidate_workspaces: Vec<(Ident, Path)> = project.workspaces.iter()
            .filter(|ws| {
                ws.rel_path.iter_path().rev()
                    .skip(1)
                    .all(|ancestor| !non_root_workspace_paths.contains(&ancestor))
            })
            .map(|ws| (ws.name.clone(), project.project_cwd.with_join(&ws.rel_path)))
            .collect();

        register_workspace_symlinks_at(
            project,
            install,
            &mut workspace_nm_tree,
            package_map_builder.as_deref_mut(),
            &work_tree.nodes[workspace_node_idx],
            &workspace_abs_path,
            candidate_workspaces,
        )?;
    }

    let mut workspace_queue
        = vec![(Path::new(), workspace_node_idx)];

    while let Some((node_rel_path, node_idx)) = workspace_queue.pop() {
        let node
            = &work_tree.nodes[node_idx];

        let children
            = node.children.as_ref()
                .expect(EXPECT_CHILDREN);

        for (ident, child_idx) in children {
            let child_node
                = &work_tree.nodes[*child_idx];

            let child_rel_path
                = node_rel_path.with_join_str(&ident.as_str());

            // `child_rel_path` already includes `node_rel_path` —
            // joining it again would double-prepend (`foo/node_modules/
            // foo/node_modules/bar`).
            let abs_path
                = workspace_abs_path.with_join(&child_rel_path);

            let rel_path
                = abs_path
                    .relative_to(&project.project_cwd);

            packages_by_location.insert(rel_path, child_node.locator.clone());

            let package_data
                = install.package_data.get(&child_node.locator.physical_locator());

            // Symlinked children (portals, links, file:) can't host a
            // nested `node_modules`; their deps either hoisted up
            // already or surface in the post-hoist portal check.
            let is_symlinked
                = matches!(package_data, Some(PackageData::Local { .. }))
                    || child_node.locator.reference.physical_reference().is_link()
                    || child_node.locator.reference.physical_reference().is_portal();

            if !is_symlinked {
                workspace_queue.push((child_rel_path.with_join_str("node_modules"), *child_idx));
            }

            match package_data {
                Some(PackageData::Abstract) => {
                    return Err(Error::Unsupported);
                },

                Some(PackageData::Local {package_directory, ..}) => {
                    let child_abs_path
                        = workspace_abs_path.with_join(&child_rel_path);

                    if let Some(package_map_builder) = package_map_builder.as_deref_mut() {
                        package_map_builder.register_package(
                            child_abs_path.clone(),
                            package_directory.clone(),
                            &child_node.locator,
                        );
                    }

                    let target_path
                        = package_directory.relative_to(&child_abs_path.dirname().unwrap());

                    workspace_nm_tree.register_entry(child_rel_path, SyncItem::Symlink {
                        target_path: target_path.clone(),
                    })?;

                    canonical_build_locations
                        .entry(child_node.locator.clone())
                        .or_insert_with(|| package_directory.relative_to(&project.project_cwd));
                },

                Some(PackageData::Zip {archive_path, package_directory, ..}) => {
                    if let Some(package_map_builder) = package_map_builder.as_deref_mut() {
                        package_map_builder.register_package(
                            abs_path.clone(),
                            abs_path.clone(),
                            &child_node.locator,
                        );
                    }

                    // SyncTree re-extracts user-deleted destinations
                    // automatically; we just need to flag for rebuild
                    // so the build cache doesn't short-circuit.
                    let dest_abs_path = abs_path.clone();

                    // Transitioning back to classic nmMode: drop the
                    // folder so SyncTree rewrites regular (nlink=1)
                    // files instead of reusing existing hardlinks.
                    if force_wipe_before_extract && dest_abs_path.fs_exists() {
                        let _ = dest_abs_path.fs_rm().ok_missing();
                    }

                    if !dest_abs_path.fs_exists() {
                        force_rebuild_locators.insert(child_node.locator.clone());
                    }

                    if hardlinks_mode {
                        // Hand off to the CAS extractor; `Any` keeps
                        // the parent walk from treating the folder as
                        // extraneous while leaving its contents alone.
                        workspace_nm_tree.register_entry(child_rel_path, SyncItem::Any)?;
                        cas_extractions.push((dest_abs_path.clone(), child_node.locator.clone()));
                    } else {
                        workspace_nm_tree.register_entry(child_rel_path, SyncItem::Folder {
                            template: Some(SyncTemplate::Zip {
                                archive_path: archive_path.clone(),
                                inner_path: package_directory.relative_to(&archive_path),
                            }),
                        })?;
                    }

                    canonical_build_locations
                        .entry(child_node.locator.clone())
                        .or_insert_with(|| dest_abs_path.relative_to(&project.project_cwd));
                },

                Some(PackageData::MissingZip {..}) => {
                    // Nothing to do here
                },

                None => match &child_node.locator.reference {
                    Reference::Link(params) if params.path.starts_with('/') => {
                        let target_path
                            = Path::from_file_string(&params.path)?;

                        if let Some(package_map_builder) = package_map_builder.as_deref_mut() {
                            package_map_builder.register_package(
                                abs_path.clone(),
                                target_path.clone(),
                                &child_node.locator,
                            );
                        }

                        workspace_nm_tree.register_entry(child_rel_path, SyncItem::Symlink {
                            target_path,
                        })?;
                    },

                    _ => {
                        unreachable!("Expected package data for {}", ToHumanString::to_print_string(&child_node.locator.physical_locator()));
                    },
                },
            }
        }

        // Register bin symlinks for all direct dependencies of this node
        // Skip any bins that conflict with workspace binaries (workspace takes precedence)
        let mut binaries
            = collect_binaries_from_dependencies(install, children, &work_tree);

        // For root level, filter out bins that conflict with workspace binaries
        if node_rel_path.is_empty() {
            for bin_name in workspace_binaries.keys() {
                binaries.remove(bin_name);
            }
        }

        register_bin_symlinks_at_path(&mut workspace_nm_tree, &node_rel_path, &binaries)?;
    }

    workspace_nm_tree
        .run(workspace_abs_path.clone())?;

    // Always materialize node_modules: tools (and tests) expect the
    // directory to exist even when the workspace has no deps.
    workspace_abs_path.fs_create_dir_all()?;

    Ok(())
}

fn build_requests_from_locations(
    project: &Project,
    install: &Install,
    canonical_build_locations: &BTreeMap<Locator, Path>,
    force_rebuild_locators: &BTreeSet<Locator>,
    dependencies_meta: &Vec<(VersionFilter, PackageMeta)>,
) -> Result<BuildRequests, Error> {
    let tree
        = &install.install_state.resolution_tree;

    let mut all_build_entries
        = Vec::<BuildRequest>::new();
    let mut package_build_entries
        = BTreeMap::<Locator, usize>::new();

    for (locator, build_cwd) in canonical_build_locations {
        // Virtual locators share their build with the physical one.
        if locator.reference.is_virtual_reference() {
            continue;
        }

        let physical = locator.physical_locator();

        let Some(physical_package_data) = install.package_data.get(&physical) else {
            continue;
        };

        let Some(resolution) = tree.locator_resolutions.get(locator) else {
            continue;
        };

        let info = linker::helpers::get_package_internal_info(
            project,
            install,
            dependencies_meta,
            locator,
            resolution,
            physical_package_data,
        );

        if info.build_step.is_noop() {
            continue;
        }

        package_build_entries.insert(locator.clone(), all_build_entries.len());

        all_build_entries.push(build::BuildRequest {
            cwd: build_cwd.clone(),
            locator: locator.clone(),
            build_step: info.build_step,
            allowed_to_fail: tree.optional_builds.contains(locator),
            force_rebuild: force_rebuild_locators.contains(locator),
            inline_builds: install.inline_builds,
        });
    }

    let dependencies = linker::helpers::populate_build_entry_dependencies(
        &package_build_entries,
        &tree.locator_resolutions,
        &tree.descriptor_to_locator,
    )?;

    Ok(BuildRequests {
        entries: all_build_entries,
        dependencies,
    })
}

pub async fn link_island_nm(
    project: &Project,
    install: &Install,
    island: &crate::island::ResolvedIsland,
) -> Result<LinkResult, Error> {
    let mut packages_by_location
        = BTreeMap::new();

    let mut canonical_build_locations
        = BTreeMap::new();
    let mut force_rebuild_locators
        = BTreeSet::new();
    let mut cas_extractions
        = Vec::new();
    let mut package_maps
        = Vec::new();

    for workspace_ident in &island.workspace_idents {
        let workspace = project.workspace_by_ident(workspace_ident)?;
        let workspace_locator = workspace.locator();
        let package_map_base_path
            = workspace.path.with_join_str("node_modules");
        let mut package_map_builder
            = NodeModulesPackageMapBuilder::new_at(project, install, package_map_base_path.clone());

        let mut work_tree = WorkTree::new_for_island_workspace(
            project,
            &install.install_state,
            workspace_locator,
        );

        let mut hoister
            = Hoister::new(&mut work_tree);

        hoister.hoist();

        generate_workspace_node_modules(
            project,
            install,
            &work_tree,
            0,
            Some(&mut package_map_builder),
            &mut packages_by_location,
            &mut canonical_build_locations,
            &mut force_rebuild_locators,
            &mut cas_extractions,
        )?;

        package_maps.push((
            project.package_map_path(Some(workspace)),
            package_map_builder.build()?,
        ));
    }

    run_cas_extractions(project, install, &cas_extractions)?;

    for (package_map_path, package_map) in package_maps {
        persist_package_map_at(&package_map_path, &package_map)?;
    }

    let dependencies_meta
        = linker::helpers::TopLevelConfiguration::from_project(project);

    let build_requests = build_requests_from_locations(
        project,
        install,
        &canonical_build_locations,
        &force_rebuild_locators,
        &dependencies_meta,
    )?;

    Ok(LinkResult {
        packages_by_location,
        build_requests,
    })
}

fn nm_mode_token(mode: zpm_config::NmMode) -> &'static str {
    match mode {
        zpm_config::NmMode::Classic => "classic",
        zpm_config::NmMode::HardlinksLocal => "hardlinks-local",
        zpm_config::NmMode::HardlinksGlobal => "hardlinks-global",
    }
}

/// True when `nmMode` differs from the previous install — switching
/// `hardlinks-*` → `classic` has to break existing hardlinks.
fn nm_mode_transitioned(project: &Project) -> bool {
    let current = nm_mode_token(project.config.settings.nm_mode.value);
    let previous = project.install_state.as_ref()
        .and_then(|state| state.nm_mode.as_deref());
    match previous {
        None => false,
        Some(prev) => prev != current,
    }
}

/// Routes hardlink-mode extractions through a content-aware extractor.
/// `hardlinks-local` keeps the dedup table in memory (first
/// destination doubles as canonical copy); `hardlinks-global` shares
/// the dedup index across projects via the global CAS.
fn run_cas_extractions(
    project: &Project,
    install: &Install,
    cas_extractions: &[(Path, Locator)],
) -> Result<(), Error> {
    if cas_extractions.is_empty() {
        return Ok(());
    }

    match project.config.settings.nm_mode.value {
        zpm_config::NmMode::HardlinksLocal => {
            let mut local_index = BTreeMap::new();
            for (dest_abs_path, locator) in cas_extractions {
                let package_data = install.package_data
                    .get(&locator.physical_locator())
                    .unwrap_or_else(|| panic!("Expected package_data for {}", locator.to_print_string()));

                linker::helpers::fs_extract_archive_with_local_dedup(
                    dest_abs_path,
                    package_data,
                    &mut local_index,
                )?;
            }
        },
        zpm_config::NmMode::HardlinksGlobal => {
            let index_root = project.config.settings.global_folder.value
                .with_join_str("index");

            for (dest_abs_path, locator) in cas_extractions {
                let package_data = install.package_data
                    .get(&locator.physical_locator())
                    .unwrap_or_else(|| panic!("Expected package_data for {}", locator.to_print_string()));

                linker::helpers::fs_extract_archive_with_cas(dest_abs_path, package_data, &index_root)?;
            }
        },
        zpm_config::NmMode::Classic => {},
    }

    Ok(())
}

/// Warns when portals are present. Node resolves through the
/// symlink's target, so without `--preserve-symlinks` it can't see
/// dependencies hoisted into the parent's node_modules.
fn warn_about_portals_if_any(install: &Install) {
    let has_portal = install.install_state
        .resolution_tree
        .locator_resolutions
        .keys()
        .any(|locator| locator.reference.physical_reference().is_portal());

    if !has_portal {
        return;
    }

    crate::report::if_active(|report| {
        report.warn(
            "Portals are in use. Make sure to set --preserve-symlinks (or NODE_PRESERVE_SYMLINKS_MAIN=1) so node can resolve hoisted dependencies through the portal symlink.".to_string(),
        );
    });
}

/// Fails the install when an external portal has direct deps that
/// couldn't hoist into the portal's parent (a conflicting locator is
/// already there). Portal targets are read-only by convention, so we
/// can't write the dep nested under them. Internal portals
/// (project-relative paths) are exempt.
fn check_external_portal_conflicts(
    project: &Project,
    install: &Install,
    work_tree: &hoist::WorkTree,
) -> Result<(), Error> {
    let mut has_conflict = false;

    for node in &work_tree.nodes {
        // Peer-bearing portals wear a Virtual reference; unwrap for
        // the type check and keep `node.locator` for display.
        if !node.locator.reference.physical_reference().is_portal() {
            continue;
        }

        let Some(children) = &node.children else {
            continue;
        };

        if children.is_empty() {
            continue;
        }

        let Some(parent_idx) = node.parent_idx else {
            continue;
        };

        let parent_node = &work_tree.nodes[parent_idx];

        let Some(parent_children) = parent_node.children.as_ref() else {
            continue;
        };

        let package_directory = install.package_data
            .get(&node.locator.physical_locator())
            .and_then(|pd| match pd {
                PackageData::Local { package_directory, .. } => Some(package_directory),
                _ => None,
            });

        let is_internal = package_directory
            .map(|pd| project.project_cwd.contains(pd))
            .unwrap_or(false);

        if is_internal {
            continue;
        }

        for (child_ident, &child_idx) in children {
            let child_locator
                = &work_tree.nodes[child_idx].locator;

            let parent_locator = parent_children.get(child_ident)
                .map(|&idx| &work_tree.nodes[idx].locator);

            // If a sibling portal under the same parent originally
            // declared the conflicting version, blame it explicitly —
            // the hoister picked its copy first.
            let sibling_portal = parent_locator
                .and_then(|parent_loc| {
                    parent_children.iter()
                        .filter_map(|(_, &sibling_idx)| {
                            let sibling = &work_tree.nodes[sibling_idx];
                            if sibling_idx == child_idx {
                                return None;
                            }
                            if !sibling.locator.reference.physical_reference().is_portal() {
                                return None;
                            }
                            let original = sibling.dependencies.get(child_ident)?;
                            if original == parent_loc {
                                Some(&sibling.locator)
                            } else {
                                None
                            }
                        })
                        .next()
                });

            crate::report::if_active(|report| {
                match (parent_locator, sibling_portal) {
                    (Some(parent_locator), Some(sibling_locator)) => {
                        report.warn(format!(
                            "{}: dependency {} conflicts with dependency {} from sibling portal {}",
                            node.locator.to_print_string(),
                            child_locator.to_print_string(),
                            parent_locator.to_print_string(),
                            sibling_locator.ident.to_print_string(),
                        ));
                    },
                    (Some(parent_locator), None) => {
                        report.warn(format!(
                            "{}: dependency {} conflicts with parent dependency {}",
                            node.locator.to_print_string(),
                            child_locator.to_print_string(),
                            parent_locator.to_print_string(),
                        ));
                    },
                    (None, _) => {
                        report.warn(format!(
                            "{}: dependency {} can't be hoisted into the parent and the portal target is external",
                            node.locator.to_print_string(),
                            child_locator.to_print_string(),
                        ));
                    },
                }
            });

            has_conflict = true;
        }
    }

    if has_conflict {
        return Err(Error::SilentError);
    }

    Ok(())
}

pub async fn link_project_nm(project: &Project, install: &Install) -> Result<LinkResult, Error> {
    warn_about_portals_if_any(install);

    let mut work_tree
        = WorkTree::new(project, &install.install_state);

    let mut hoister
        = Hoister::new(&mut work_tree);

    let mut packages_by_location
        = BTreeMap::new();

    let mut canonical_build_locations
        = BTreeMap::new();
    let mut force_rebuild_locators
        = BTreeSet::new();
    let mut cas_extractions
        = Vec::new();

    hoister.hoist();

    check_external_portal_conflicts(project, install, &work_tree)?;

    let mut package_map_builder
        = NodeModulesPackageMapBuilder::new(project, install);

    let mut project_queue
        = vec![0usize];

    while let Some(workspace_node_idx) = project_queue.pop() {
        generate_workspace_node_modules(
            project,
            install,
            &work_tree,
            workspace_node_idx,
            Some(&mut package_map_builder),
            &mut packages_by_location,
            &mut canonical_build_locations,
            &mut force_rebuild_locators,
            &mut cas_extractions,
        )?;

        project_queue.extend_from_slice(&work_tree.nodes[workspace_node_idx].workspaces_idx);
    }

    run_cas_extractions(project, install, &cas_extractions)?;

    persist_package_map(project, &package_map_builder.build()?)?;

    let dependencies_meta
        = linker::helpers::TopLevelConfiguration::from_project(project);

    let build_requests = build_requests_from_locations(
        project,
        install,
        &canonical_build_locations,
        &force_rebuild_locators,
        &dependencies_meta,
    )?;

    Ok(LinkResult {
        packages_by_location,
        build_requests,
    })
}
