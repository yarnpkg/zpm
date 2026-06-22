use std::{collections::{BTreeMap, BTreeSet}, str::FromStr};

use serde::Serialize;
use zpm_config::NodePackageMapType;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator};
use zpm_utils::{Path, ToFileString, ToHumanString};

use crate::{error::Error, install::Install, project::Project, tree_resolver::ResolutionTree};

#[derive(Debug, Serialize)]
pub struct PackageMap {
    packages: BTreeMap<String, PackageMapPackage>,
}

#[derive(Debug, Serialize)]
struct PackageMapPackage {
    url: String,
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug)]
struct PackageMapNode {
    id: String,
    package_path: Path,
    dependency_names: Option<BTreeSet<String>>,
}

pub struct NodeModulesPackageMapBuilder<'a> {
    project: &'a Project,
    install: &'a Install,
    base_path: Path,
    package_map_nodes: BTreeMap<Path, PackageMapNode>,
    package_locations_by_node_modules_path: BTreeMap<Path, BTreeMap<String, Path>>,
}

pub struct PnpmPackageMapBuilder {
    base_path: Path,
    top_level_locator: Locator,
    package_map_type: NodePackageMapType,
    package_map_nodes_by_locator: BTreeMap<Locator, PnpmPackageMapNode>,
}

#[derive(Debug)]
struct PnpmPackageMapNode {
    package_location: Path,
    dependencies: BTreeMap<String, Locator>,
}

impl<'a> NodeModulesPackageMapBuilder<'a> {
    pub fn new(project: &'a Project, install: &'a Install) -> Self {
        Self::new_at(project, install, project.nm_path())
    }

    pub fn new_at(project: &'a Project, install: &'a Install, base_path: Path) -> Self {
        Self {
            project,
            install,
            base_path,
            package_map_nodes: BTreeMap::new(),
            package_locations_by_node_modules_path: BTreeMap::new(),
        }
    }

    pub fn register_package(&mut self, location: Path, package_path: Path, locator: &Locator) {
        let normalized_location
            = location.without_trailing_separators();
        let normalized_package_path
            = package_path.without_trailing_separators();

        let package_map_node = PackageMapNode {
            id: get_package_id(&self.base_path, &normalized_location),
            package_path: normalized_package_path.clone(),
            dependency_names: Some(get_package_dependency_names(self.project, self.install, locator, &normalized_package_path)),
        };

        self.package_map_nodes.insert(normalized_location.clone(), package_map_node);

        if let Some((node_modules_path, package_name)) = get_package_name(&normalized_location) {
            self.package_locations_by_node_modules_path
                .entry(node_modules_path)
                .or_default()
                .insert(package_name, normalized_location);
        }
    }

    pub fn build(&self) -> Result<PackageMap, Error> {
        let package_map_type
            = self.project.config.settings.node_package_map_type.value;

        let mut packages
            = BTreeMap::new();

        for package_map_node in self.package_map_nodes.values() {
            packages.insert(package_map_node.id.clone(), PackageMapPackage {
                url: get_relative_url(&self.base_path, &package_map_node.package_path),
                dependencies: self.get_package_dependencies(
                    &package_map_node.package_path,
                    match package_map_type {
                        NodePackageMapType::Standard => package_map_node.dependency_names.as_ref(),
                        NodePackageMapType::Loose => None,
                    },
                )?,
            });
        }

        Ok(PackageMap {
            packages,
        })
    }

    fn get_package_dependencies(&self, package_path: &Path, dependency_names: Option<&BTreeSet<String>>) -> Result<BTreeMap<String, String>, Error> {
        let mut dependencies
            = BTreeMap::new();

        let mut current_path
            = package_path.clone();

        loop {
            let node_modules_path
                = current_path.with_join_str("node_modules");

            if let Some(package_locations) = self.package_locations_by_node_modules_path.get(&node_modules_path) {
                for (dependency_name, dependency_location) in package_locations {
                    if let Some(dependency_names) = dependency_names {
                        if !dependency_names.contains(dependency_name) {
                            continue;
                        }
                    }

                    if dependencies.contains_key(dependency_name) {
                        continue;
                    }

                    let dependency
                        = self.package_map_nodes
                            .get(dependency_location)
                            .ok_or_else(|| package_map_error(format!("expected {dependency_location:?} to have been registered")))?;

                    dependencies.insert(dependency_name.clone(), dependency.id.clone());
                }
            }

            let Some(parent_path) = current_path.dirname() else {
                break;
            };

            if parent_path == current_path {
                break;
            }

            current_path = parent_path;
        }

        Ok(dependencies)
    }
}

impl PnpmPackageMapBuilder {
    pub fn new(project: &Project) -> Self {
        Self {
            base_path: project.nm_path(),
            top_level_locator: project.root_workspace().locator(),
            package_map_type: project.config.settings.node_package_map_type.value,
            package_map_nodes_by_locator: BTreeMap::new(),
        }
    }

    pub fn register_package(&mut self, locator: &Locator, package_location: Path) {
        self.package_map_nodes_by_locator
            .entry(locator.clone())
            .or_insert_with(|| PnpmPackageMapNode {
                package_location: package_location.without_trailing_separators(),
                dependencies: BTreeMap::new(),
            });
    }

    pub fn register_dependency(&mut self, locator: &Locator, dependency_name: &Ident, dependency_locator: &Locator) -> Result<(), Error> {
        if !self.package_map_nodes_by_locator.contains_key(dependency_locator) {
            return Err(package_map_error(format!("expected dependency {} to have been registered", dependency_locator.to_print_string())));
        }

        let package_map_node
            = self.package_map_nodes_by_locator
                .get_mut(locator)
                .ok_or_else(|| package_map_error(format!("expected {} to have been registered", locator.to_print_string())))?;

        package_map_node.dependencies.insert(dependency_name.as_str().to_string(), dependency_locator.clone());

        Ok(())
    }

    pub fn build(&self) -> Result<PackageMap, Error> {
        let top_level_package_map_node
            = self.package_map_nodes_by_locator
                .get(&self.top_level_locator)
                .ok_or_else(|| package_map_error("expected the top-level package to have been registered"))?;

        let package_ids_by_locator: BTreeMap<Locator, String>
            = self.package_map_nodes_by_locator
                .iter()
                .map(|(locator, package_map_node)| {
                    (locator.clone(), get_package_id(&self.base_path, &package_map_node.package_location))
                })
                .collect();

        let mut packages
            = BTreeMap::new();

        let mut package_map_nodes
            = self.package_map_nodes_by_locator.values().collect::<Vec<_>>();

        package_map_nodes.sort_by_key(|package_map_node| {
            get_package_id(&self.base_path, &package_map_node.package_location)
        });

        for package_map_node in package_map_nodes {
            let dependencies = match self.package_map_type {
                NodePackageMapType::Standard => package_map_node.dependencies.clone(),
                NodePackageMapType::Loose => {
                    let mut dependencies
                        = top_level_package_map_node.dependencies.clone();

                    dependencies.extend(package_map_node.dependencies.clone());
                    dependencies
                },
            };

            packages.insert(get_package_id(&self.base_path, &package_map_node.package_location), PackageMapPackage {
                url: get_relative_url(&self.base_path, &package_map_node.package_location),
                dependencies: serialize_pnpm_dependencies(&dependencies, &package_ids_by_locator)?,
            });
        }

        Ok(PackageMap {
            packages,
        })
    }
}

pub fn persist_package_map(project: &Project, package_map: &PackageMap) -> Result<(), Error> {
    persist_package_map_at(&project.package_map_path(None), package_map)
}

pub fn persist_package_map_at(package_map_path: &Path, package_map: &PackageMap) -> Result<(), Error> {
    if let Some(parent) = package_map_path.dirname() {
        parent.fs_create_dir_all()?;
    }

    package_map_path.fs_change(format!("{}\n", JsonDocument::to_string_pretty(package_map)?), false)?;

    Ok(())
}

fn serialize_pnpm_dependencies(dependencies: &BTreeMap<String, Locator>, package_ids_by_locator: &BTreeMap<Locator, String>) -> Result<BTreeMap<String, String>, Error> {
    dependencies
        .iter()
        .map(|(dependency_name, dependency_locator)| {
            let package_id
                = package_ids_by_locator
                    .get(dependency_locator)
                    .ok_or_else(|| package_map_error(format!("expected dependency {} to have a package id", dependency_locator.to_print_string())))?;

            Ok((dependency_name.clone(), package_id.clone()))
        })
        .collect()
}

fn package_map_error(message: impl Into<String>) -> Error {
    Error::PackageMapGenerationError(message.into())
}

fn get_package_dependency_names(project: &Project, install: &Install, locator: &Locator, package_path: &Path) -> BTreeSet<String> {
    let tree
        = &install.install_state.resolution_tree;

    let mut dependency_names = resolution_dependency_names(tree, locator)
        .or_else(|| workspace_package_dependency_names(project, tree, package_path))
        .unwrap_or_default();

    // Add implicit self-dependency for non-workspace packages when there's no explicit self-dependency
    if !locator.reference.is_workspace_reference() && !dependency_names.contains(locator.ident.as_str()) {
        dependency_names.insert(locator.ident.as_str().to_string());
    }

    dependency_names
}

fn resolution_dependency_names(tree: &ResolutionTree, locator: &Locator) -> Option<BTreeSet<String>> {
    tree.locator_resolutions
        .get(locator)
        .map(|resolution| {
            resolution.dependencies
                .keys()
                .map(|ident| ident.as_str().to_string())
                .collect()
        })
}

fn workspace_package_dependency_names(project: &Project, tree: &ResolutionTree, package_path: &Path) -> Option<BTreeSet<String>> {
    let package_rel_path = package_path.forward_relative_to(&project.project_cwd)?;
    let workspace = project.try_closest_workspace_by_rel_path(&package_rel_path)?;

    resolution_dependency_names(tree, &workspace.locator())
}

fn get_package_name(location: &Path) -> Option<(Path, String)> {
    let segments
        = location.components().collect::<Vec<_>>();

    let node_modules_index
        = segments.iter().rposition(|segment| *segment == "node_modules")?;

    let scope_or_name
        = segments.get(node_modules_index + 1)?;

    let node_modules_path
        = Path::from_str(&segments[..=node_modules_index].join("/")).ok()?;

    if !scope_or_name.starts_with('@') {
        return Some((node_modules_path, (*scope_or_name).to_string()));
    }

    let name
        = segments.get(node_modules_index + 2)?;

    Some((node_modules_path, format!("{scope_or_name}/{name}")))
}

fn get_relative_url(from: &Path, to: &Path) -> String {
    let relative_path
        = to.relative_to(from);

    let relative_path
        = if relative_path.is_empty() {
            ".".to_string()
        } else {
            relative_path.to_file_string()
        };

    if relative_path.starts_with('.') {
        relative_path
    } else {
        format!("./{relative_path}")
    }
}

fn get_package_id(base_path: &Path, location: &Path) -> String {
    let relative_path
        = location.relative_to(base_path);

    let relative_path
        = if relative_path.is_empty() {
            ".".to_string()
        } else {
            relative_path.to_file_string()
        };

    if relative_path == ".." {
        ".".to_string()
    } else {
        relative_path
    }
}
