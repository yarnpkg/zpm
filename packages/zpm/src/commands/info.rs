use std::collections::{BTreeMap, BTreeSet};

use clipanion::cli;
use globset::GlobBuilder;
use indexmap::IndexMap;
use zpm_primitives::{DescriptorResolution, IdentGlob, Locator, Reference};
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
    patterns: Vec<IdentGlob>,
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
        Ok(move |locator: &Locator| {
            self.patterns.is_empty() || self.patterns.iter().any(|matcher| matcher.check(&locator.ident))
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
