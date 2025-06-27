use std::collections::{BTreeMap, BTreeSet};

use clipanion::cli;
use globset::GlobBuilder;
use zpm_utils::{ToFileString, FromFileString};
use zpm_formats::zip::ZipSupport;

use crate::{
    cache::CompositeCache,
    error::Error,
    primitives::{Descriptor, Locator, Reference},
    project::{self, Project},
    ui::tree::Node,
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

        // Restore install state
        project.import_install_state()?;

        // Build extra data set
        let mut extra_set = BTreeSet::new();
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

        // Get the packages to look at based on options
        let lookup_set = self.get_lookup_set(&project, active_workspace_idx)?;
        
        // Find packages matching the patterns
        let selection = self.find_selection(&lookup_set)?;

        if selection.is_empty() {
            return Err(Error::ConflictingOptions("No package matched your request".to_string()));
        }

        // Build dependent map if needed
        let dependent_map = if self.dependents {
            self.build_dependent_map(&project)?
        } else {
            BTreeMap::new()
        };

        // Build virtual instances map
        let all_instances = self.build_virtual_instances(&lookup_set);

        // Build the output tree
        let root_node = self.build_info_tree(
            &project,
            &selection,
            &dependent_map,
            &all_instances,
            &extra_set,
        ).await?;

        // Output the tree
        if self.json {
            // TODO: Implement JSON output
            eprintln!("JSON output not yet implemented");
        } else {
            print!("{}", root_node.to_string());
        }

        Ok(())
    }

    fn get_lookup_set<'a>(
        &self,
        project: &'a Project,
        active_workspace_idx: Option<usize>,
    ) -> Result<Vec<&'a Locator>, Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        if self.all && self.recursive {
            // All packages in the project
            Ok(install_state.resolution_tree.locator_resolutions.keys().collect())
        } else if self.all {
            // All direct dependencies of all workspaces
            let mut packages = Vec::new();
            for workspace in &project.workspaces {
                let resolution = install_state.resolution_tree.locator_resolutions
                    .get(&workspace.locator())
                    .ok_or_else(|| Error::PackageNotFound(workspace.locator().ident.clone(), project.project_cwd.clone()))?;
                
                for descriptor in resolution.dependencies.values() {
                    if let Some(locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                        packages.push(locator);
                    }
                }
            }
            Ok(packages)
        } else {
            // Packages from active workspace
            let active_workspace_idx = active_workspace_idx
                .ok_or_else(|| Error::ProjectNotFound(project.shell_cwd.clone()))?;
            let workspace = &project.workspaces[active_workspace_idx];
            
            self.traverse_workspace(project, workspace, self.recursive)
        }
    }

    fn traverse_workspace<'a>(
        &self,
        project: &'a Project,
        workspace: &'a project::Workspace,
        recursive: bool,
    ) -> Result<Vec<&'a Locator>, Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let initial_locator = workspace.locator();
        let mut seen = BTreeSet::new();
        let mut queue = vec![&initial_locator];
        let mut result = Vec::new();

        while let Some(locator) = queue.pop() {
            if !seen.insert(locator.clone()) {
                continue;
            }

            // Find the actual reference in the resolution tree
            if let Some(tree_locator) = install_state.resolution_tree.locator_resolutions.keys()
                .find(|l| **l == *locator) {
                result.push(tree_locator);
            }

            if !recursive && *locator != initial_locator {
                continue;
            }

            // Add dependencies
            if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
                for descriptor in resolution.dependencies.values() {
                    if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                        queue.push(dep_locator);
                    }
                }
            }
        }

        Ok(result)
    }

    fn find_selection<'a>(
        &self,
        lookup_set: &[&'a Locator],
    ) -> Result<Vec<&'a Locator>, Error> {
        if self.patterns.is_empty() {
            // Return all packages if no patterns specified
            return Ok(lookup_set.to_vec());
        }

        let mut selection = Vec::new();
        
        for pattern in &self.patterns {
            // Try to parse pattern as a locator
            let pattern_locator = match <Locator as FromFileString>::from_file_string(pattern) {
                Ok(locator) => locator,
                Err(_) => {
                    // If not a valid locator, treat as a glob pattern on ident
                    let glob = GlobBuilder::new(pattern)
                        .literal_separator(false)
                        .build()?
                        .compile_matcher();
                    
                    for locator in lookup_set {
                        if glob.is_match(locator.ident.to_file_string()) {
                            selection.push(*locator);
                        }
                    }
                    continue;
                }
            };

            // Check if pattern is virtual
            let pattern_is_virtual = matches!(&pattern_locator.reference, Reference::Virtual(_));
            let base_pattern_locator = if pattern_is_virtual {
                pattern_locator.physical_locator()
            } else {
                pattern_locator.clone()
            };

            // Match against lookup set
            for locator in lookup_set {
                // Check ident match
                if locator.ident != pattern_locator.ident {
                    continue;
                }

                // If pattern has no reference, match all with same ident
                if pattern.contains('#') {
                    let locator_is_virtual = matches!(&locator.reference, Reference::Virtual(_));
                    let base_locator = if locator_is_virtual {
                        locator.physical_locator()
                    } else {
                        (*locator).clone()
                    };

                    // If pattern is virtual, only match exact virtual reference
                    if pattern_is_virtual && locator_is_virtual {
                        if pattern_locator.reference != locator.reference {
                            continue;
                        }
                    }

                    // Check base reference match
                    if base_pattern_locator.reference != base_locator.reference {
                        continue;
                    }
                }

                selection.push(*locator);
            }
        }

        // Deduplicate selection
        let mut unique_selection = Vec::new();
        let mut seen = BTreeSet::new();
        for locator in selection {
            if seen.insert(locator) {
                unique_selection.push(locator);
            }
        }

        Ok(unique_selection)
    }

    fn build_dependent_map<'a>(
        &self,
        project: &'a Project,
    ) -> Result<BTreeMap<&'a Locator, Vec<&'a Locator>>, Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let mut dependent_map = BTreeMap::new();

        for (locator, resolution) in &install_state.resolution_tree.locator_resolutions {
            for descriptor in resolution.dependencies.values() {
                if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                    dependent_map.entry(dep_locator).or_insert_with(Vec::new).push(locator);
                }
            }
        }

        Ok(dependent_map)
    }

    fn build_virtual_instances<'a>(
        &self,
        lookup_set: &[&'a Locator],
    ) -> BTreeMap<Locator, Vec<&'a Locator>> {
        let mut instances = BTreeMap::new();

        for locator in lookup_set {
            if matches!(&locator.reference, Reference::Virtual(_)) {
                let base = locator.physical_locator();
                instances.entry(base).or_insert_with(Vec::new).push(*locator);
            }
        }

        instances
    }

    async fn build_info_tree<'a>(
        &self,
        project: &'a Project,
        selection: &[&'a Locator],
        dependent_map: &BTreeMap<&'a Locator, Vec<&'a Locator>>,
        all_instances: &BTreeMap<Locator, Vec<&'a Locator>>,
        extra_set: &BTreeSet<&str>,
    ) -> Result<Node, Error> {
        let mut root_children = Vec::new();

        let cache = if extra_set.contains("cache") || extra_set.contains("manifest") {
            Some(project.package_cache()?)
        } else {
            None
        };

        for locator in selection {
            let is_virtual = matches!(&locator.reference, Reference::Virtual(_));
            if !self.virtuals && is_virtual {
                continue;
            }

            let node = self.build_package_node(
                project,
                locator,
                dependent_map,
                all_instances,
                extra_set,
                cache.as_ref(),
            ).await?;

            root_children.push(node);
        }

        Ok(Node {
            label: String::new(),
            children: root_children,
        })
    }

    async fn build_package_node<'a>(
        &self,
        project: &'a Project,
        locator: &'a Locator,
        dependent_map: &BTreeMap<&'a Locator, Vec<&'a Locator>>,
        all_instances: &BTreeMap<Locator, Vec<&'a Locator>>,
        extra_set: &BTreeSet<&str>,
        cache: Option<&CompositeCache>,
    ) -> Result<Node, Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let is_virtual = matches!(&locator.reference, Reference::Virtual(_));
        
        let mut label = locator.to_file_string();
        if self.name_only {
            label = locator.ident.to_file_string();
        }

        let mut children = Vec::new();

        if !self.name_only {
            // Add version
            if let Some(resolution) = install_state.normalized_resolutions.get(locator) {
                children.push(Node {
                    label: format!("Version: {}", resolution.version.to_file_string()),
                    children: vec![],
                });
            }

            // Add instances count for non-virtual packages
            if !is_virtual {
                if let Some(instances) = all_instances.get(locator) {
                    children.push(Node {
                        label: format!("Instances: {}", instances.len()),
                        children: vec![],
                    });
                }
            }

            // Add extra information
            if !is_virtual {
                // Manifest information
                if extra_set.contains("manifest") {
                    if let Some(manifest_node) = self.build_manifest_node(project, locator, cache).await? {
                        children.push(manifest_node);
                    }
                }

                // Cache information
                if extra_set.contains("cache") && cache.is_some() {
                    if let Some(cache_node) = self.build_cache_node(project, locator, cache).await? {
                        children.push(cache_node);
                    }
                }
            }

            // Add binaries
            if !is_virtual {
                if let Ok(binaries) = project.package_self_binaries(locator) {
                    if !binaries.is_empty() {
                        let mut bin_children = Vec::new();
                        for (name, _binary) in binaries {
                            bin_children.push(Node {
                                label: name,
                                children: vec![],
                            });
                        }
                        children.push(Node {
                            label: "Exported Binaries".to_string(),
                            children: bin_children,
                        });
                    }
                }
            }

            // Add dependents
            if let Some(dependents) = dependent_map.get(locator) {
                if !dependents.is_empty() {
                    let mut dep_children = Vec::new();
                    for dependent in dependents {
                        dep_children.push(Node {
                            label: dependent.to_file_string(),
                            children: vec![],
                        });
                    }
                    children.push(Node {
                        label: "Dependents".to_string(),
                        children: dep_children,
                    });
                }
            }

            // Add dependencies
            if !is_virtual {
                if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
                    if !resolution.dependencies.is_empty() {
                        let mut dep_children = Vec::new();
                        for (_ident, descriptor) in &resolution.dependencies {
                            let dep_locator = install_state.resolution_tree.descriptor_to_locator
                                .get(descriptor);
                            
                            let label = if let Some(dep_locator) = dep_locator {
                                format!("{} → {}", descriptor.to_file_string(), dep_locator.to_file_string())
                            } else {
                                descriptor.to_file_string()
                            };
                            
                            dep_children.push(Node {
                                label,
                                children: vec![],
                            });
                        }
                        children.push(Node {
                            label: "Dependencies".to_string(),
                            children: dep_children,
                        });
                    }
                }
            }

            // Add peer dependencies for virtual packages
            if is_virtual {
                if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(locator) {
                    if !resolution.peer_dependencies.is_empty() {
                        let mut peer_children = Vec::new();
                        for (ident, peer_range) in &resolution.peer_dependencies {
                            let descriptor = Descriptor {
                                ident: ident.clone(),
                                range: peer_range.to_range().clone(),
                                parent: None,
                            };
                            
                            let dep_locator = resolution.dependencies.get(ident)
                                .and_then(|desc| install_state.resolution_tree.descriptor_to_locator.get(desc));
                            
                            let label = if let Some(dep_locator) = dep_locator {
                                format!("{} → {}", descriptor.to_file_string(), dep_locator.to_file_string())
                            } else {
                                descriptor.to_file_string()
                            };
                            
                            peer_children.push(Node {
                                label,
                                children: vec![],
                            });
                        }
                        children.push(Node {
                            label: "Peer dependencies".to_string(),
                            children: peer_children,
                        });
                    }
                }
            }
        }

        Ok(Node {
            label,
            children,
        })
    }

    async fn build_manifest_node<'a>(
        &self,
        project: &'a Project,
        locator: &'a Locator,
        _cache: Option<&CompositeCache>,
    ) -> Result<Option<Node>, Error> {
        // Skip for workspace and link packages
        if matches!(&locator.reference, Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_) | Reference::Link(_)) {
            return Ok(None);
        }

        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        // Try to get the package location
        let location = install_state.locations_by_package.get(locator);
        if location.is_none() {
            return Ok(None);
        }
        let location = location.unwrap();

        // Try to read manifest from package location
        let manifest_path = project.project_cwd
            .with_join(location)
            .with_join_str("package.json");
        
        let manifest_text = manifest_path.fs_read_text_with_zip()
            .map_err(|_| Error::ManifestNotFound)?;
        
        let raw_manifest: serde_json::Value = sonic_rs::from_str(&manifest_text)
            .map_err(|_| Error::ManifestParseError(location.clone()))?;

        let mut children = Vec::new();

        // Extract license from raw manifest
        if let Some(license) = raw_manifest.get("license").and_then(|v| v.as_str()) {
            children.push(Node {
                label: format!("License: {}", license),
                children: vec![],
            });
        }

        // Extract homepage from raw manifest
        if let Some(homepage) = raw_manifest.get("homepage").and_then(|v| v.as_str()) {
            children.push(Node {
                label: format!("Homepage: {}", homepage),
                children: vec![],
            });
        }

        if children.is_empty() {
            return Ok(None);
        }

        Ok(Some(Node {
            label: "Manifest".to_string(),
            children,
        }))
    }

    async fn build_cache_node<'a>(
        &self,
        project: &'a Project,
        locator: &'a Locator,
        cache: Option<&CompositeCache>,
    ) -> Result<Option<Node>, Error> {
        let cache = cache.ok_or(Error::ConflictingOptions("Cache not available".to_string()))?;
        
        let lockfile = project.lockfile()?;
        let entry = lockfile.entries.get(locator);
        let checksum = entry.and_then(|e| e.checksum.as_ref());

        let cache_path = cache.key_path(locator, ".zip")?;
        
        let mut children = Vec::new();

        if let Some(checksum) = checksum {
            children.push(Node {
                label: format!("Checksum: {}", checksum.to_file_string()),
                children: vec![],
            });
        }

        children.push(Node {
            label: format!("Path: {}", cache_path.to_file_string()),
            children: vec![],
        });

        // Try to get file size
        if let Ok(metadata) = cache_path.fs_metadata() {
            children.push(Node {
                label: format!("Size: {}", metadata.len()),
                children: vec![],
            });
        }

        Ok(Some(Node {
            label: "Cache".to_string(),
            children,
        }))
    }
}
