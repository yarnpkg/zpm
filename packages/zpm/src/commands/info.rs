use std::collections::{BTreeMap, BTreeSet};

use clipanion::cli;
use globset::GlobBuilder;
use indexmap::IndexMap;
use zpm_primitives::{DescriptorResolution, Locator, Reference};
use zpm_utils::{AbstractValue, Size, ToFileString};

use crate::{
    cache::CompositeCache, error::Error, install::InstallState, project::{Project, Workspace}, ui::{self, tree::{Node, TreeNodeChildren}}
};

#[cli::command]
#[cli::path("info")]
#[cli::category("Package information")]
#[cli::description("See information related to packages")]
#[cli::usage(r#"
This command prints various information related to the specified packages, accepting glob patterns.

By default, if the locator reference is missing, Yarn will default to print the information about all the matching direct dependencies of the package for the active workspace. To instead print all versions of the package that are direct dependencies of any of your workspaces, use the `-A,--all` flag. Adding the `-R,--recursive` flag will also report transitive dependencies.

Some fields will be hidden by default in order to keep the output readable, but can be selectively displayed by using additional options (`--dependents`, `--manifest`, `--virtuals`, ...) described in the option descriptions.

Note that this command will only print the information directly related to the selected packages - if you wish to know why the package is there in the first place, use `yarn why` which will do just that (it also provides a `-R,--recursive` flag that may be of some help).
"#)]
pub struct Info {
    #[cli::option("-A,--all", default = false)]
    #[cli::description("Print versions of a package from the whole project")]
    all: bool,

    #[cli::option("-R,--recursive", default = false)]
    #[cli::description("Print information for all packages, including transitive dependencies")]
    recursive: bool,

    #[cli::option("-X,--extra", default = Vec::new())]
    #[cli::description("An array of requests of extra data provided by plugins")]
    extra: Vec<String>,

    #[cli::option("--cache", default = false)]
    #[cli::description("Print information about the cache entry of a package (path, size, checksum)")]
    cache: bool,

    #[cli::option("--dependents", default = false)]
    #[cli::description("Print all dependents for each matching package")]
    dependents: bool,

    #[cli::option("--manifest", default = false)]
    #[cli::description("Print data obtained by looking at the package archive (license, homepage, ...)")]
    manifest: bool,

    #[cli::option("--name-only", default = false)]
    #[cli::description("Only print the name for the matching packages")]
    name_only: bool,

    #[cli::option("--virtuals", default = false)]
    #[cli::description("Print each instance of the virtual packages")]
    virtuals: bool,

    #[cli::option("--json", default = false)]
    #[cli::description("Format the output as an NDJSON stream")]
    json: bool,

    #[cli::positional]
    patterns: Vec<String>,
}

impl Info {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project = Project::new(None).await?;

        let active_workspace_idx = if !self.all {
            match project.active_workspace_idx() {
                Ok(idx) => Some(idx),
                Err(_) => return Err(Error::ProjectNotFound(project.shell_cwd.clone())),
            }
        } else {
            None
        };

        project
            .import_install_state()?;

        let package_cache
            = project.package_cache()?;

        let mut extra_set
            = BTreeSet::new();

        for extra in &self.extra {
            extra_set.insert(extra.as_str());
        }

        if self.cache {
            extra_set.insert("cache");
        }

        if self.dependents {
            extra_set.insert("dependents");
        }

        if self.manifest {
            extra_set.insert("manifest");
        }

        let filter
            = self.get_filter()?;
        let selection
            = self.extract_selection(&project, active_workspace_idx, filter)?;

        if selection.is_empty() {
            return Err(Error::ConflictingOptions("No package matched your request".to_string()));
        }

        let dependent_map
            = self.build_dependent_map(&project, &selection)?;
        let virtual_map
            = self.build_virtual_map(&project, &selection)?;

        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let mut root_children
            = vec![];

        for locator in selection {
            root_children.push(self.generate_info_node(
                &package_cache,
                install_state,
                &dependent_map,
                &virtual_map,
                locator,
            ));
        }

        let root_node = ui::tree::Node {
            label: None,
            value: None,
            children: Some(ui::tree::TreeNodeChildren::Vec(root_children)),
        };

        let rendering
            = ui::tree::TreeRenderer::new()
                .render(&root_node, self.json);

        print!("{}", rendering);

        Ok(())
    }

    fn generate_info_node(&self, package_cache: &CompositeCache, install_state: &InstallState, dependent_map: &BTreeMap<Locator, BTreeSet<Locator>>, virtual_map: &BTreeMap<Locator, BTreeSet<Locator>>, locator: Locator) -> ui::tree::Node<'_> {
        let mut children
            = IndexMap::new();

        if !self.name_only {
            let resolution
                = install_state.normalized_resolutions.get(&locator)
                    .expect("Expected the locator to be in the normalized resolutions");

            children.insert("Version".to_string(), Node {
                label: Some("Version".to_string()),
                value: Some(AbstractValue::new(resolution.version.clone())),
                children: None,
            });

            if self.cache {
                let cache_path
                    = package_cache.key_path(&locator, ".zip");

                let cache_size = cache_path
                    .fs_metadata()
                    .ok()
                    .map(|metadata| metadata.len());

                let mut cache_children = vec![
                    Node {
                        label: Some("Path".to_string()),
                        value: Some(AbstractValue::new(cache_path)),
                        children: None,
                    },
                ];

                if let Some(cache_size) = cache_size {
                    cache_children.push(Node {
                        label: Some("Size".to_string()),
                        value: Some(AbstractValue::new(Size::new(cache_size))),
                        children: None,
                    });
                }

                children.insert("Cache".to_string(), Node {
                    label: Some("Cache".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(cache_children)),
                });
            }

            let dep_children
                = resolution.dependencies.values()
                    .map(|descriptor| (descriptor, &install_state.resolution_tree.descriptor_to_locator[descriptor]))
                    .map(|(descriptor, locator)| DescriptorResolution::new(descriptor.clone(), locator.clone()))
                    .map(|descriptor_resolution| Node::new_value(descriptor_resolution))
                    .collect::<Vec<_>>();

            if dep_children.len() > 0 {
                children.insert("Dependencies".to_string(), Node {
                    label: Some("Dependencies".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(dep_children)),
                });
            }
        }

        ui::tree::Node {
            label: None,
            value: Some(AbstractValue::new(locator)),
            children: Some(TreeNodeChildren::Map(children)),
        }
    }

    fn extract_selection(&self, project: &Project, active_workspace_idx: Option<usize>, filter: impl Fn(&Locator) -> bool) -> Result<BTreeSet<Locator>, Error> {
        if self.all {
            if self.recursive {
                return Ok(self.extract_packages_from_project(project, filter)?);
            } else {
                return Ok(self.extract_packages_from_project_dependencies(project, filter)?);
            }
        }

        let active_workspace_idx = active_workspace_idx
            .ok_or_else(|| Error::ProjectNotFound(project.shell_cwd.clone()))?;

        let workspace
            = &project.workspaces[active_workspace_idx];

        if self.recursive {
            Ok(self.extract_packages_from_recursive_traversal(project, &workspace, filter)?)
        } else {
            Ok(self.extract_packages_from_workspace_dependencies(project, &workspace, filter)?)
        }
    }

    fn extract_packages_from_project(&self, project: &Project, filter: impl Fn(&Locator) -> bool) -> Result<BTreeSet<Locator>, Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let selection
            = install_state.resolution_tree.locator_resolutions.keys()
                .map(|locator| locator.physical_locator())
                .filter(filter)
                .collect::<BTreeSet<_>>();

        Ok(selection)
    }

    fn extract_packages_from_project_dependencies(&self, project: &Project, filter: impl Fn(&Locator) -> bool) -> Result<BTreeSet<Locator>, Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let workspace_resolutions
            = project.workspaces
                .iter()
                .map(|workspace| install_state.resolution_tree.locator_resolutions.get(&workspace.locator()).expect("Expected the workspace to be in the resolution tree"));

        let selection
            = workspace_resolutions.into_iter()
                .flat_map(|resolution| resolution.dependencies.values())
                .map(|descriptor| install_state.resolution_tree.descriptor_to_locator.get(descriptor).expect("Expected the descriptor to be in the resolution tree"))
                .map(|locator| locator.physical_locator())
                .filter(filter)
                .collect::<BTreeSet<_>>();

        Ok(selection)
    }

    fn extract_packages_from_workspace_dependencies(&self, project: &Project, workspace: &Workspace, filter: impl Fn(&Locator) -> bool) -> Result<BTreeSet<Locator>, Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let workspace_resolution
            = install_state.resolution_tree.locator_resolutions
                .get(&workspace.locator())
                .expect("Expected the workspace to be in the resolution tree");

        let selection
            = workspace_resolution.dependencies.values()
                .map(|descriptor| install_state.resolution_tree.descriptor_to_locator.get(descriptor).expect("Expected the descriptor to be in the resolution tree"))
                .map(|locator| locator.physical_locator())
                .filter(filter)
                .collect::<BTreeSet<_>>();

        Ok(selection)
    }

    fn extract_packages_from_recursive_traversal(&self, project: &Project, workspace: &Workspace, filter: impl Fn(&Locator) -> bool) -> Result<BTreeSet<Locator>, Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let workspace_resolution
            = install_state.resolution_tree.locator_resolutions
                .get(&workspace.locator())
                .expect("Expected the workspace to be in the resolution tree");

        let mut seen
            = BTreeSet::new();
        let mut queue
            = vec![&workspace_resolution.locator];

        while let Some(locator) = queue.pop() {
            if !seen.insert(locator) {
                continue;
            }

            if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
                for descriptor in resolution.dependencies.values() {
                    if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                        queue.push(dep_locator);
                    }
                }
            }
        }

        let traversed
            = seen.into_iter()
                .map(|locator| locator.physical_locator())
                .filter(|locator| filter(locator))
                .collect::<BTreeSet<_>>();

        Ok(traversed)
    }

    fn get_filter(&self) -> Result<impl Fn(&Locator) -> bool, Error> {
        let matchers = self.patterns
            .iter()
            .map(|pattern| GlobBuilder::new(pattern).literal_separator(false).build())
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .map(|glob| glob.compile_matcher())
            .collect::<Vec<_>>();

        Ok(move |locator: &Locator| {
            matchers.is_empty() || matchers.iter().any(|matcher| matcher.is_match(locator.ident.to_file_string()))
        })
    }

    fn build_dependent_map(&self, project: &Project, selection: &BTreeSet<Locator>) -> Result<BTreeMap<Locator, BTreeSet<Locator>>, Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let mut dependent_map
            = BTreeMap::new();

        for (locator, resolution) in &install_state.resolution_tree.locator_resolutions {
            for descriptor in resolution.dependencies.values() {
                let dependency = install_state.resolution_tree.descriptor_to_locator
                    .get(descriptor)
                    .expect("Expected the descriptor to be in the resolution tree")
                    .physical_locator();

                if selection.contains(&dependency) {
                    dependent_map.entry(dependency)
                        .or_insert_with(BTreeSet::new)
                        .insert(locator.clone());
                }
            }
        }

        Ok(dependent_map)
    }

    fn build_virtual_map(&self, project: &Project, selection: &BTreeSet<Locator>) -> Result<BTreeMap<Locator, BTreeSet<Locator>>, Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let mut virtual_map
            = BTreeMap::new();

        for locator in install_state.resolution_tree.locator_resolutions.keys() {
            if matches!(&locator.reference, Reference::Virtual(_)) {
                let physical_locator
                    = locator.physical_locator();

                if selection.contains(&physical_locator) {
                    virtual_map.entry(physical_locator)
                        .or_insert_with(BTreeSet::new)
                        .insert(locator.clone());
                }
            }
        }

        Ok(virtual_map)
    }
}

//     async fn build_info_tree<'a>(
//         &self,
//         project: &'a Project,
//         selection: &BTreeSet<Locator>,
//         dependent_map: &BTreeMap<&'a Locator, Vec<&'a Locator>>,
//         all_instances: &BTreeMap<Locator, Vec<&'a Locator>>,
//         extra_set: &BTreeSet<&str>,
//     ) -> Result<Node, Error> {
//         let mut root_children = Vec::new();

//         let cache = if extra_set.contains("cache") || extra_set.contains("manifest") {
//             Some(project.package_cache()?)
//         } else {
//             None
//         };

//         for locator in selection {
//             let is_virtual = matches!(&locator.reference, Reference::Virtual(_));
//             if !self.virtuals && is_virtual {
//                 continue;
//             }

//             let node = self.build_package_node(
//                 project,
//                 locator,
//                 dependent_map,
//                 all_instances,
//                 extra_set,
//                 cache.as_ref(),
//             ).await?;

//             root_children.push(node);
//         }

//         Ok(Node {
//             label: String::new(),
//             children: root_children,
//         })
//     }

//     async fn build_package_node<'a>(
//         &self,
//         project: &'a Project,
//         locator: &'a Locator,
//         dependent_map: &BTreeMap<&'a Locator, Vec<&'a Locator>>,
//         all_instances: &BTreeMap<Locator, Vec<&'a Locator>>,
//         extra_set: &BTreeSet<&str>,
//         cache: Option<&CompositeCache>,
//     ) -> Result<Node, Error> {
//         let install_state = project.install_state.as_ref()
//             .ok_or(Error::InstallStateNotFound)?;

//         let is_virtual = matches!(&locator.reference, Reference::Virtual(_));

//         let mut label = locator.to_file_string();
//         if self.name_only {
//             label = locator.ident.to_file_string();
//         }

//         let mut children = Vec::new();

//         if !self.name_only {
//             // Add version
//             if let Some(resolution) = install_state.normalized_resolutions.get(locator) {
//                 children.push(Node {
//                     label: format!("Version: {}", resolution.version.to_file_string()),
//                     children: vec![],
//                 });
//             }

//             // Add instances count for non-virtual packages
//             if !is_virtual {
//                 if let Some(instances) = all_instances.get(locator) {
//                     children.push(Node {
//                         label: format!("Instances: {}", instances.len()),
//                         children: vec![],
//                     });
//                 }
//             }

//             // Add extra information
//             if !is_virtual {
//                 // Manifest information
//                 if extra_set.contains("manifest") {
//                     if let Some(manifest_node) = self.build_manifest_node(project, locator, cache).await? {
//                         children.push(manifest_node);
//                     }
//                 }

//                 // Cache information
//                 if extra_set.contains("cache") && cache.is_some() {
//                     if let Some(cache_node) = self.build_cache_node(project, locator, cache).await? {
//                         children.push(cache_node);
//                     }
//                 }
//             }

//             // Add binaries
//             if !is_virtual {
//                 if let Ok(binaries) = project.package_self_binaries(locator) {
//                     if !binaries.is_empty() {
//                         let mut bin_children = Vec::new();
//                         for (name, _binary) in binaries {
//                             bin_children.push(Node {
//                                 label: name,
//                                 children: vec![],
//                             });
//                         }
//                         children.push(Node {
//                             label: "Exported Binaries".to_string(),
//                             children: bin_children,
//                         });
//                     }
//                 }
//             }

//             // Add dependents
//             if let Some(dependents) = dependent_map.get(locator) {
//                 if !dependents.is_empty() {
//                     let mut dep_children = Vec::new();
//                     for dependent in dependents {
//                         dep_children.push(Node {
//                             label: dependent.to_file_string(),
//                             children: vec![],
//                         });
//                     }
//                     children.push(Node {
//                         label: "Dependents".to_string(),
//                         children: dep_children,
//                     });
//                 }
//             }

//             // Add dependencies
//             if !is_virtual {
//                 if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
//                     if !resolution.dependencies.is_empty() {
//                         let mut dep_children = Vec::new();
//                         for (_ident, descriptor) in &resolution.dependencies {
//                             let dep_locator = install_state.resolution_tree.descriptor_to_locator
//                                 .get(descriptor);

//                             let label = if let Some(dep_locator) = dep_locator {
//                                 format!("{} → {}", descriptor.to_file_string(), dep_locator.to_file_string())
//                             } else {
//                                 descriptor.to_file_string()
//                             };

//                             dep_children.push(Node {
//                                 label,
//                                 children: vec![],
//                             });
//                         }
//                         children.push(Node {
//                             label: "Dependencies".to_string(),
//                             children: dep_children,
//                         });
//                     }
//                 }
//             }

//             // Add peer dependencies for virtual packages
//             if is_virtual {
//                 if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
//                     if !resolution.peer_dependencies.is_empty() {
//                         let mut peer_children = Vec::new();
//                         for (ident, peer_range) in &resolution.peer_dependencies {
//                             let descriptor = Descriptor {
//                                 ident: ident.clone(),
//                                 range: peer_range.to_range().clone(),
//                                 parent: None,
//                             };

//                             let dep_locator = resolution.dependencies.get(ident)
//                                 .and_then(|desc| install_state.resolution_tree.descriptor_to_locator.get(desc));

//                             let label = if let Some(dep_locator) = dep_locator {
//                                 format!("{} → {}", descriptor.to_file_string(), dep_locator.to_file_string())
//                             } else {
//                                 descriptor.to_file_string()
//                             };

//                             peer_children.push(Node {
//                                 label,
//                                 children: vec![],
//                             });
//                         }
//                         children.push(Node {
//                             label: "Peer dependencies".to_string(),
//                             children: peer_children,
//                         });
//                     }
//                 }
//             }
//         }

//         Ok(Node {
//             label,
//             children,
//         })
//     }

//     async fn build_manifest_node<'a>(
//         &self,
//         project: &'a Project,
//         locator: &'a Locator,
//         _cache: Option<&CompositeCache>,
//     ) -> Result<Option<Node>, Error> {
//         // Skip for workspace and link packages
//         if matches!(&locator.reference, Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_) | Reference::Link(_)) {
//             return Ok(None);
//         }

//         let install_state = project.install_state.as_ref()
//             .ok_or(Error::InstallStateNotFound)?;

//         // Try to get the package location
//         let location = install_state.locations_by_package.get(locator);
//         if location.is_none() {
//             return Ok(None);
//         }
//         let location = location.unwrap();

//         // Try to read manifest from package location
//         let manifest_path = project.project_cwd
//             .with_join(location)
//             .with_join_str("package.json");

//         let manifest_text = manifest_path.fs_read_text_with_zip()
//             .map_err(|_| Error::ManifestNotFound)?;

//         let raw_manifest: serde_json::Value = sonic_rs::from_str(&manifest_text)
//             .map_err(|_| Error::ManifestParseError(location.clone()))?;

//         let mut children = Vec::new();

//         // Extract license from raw manifest
//         if let Some(license) = raw_manifest.get("license").and_then(|v| v.as_str()) {
//             children.push(Node {
//                 label: format!("License: {}", license),
//                 children: vec![],
//             });
//         }

//         // Extract homepage from raw manifest
//         if let Some(homepage) = raw_manifest.get("homepage").and_then(|v| v.as_str()) {
//             children.push(Node {
//                 label: format!("Homepage: {}", homepage),
//                 children: vec![],
//             });
//         }

//         if children.is_empty() {
//             return Ok(None);
//         }

//         Ok(Some(Node {
//             label: "Manifest".to_string(),
//             children,
//         }))
//     }

//     async fn build_cache_node<'a>(
//         &self,
//         project: &'a Project,
//         locator: &'a Locator,
//         cache: Option<&CompositeCache>,
//     ) -> Result<Option<Node>, Error> {
//         let cache = cache.ok_or(Error::ConflictingOptions("Cache not available".to_string()))?;

//         let lockfile = project.lockfile()?;
//         let entry = lockfile.entries.get(locator);
//         let checksum = entry.and_then(|e| e.checksum.as_ref());

//         let cache_path = cache.key_path(locator, ".zip")?;

//         let mut children = Vec::new();

//         if let Some(checksum) = checksum {
//             children.push(Node {
//                 label: format!("Checksum: {}", checksum.to_file_string()),
//                 children: vec![],
//             });
//         }

//         children.push(Node {
//             label: format!("Path: {}", cache_path.to_file_string()),
//             children: vec![],
//         });

//         // Try to get file size
//         if let Ok(metadata) = cache_path.fs_metadata() {
//             children.push(Node {
//                 label: format!("Size: {}", metadata.len()),
//                 children: vec![],
//             });
//         }

//         Ok(Some(Node {
//             label: "Cache".to_string(),
//             children,
//         }))
//     }
// }
