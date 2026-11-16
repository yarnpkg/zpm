use std::collections::{BTreeMap, BTreeSet};

use rkyv::Archive;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use zpm_primitives::{Descriptor, Ident, Locator, Range, Reference};
use zpm_utils::{System, ToHumanString};

use crate::{
    error::Error, resolvers::Resolution
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub struct ResolutionTree {
    pub roots: BTreeSet<Descriptor>,
    pub descriptor_to_locator: BTreeMap<Descriptor, Locator>,
    pub locator_resolutions: BTreeMap<Locator, Resolution>,
    pub optional_builds: BTreeSet<Locator>,
}

impl ResolutionTree {
    /// Yields every distinct physical locator in the tree (each virtual is
    /// collapsed to its underlying locator and yielded once). Use this
    /// when a diagnostic or build pass cares about packages-as-they-live-
    /// on-disk rather than per-virtual instances.
    pub fn iter_physical(&self) -> impl Iterator<Item = Locator> {
        let physicals: BTreeSet<Locator> = self.locator_resolutions.keys()
            .map(|locator| locator.physical_locator())
            .collect();

        physicals.into_iter()
    }
}

/// How deep peer dependency resolution may recurse before we give up (same as
/// Berry). Each level of the dependency chain being traversed costs one level
/// (packages that have already been traversed return immediately, so cycles
/// stay well below it unless they keep generating new virtual instances).
pub const MAX_RESOLUTION_DEPTH: usize = 1000;

/// How many of the last traversed packages we print when reaching the limit.
const RESOLUTION_STACK_EXCERPT_SIZE: usize = 5;

#[derive(Default)]
pub struct TreeResolver {
    resolution_tree: ResolutionTree,
    virtual_stack: BTreeMap<Locator, u8>,
    resolution_stack: Vec<Locator>,
    original_workspace_definitions: BTreeMap<Locator, Resolution>,
    peer_dependency_links: BTreeMap<Locator, BTreeMap<Ident, BTreeSet<Locator>>>,
    peer_dependency_dependents: BTreeMap<Locator, BTreeSet<Locator>>,
    virtual_instances: BTreeMap<Locator, BTreeMap<(Vec<Ident>, Vec<Locator>), Descriptor>>,
    volatile_descriptor: BTreeSet<Descriptor>,
    volatile_locator: BTreeSet<Locator>,
}

impl TreeResolver {
    fn package_identity(locator: &Locator) -> Locator {
        let physical_locator = locator.physical_locator();

        match &physical_locator.reference {
            Reference::Registry(params) => {
                Locator::new(params.ident.clone(), physical_locator.reference.clone())
            },

            Reference::PypiRegistry(params) => {
                Locator::new(params.ident.clone(), physical_locator.reference.clone())
            },

            Reference::WorkspaceIdent(params) => {
                Locator::new(params.ident.clone(), physical_locator.reference.clone())
            },

            _ => physical_locator,
        }
    }

    fn package_ident(locator: &Locator) -> Ident {
        match locator.reference.physical_reference() {
            Reference::Registry(params) => params.ident.clone(),
            Reference::PypiRegistry(params) => params.ident.clone(),
            Reference::WorkspaceIdent(params) => params.ident.clone(),
            _ => locator.ident.clone(),
        }
    }

    pub fn with_resolutions(mut self, descriptor_to_locators: &BTreeMap<Descriptor, Locator>, normalized_resolutions: &BTreeMap<Locator, Resolution>) -> Result<Self, Error> {
        self.resolution_tree.descriptor_to_locator.clear();
        self.resolution_tree.locator_resolutions.clear();

        self.original_workspace_definitions.clear();

        let system
            = System::current();

        for (descriptor, mut locator) in descriptor_to_locators.iter().sorted_by_cached_key(|(d, _)| d.ident.clone()) {
            let mut resolution
                = &normalized_resolutions[locator];

            if !resolution.variants.is_empty() {
                let matching_variant
                    = resolution.variants.iter()
                        .map(|variant| &descriptor_to_locators[variant])
                        .map(|locator| &normalized_resolutions[locator])
                        .find(|resolution| resolution.requirements.validate_system(&system));

                match matching_variant {
                    Some(matching_variant) => {
                        locator = &matching_variant.locator;
                        resolution = &normalized_resolutions[locator];
                    },

                    // PyPI packages only get variants for the platforms
                    // they ship artifacts for; one without a variant for
                    // this system (pywin32 on macOS) is only reached through
                    // marker-guarded edges that don't apply here. The
                    // abstract parent stays in the tree so the edges still
                    // resolve, and the venv linker never installs it.
                    None if matches!(locator.reference.physical_reference(), Reference::PypiRegistry(_) | Reference::PypiShorthand(_)) => {},

                    None => {
                        return Err(Error::NoMatchingVariantFound(locator.clone()));
                    },
                }
            }

            self.resolution_tree.descriptor_to_locator.insert(descriptor.clone(), locator.clone());
            self.resolution_tree.locator_resolutions.insert(locator.clone(), resolution.clone());
            self.resolution_tree.optional_builds.insert(locator.clone());

            if let Reference::WorkspaceIdent(_) = locator.reference {
                self.original_workspace_definitions.insert(locator.clone(), resolution.clone());
            }
        }

        Ok(self)
    }

    pub fn with_roots(mut self, roots: BTreeSet<Descriptor>) -> Self {
        self.resolution_tree.roots = roots;
        self
    }

    pub fn run(mut self) -> Result<ResolutionTree, Error> {
        let roots = self.resolution_tree.roots.iter()
            .sorted()
            .cloned()
            .collect_vec();

        self.volatile_descriptor = self.resolution_tree.descriptor_to_locator.keys()
            .cloned()
            .collect();

        self.volatile_locator = self.resolution_tree.locator_resolutions.keys()
            .cloned()
            .collect();

        for root_descriptor in roots {
            let resolution = self.resolution_tree.descriptor_to_locator
                .get(&root_descriptor)
                .cloned().unwrap();

            self.volatile_descriptor.remove(&root_descriptor);
            self.resolve_peer_dependencies(
                &root_descriptor,
                &resolution,
                &BTreeMap::new(),
                &resolution,
                false,
            )?;
        }

        for locator in self.original_workspace_definitions {
            if let Some(resolution) = self.resolution_tree.locator_resolutions.get_mut(&locator.0) {
                resolution.peer_dependencies.clear();
            }
        }

        for volatile_descriptor in self.volatile_descriptor.iter() {
            self.resolution_tree.descriptor_to_locator.remove(volatile_descriptor);
        }

        for volatile_locator in self.volatile_locator.iter() {
            self.resolution_tree.locator_resolutions.remove(volatile_locator);
            self.resolution_tree.optional_builds.remove(volatile_locator);
        }

        Ok(self.resolution_tree)
    }

    fn resolve_peer_dependencies_impl(&mut self, parent_descriptor: &Descriptor, parent_locator: &Locator, peer_slots: &BTreeMap<Ident, Locator>, top_locator: &Locator, is_optional: bool) -> Result<(), Error> {
        if !is_optional {
            self.resolution_tree.optional_builds.remove(parent_locator);
        }

        if !self.volatile_locator.remove(parent_locator) {
            return Ok(());
        }

        struct VirtualOperation {
            physical_locator: Locator,

            virtualized_descriptor: Descriptor,
            virtualized_locator: Locator,

            next_peer_slots: BTreeMap<Ident, Locator>,

            is_optional: bool,
        }

        let mut virtual_operations = vec![];

        let parent_dependencies: Vec<_> = self.resolution_tree.locator_resolutions.get(parent_locator)
            .expect("Expected the parent locator to have a resolution")
            .dependencies.iter()
            .map(|(ident, descriptor)| (ident.clone(), descriptor.clone()))
            .sorted()
            .collect();

        for (dependency_ident, dependency_descriptor) in &parent_dependencies {
            let is_peer_dependency = self.resolution_tree.locator_resolutions
                .get(parent_locator).unwrap()
                .peer_dependencies
                .contains_key(dependency_ident);

            if is_peer_dependency && parent_locator != top_locator {
                continue;
            }

            if matches!(dependency_descriptor.range, Range::Virtual(_)) {
                panic!("Virtual packages shouldn't be encountered when virtualizing a branch");
            }

            self.volatile_descriptor.remove(dependency_descriptor);

            let is_optional = is_optional || self.resolution_tree.locator_resolutions
                .get(parent_locator).unwrap()
                .optional_dependencies
                .contains(dependency_ident);

            let dependency_locator = self.resolution_tree.descriptor_to_locator.get(dependency_descriptor)
                .unwrap_or_else(|| panic!("Expected a locator to be found for {}", dependency_descriptor.to_print_string()));

            let pkg = self.original_workspace_definitions.get(dependency_locator)
                .or(self.resolution_tree.locator_resolutions.get(dependency_locator))
                .unwrap();

            let has_peer_dependencies = !pkg
                .peer_dependencies.is_empty();

            if !has_peer_dependencies {
                self.resolve_peer_dependencies(
                    dependency_descriptor,
                    &dependency_locator.clone(),
                    peer_slots,
                    top_locator,
                    is_optional,
                )?;

                continue;
            }

            let parent_hash
                = zpm_utils::Hash64::from_string(parent_locator);

            let virtualized_descriptor = dependency_descriptor
                .virtualized_with_hash(parent_hash.clone());

            let virtualized_locator = dependency_locator
                .virtualized_with_hash(parent_hash);

            let mut virtualized_resolution = pkg
                .clone();

            virtualized_resolution.locator = virtualized_locator.clone();

            // We need to add it so it can be removed from within the nested resolve_peer_dependencies_impl call
            self.volatile_locator.insert(virtualized_locator.clone());

            self.resolution_tree.locator_resolutions.insert(
                virtualized_locator.clone(),
                virtualized_resolution.clone(),
            );

            virtual_operations.push(VirtualOperation {
                physical_locator: dependency_locator.clone(),

                virtualized_descriptor: virtualized_descriptor.clone(),
                virtualized_locator: virtualized_locator.clone(),

                next_peer_slots: BTreeMap::new(),

                is_optional,
            });

            self.resolution_tree.descriptor_to_locator.insert(
                virtualized_descriptor,
                virtualized_locator,
            );
        }

        for operation in &virtual_operations {
            let parent_resolution = self.resolution_tree.locator_resolutions.get_mut(parent_locator)
                .unwrap();

            parent_resolution.dependencies.insert(
                operation.virtualized_descriptor.ident.clone(),
                operation.virtualized_descriptor.clone(),
            );
        }

        for operation in &mut virtual_operations {
            let peer_dependencies: Vec<_> = self.resolution_tree.locator_resolutions
                .get(&operation.physical_locator).unwrap()
                .peer_dependencies
                .keys().cloned().sorted().collect();

            let mut missing_peer_dependencies = BTreeSet::new();
            let mut peer_dependencies_to_remove = vec![];

            for peer_ident in peer_dependencies {
                let mut peer_descriptor = self.resolution_tree.locator_resolutions
                    .get(parent_locator).unwrap()
                    .dependencies
                    .get(&peer_ident).cloned();

                if peer_descriptor.is_none() && Self::package_ident(parent_locator) == peer_ident {
                    peer_descriptor = Some(parent_descriptor.clone());
                }

                let is_provided_by_parent = match &peer_descriptor {
                    Some(descriptor) => !matches!(descriptor.range, Range::MissingPeerDependency),
                    None => false,
                };

                // If the peerRequest isn't provided by the parent then fall back to dependencies
                if !is_provided_by_parent {
                    let has_dependency_fallback = self.resolution_tree.locator_resolutions
                        .get(&operation.virtualized_locator).unwrap()
                        .dependencies
                        .contains_key(&peer_ident);

                    if has_dependency_fallback {
                        peer_dependencies_to_remove.push(peer_ident.clone());
                        continue;
                    }
                }

                let peer_descriptor = peer_descriptor.unwrap_or_else(|| Descriptor::new(
                    peer_ident.clone(),
                    Range::MissingPeerDependency,
                ));

                self.resolution_tree.locator_resolutions
                    .get_mut(&operation.virtualized_locator).unwrap()
                    .dependencies
                    .insert(peer_ident.clone(), peer_descriptor.clone());

                if matches!(peer_descriptor.range, Range::MissingPeerDependency) {
                    missing_peer_dependencies.insert(peer_ident.clone());
                }

                operation.next_peer_slots.insert(
                    peer_ident.clone(),
                    peer_slots.get(&peer_ident).unwrap_or(&operation.virtualized_locator).clone(),
                );
            }

            let virtualized_resolution = self.resolution_tree.locator_resolutions
                .get_mut(&operation.virtualized_locator).unwrap();

            virtualized_resolution.missing_peer_dependencies
                = missing_peer_dependencies;

            for peer_ident in peer_dependencies_to_remove {
                virtualized_resolution.peer_dependencies.remove(&peer_ident);
            }
        }

        let mut stable;
        loop {
            stable = true;

            for operation in &virtual_operations {
                if !self.resolution_tree.locator_resolutions.contains_key(&operation.virtualized_locator) {
                    continue;
                }

                let virtualized_resolution = self.resolution_tree.locator_resolutions
                    .get(&operation.virtualized_locator).unwrap();

                let virtual_instance_peers = virtualized_resolution
                    .peer_dependencies
                    .keys()
                    .cloned()
                    .sorted()
                    .collect();

                let virtual_instance_resolutions: Vec<_> = virtualized_resolution
                    .dependencies.values()
                    .filter_map(|d| self.resolution_tree.descriptor_to_locator.get(d).cloned())
                    .sorted()
                    .collect();

                let virtual_instance_hash = (virtual_instance_peers, virtual_instance_resolutions);

                let package_identity = Self::package_identity(&operation.physical_locator);

                let other_virtual_instances = self.virtual_instances.entry(package_identity)
                    .or_default();

                let master_descriptor = other_virtual_instances.entry(virtual_instance_hash)
                    .or_insert_with(|| operation.virtualized_descriptor.clone());

                // Since we're applying multiple pass, we might have already registered
                // ourselves as the "master" descriptor in the previous pass.
                if *master_descriptor == operation.virtualized_descriptor {
                    continue;
                }

                let master_locator = self.resolution_tree.descriptor_to_locator.get(master_descriptor)
                    .expect("Expected the virtual instance master to have a locator")
                    .clone();

                self.resolution_tree.descriptor_to_locator.insert(
                    operation.virtualized_descriptor.clone(),
                    master_locator,
                );

                self.resolution_tree.locator_resolutions.remove(&operation.virtualized_locator);
                stable = false;
            }

            if stable {
                break;
            }
        }

        for operation in &mut virtual_operations {
            if !self.resolution_tree.locator_resolutions.contains_key(&operation.virtualized_locator) {
                continue;
            }

            // The stack overflow is checked against two level because a workspace
            // may have a dev dependency on another workspace that lists the first
            // one as a regular dependency. In this case the loop will break so we
            // don't need to throw an exception.
            let stack_depth = self.virtual_stack.get(&operation.physical_locator)
                .cloned()
                .unwrap_or_default();

            if stack_depth >= 2 {
                // Throw error
                continue;
            }

            self.virtual_stack.insert(operation.physical_locator.clone(), stack_depth + 1);

            self.resolve_peer_dependencies(
                &operation.virtualized_descriptor,
                &operation.virtualized_locator,
                &operation.next_peer_slots,
                top_locator,
                operation.is_optional,
            )?;

            self.virtual_stack.insert(operation.physical_locator.clone(), stack_depth);
        }

        for operation in &mut virtual_operations {
            let parent_resolution = self.resolution_tree.locator_resolutions.get(parent_locator)
                .unwrap();

            // Regardless of whether the initial virtualized package got deduped
            // or not, we now register that *this* package is now a dependent on
            // whatever its peer dependencies have been resolved to. We'll later
            // use this information to generate warnings.
            let final_descriptor = parent_resolution.dependencies.get(&operation.virtualized_descriptor.ident).cloned()
                .expect("Expected the peer dependency to have been turned into a dependency");

            let final_resolution = self.resolution_tree.descriptor_to_locator.get(&final_descriptor).cloned()
                .expect("Expected the final resolution to be present");

            self.peer_dependency_dependents
                .entry(final_resolution.clone()).or_default()
                .insert(parent_locator.clone());

            let peer_dependencies = &self.resolution_tree.locator_resolutions
                .get(&final_resolution).unwrap()
                .peer_dependencies;

            for peer_ident in peer_dependencies.keys().sorted() {
                let root = operation.next_peer_slots.get(peer_ident)
                    .expect("Expected the peer dependency ident to be listed in the next peer slots");

                let root = if root == &operation.virtualized_locator {
                    &final_resolution
                } else {
                    root
                };

                self.peer_dependency_links
                    .entry(root.clone())
                    .or_default()
                    .entry(peer_ident.clone())
                    .or_default();
            }

            let virtualized_resolution = self.resolution_tree.locator_resolutions
                .get_mut(&final_resolution).unwrap();

            for missing_peer_dependency in &virtualized_resolution.missing_peer_dependencies {
                virtualized_resolution.dependencies.remove(missing_peer_dependency);
            }

            // No need to keep track of the original package after it's been virtualized
            self.resolution_tree.optional_builds.remove(&operation.physical_locator);
        }

        Ok(())
    }

    fn resolve_peer_dependencies(&mut self, parent_descriptor: &Descriptor, parent_locator: &Locator, peer_slots: &BTreeMap<Ident, Locator>, top_locator: &Locator, is_optional: bool) -> Result<(), Error> {
        // Silently stopping here would leave the rest of the chain without
        // its peer dependencies; the install must fail instead
        if self.resolution_stack.len() > MAX_RESOLUTION_DEPTH {
            let excerpt_start
                = self.resolution_stack.len().saturating_sub(RESOLUTION_STACK_EXCERPT_SIZE - 1);

            let mut stack_excerpt
                = self.resolution_stack[excerpt_start..].to_vec();

            stack_excerpt.push(parent_locator.clone());

            return Err(Error::StackOverflowResolution {
                max_depth: MAX_RESOLUTION_DEPTH,
                stack_excerpt,
            });
        }

        self.resolution_stack.push(parent_locator.clone());
        let result
            = self.resolve_peer_dependencies_impl(parent_descriptor, parent_locator, peer_slots, top_locator, is_optional);
        self.resolution_stack.pop();

        result
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use zpm_primitives::{Descriptor, Ident, Locator, PeerRange};
    use zpm_utils::FromFileString;

    use crate::{
        error::Error,
        resolvers::Resolution,
    };

    use super::{ResolutionTree, TreeResolver, MAX_RESOLUTION_DEPTH};

    /// Each package is `<name>@npm:1.0.0`, requested through the
    /// `<name>@npm:1.0.0` descriptor; `root` is the only root.
    fn resolve(packages: &[(String, Vec<String>, Vec<String>)]) -> Result<ResolutionTree, Error> {
        let mut descriptor_to_locator
            = BTreeMap::new();
        let mut resolutions
            = BTreeMap::new();

        for (name, dependencies, peer_dependencies) in packages {
            let locator
                = Locator::from_file_string(&format!("{}@npm:1.0.0", name)).unwrap();

            let mut resolution
                = Resolution::new_empty(locator.clone(), Default::default());

            for dependency in dependencies {
                resolution.dependencies.insert(
                    Ident::from_file_string(dependency).unwrap(),
                    Descriptor::from_file_string(&format!("{}@npm:1.0.0", dependency)).unwrap(),
                );
            }

            for peer_dependency in peer_dependencies {
                resolution.peer_dependencies.insert(
                    Ident::from_file_string(peer_dependency).unwrap(),
                    PeerRange::from_file_string("*").unwrap(),
                );
            }

            descriptor_to_locator.insert(Descriptor::from_file_string(&format!("{}@npm:1.0.0", name)).unwrap(), locator.clone());
            resolutions.insert(locator, resolution);
        }

        let roots
            = BTreeSet::from([Descriptor::from_file_string("root@npm:1.0.0").unwrap()]);

        // The resolver recurses once per level, which needs more than the
        // default stack of the test threads in debug builds
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                TreeResolver::default()
                    .with_resolutions(&descriptor_to_locator, &resolutions)?
                    .with_roots(roots)
                    .run()
            })
            .unwrap()
            .join()
            .unwrap()
    }

    fn package(name: &str, dependencies: &[&str], peer_dependencies: &[&str]) -> (String, Vec<String>, Vec<String>) {
        (
            name.to_string(),
            dependencies.iter().map(|s| s.to_string()).collect(),
            peer_dependencies.iter().map(|s| s.to_string()).collect(),
        )
    }

    /// root -> pkg-0 -> pkg-1 -> ... -> pkg-<length - 1>, each of them
    /// having a peer dependency on `peer` (provided by root)
    fn chain(length: usize) -> Vec<(String, Vec<String>, Vec<String>)> {
        let mut packages
            = vec![package("root", &["pkg-0", "peer"], &[]), package("peer", &[], &[])];

        for t in 0..length {
            let next
                = format!("pkg-{}", t + 1);

            let dependencies: &[&str]
                = if t + 1 < length {&[&next]} else {&[]};

            packages.push(package(&format!("pkg-{}", t), dependencies, &["peer"]));
        }

        packages
    }

    fn virtual_instances_of<'a>(tree: &'a ResolutionTree, name: &str) -> Vec<&'a Resolution> {
        tree.locator_resolutions.iter()
            .filter(|(locator, _)| locator.reference.is_virtual_reference() && locator.physical_locator().ident.as_str() == name)
            .map(|(_, resolution)| resolution)
            .collect()
    }

    #[test]
    fn it_should_resolve_peer_dependencies_at_the_bottom_of_a_chain_below_the_limit() {
        let length
            = MAX_RESOLUTION_DEPTH;

        let tree
            = resolve(&chain(length)).unwrap();

        let bottom
            = virtual_instances_of(&tree, &format!("pkg-{}", length - 1));

        assert_eq!(bottom.len(), 1);
        assert_eq!(
            tree.descriptor_to_locator[&bottom[0].dependencies[&Ident::from_file_string("peer").unwrap()]],
            Locator::from_file_string("peer@npm:1.0.0").unwrap(),
        );
    }

    #[test]
    fn it_should_report_an_error_when_the_chain_exceeds_the_limit() {
        let result
            = resolve(&chain(MAX_RESOLUTION_DEPTH + 100));

        match result {
            Err(Error::StackOverflowResolution {max_depth, stack_excerpt}) => {
                assert_eq!(max_depth, MAX_RESOLUTION_DEPTH);
                assert_eq!(stack_excerpt.last().unwrap().physical_locator(), Locator::from_file_string(&format!("pkg-{}@npm:1.0.0", MAX_RESOLUTION_DEPTH)).unwrap());
            },

            Err(error)
                => panic!("Unexpected error: {}", error),

            Ok(_)
                => panic!("Expected the resolution to fail"),
        }
    }

    #[test]
    fn it_should_resolve_dependency_cycles_without_peer_dependencies() {
        resolve(&[
            package("root", &["a"], &[]),
            package("a", &["b"], &[]),
            package("b", &["a"], &[]),
        ]).unwrap();
    }

    #[test]
    fn it_should_resolve_dependency_cycles_through_peer_dependencies() {
        let tree
            = resolve(&[
                package("root", &["a", "peer"], &[]),
                package("peer", &[], &[]),
                package("a", &["b"], &["peer"]),
                package("b", &["a"], &["peer"]),
            ]).unwrap();

        let a
            = virtual_instances_of(&tree, "a");

        assert!(!a.is_empty());
        for resolution in a {
            assert_eq!(
                tree.descriptor_to_locator[&resolution.dependencies[&Ident::from_file_string("peer").unwrap()]],
                Locator::from_file_string("peer@npm:1.0.0").unwrap(),
            );
        }
    }
}
