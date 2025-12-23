use std::collections::{BTreeMap, BTreeSet};

use bincode::{Decode, Encode};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use zpm_primitives::{Descriptor, Ident, Locator, Range, Reference};
use zpm_utils::{System, ToHumanString};

use crate::{
    resolvers::Resolution,
};

#[derive(Clone, Debug, Default, Decode, Encode, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolutionTree {
    pub roots: BTreeSet<Descriptor>,
    pub descriptor_to_locator: BTreeMap<Descriptor, Locator>,
    pub locator_resolutions: BTreeMap<Locator, Resolution>,
    pub optional_builds: BTreeSet<Locator>,
}

#[derive(Default)]
pub struct TreeResolver {
    resolution_tree: ResolutionTree,
    virtual_stack: BTreeMap<Locator, u8>,
    resolution_stack: Vec<Locator>,
    virtual_dependents: BTreeMap<Descriptor, BTreeSet<Locator>>,
    original_workspace_definitions: BTreeMap<Locator, Resolution>,
    peer_dependency_links: BTreeMap<Locator, BTreeMap<Ident, BTreeSet<Locator>>>,
    peer_dependency_dependents: BTreeMap<Locator, BTreeSet<Locator>>,
    virtual_instances: BTreeMap<Locator, BTreeMap<(Ident, Vec<Locator>), Descriptor>>,
    volatile_descriptor: BTreeSet<Descriptor>,
    volatile_locator: BTreeSet<Locator>,
}

impl TreeResolver {
    pub fn with_resolutions(mut self, descriptor_to_locators: &BTreeMap<Descriptor, Locator>, normalized_resolutions: &BTreeMap<Locator, Resolution>) -> Self {
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
                        .find(|variant| variant.requirements.validate_system(&system));

                if let Some(matching_variant) = matching_variant {
                    locator = &matching_variant.locator;
                    resolution = &normalized_resolutions[locator];
                }
            }

            self.resolution_tree.descriptor_to_locator.insert(descriptor.clone(), locator.clone());
            self.resolution_tree.locator_resolutions.insert(locator.clone(), resolution.clone());
            self.resolution_tree.optional_builds.insert(locator.clone());

            if let Reference::WorkspaceIdent(_) = locator.reference {
                self.original_workspace_definitions.insert(locator.clone(), resolution.clone());
            }
        }

        self
    }

    pub fn with_roots(mut self, roots: BTreeSet<Descriptor>) -> Self {
        self.resolution_tree.roots = roots;
        self
    }

    pub fn run(mut self) -> ResolutionTree {
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
            );
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

        self.resolution_tree
    }

    fn resolve_peer_dependencies_impl(&mut self, parent_descriptor: &Descriptor, parent_locator: &Locator, peer_slots: &BTreeMap<Ident, Locator>, top_locator: &Locator, is_optional: bool) {
        if !is_optional {
            self.resolution_tree.optional_builds.remove(parent_locator);
        }

        if !self.volatile_locator.remove(parent_locator) {
            return;
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
            .dependencies.values().cloned().sorted().collect();

        for dependency_descriptor in &parent_dependencies {
            let is_peer_dependency = self.resolution_tree.locator_resolutions
                .get(parent_locator).unwrap()
                .peer_dependencies
                .contains_key(&dependency_descriptor.ident);

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
                .contains(&dependency_descriptor.ident);

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
                );

                continue;
            }

            let virtualized_descriptor = dependency_descriptor
                .virtualized_for(parent_locator);

            let virtualized_locator = dependency_locator
                .virtualized_for(parent_locator);

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

                if peer_descriptor.is_none() && parent_locator.ident == peer_ident {
                    // If the parent isn't installed under an alias we can skip unnecessary steps
                    if parent_descriptor.ident == parent_locator.ident {
                        peer_descriptor = Some(parent_descriptor.clone());
                    } else {
                        let new_peer_descriptor
                            = Descriptor::new(parent_locator.ident.clone(), parent_descriptor.range.clone());

                        self.volatile_descriptor.remove(&new_peer_descriptor);
                        peer_descriptor = Some(new_peer_descriptor);
                    }
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

                // Need to track when a virtual descriptor is set as a dependency in case
                // the descriptor will be consolidated.
                if matches!(peer_descriptor.range, Range::Virtual(_)) {
                    self.virtual_dependents.entry(peer_descriptor.clone()).or_default()
                        .insert(operation.virtualized_locator.clone());
                }

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

                let virtual_instance_resolutions: Vec<_> = self.resolution_tree.locator_resolutions
                    .get(&operation.virtualized_locator).unwrap()
                    .dependencies.values()
                    .filter_map(|d| self.resolution_tree.descriptor_to_locator.get(d).cloned())
                    .sorted()
                    .collect();

                let virtual_instance_hash = (
                    operation.virtualized_descriptor.ident.clone(),
                    virtual_instance_resolutions,
                );

                let other_virtual_instances = self.virtual_instances.entry(operation.physical_locator.clone())
                    .or_default();

                let master_descriptor = other_virtual_instances.entry(virtual_instance_hash)
                    .or_insert_with(|| operation.virtualized_descriptor.clone());

                // Since we're applying multiple pass, we might have already registered
                // ourselves as the "master" descriptor in the previous pass.
                if *master_descriptor == operation.virtualized_descriptor {
                    continue;
                }

                self.resolution_tree.descriptor_to_locator.remove(&operation.virtualized_descriptor);
                self.resolution_tree.locator_resolutions.remove(&operation.virtualized_locator);

                let mut all_dependents: Vec<_> = self.virtual_dependents
                    .entry(operation.virtualized_descriptor.clone()).or_default()
                    .iter().cloned().collect();

                all_dependents.push(parent_locator.clone());

                self.virtual_dependents.remove(&operation.virtualized_descriptor);

                for dependent in all_dependents {
                    if let Some(resolution) = self.resolution_tree.locator_resolutions.get_mut(&dependent) {
                        if resolution.dependencies.get(&operation.virtualized_descriptor.ident).unwrap() != master_descriptor {
                            stable = false;
                        }

                        resolution.dependencies.insert(
                            operation.virtualized_descriptor.ident.clone(),
                            master_descriptor.clone(),
                        );
                    }
                }
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
            );

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

            if !self.resolution_tree.locator_resolutions.contains_key(&operation.virtualized_locator) {
                continue;
            }

            let peer_dependencies = &self.resolution_tree.locator_resolutions
                .get(&operation.virtualized_locator).unwrap()
                .peer_dependencies;

            for peer_ident in peer_dependencies.keys().sorted() {
                let root = operation.next_peer_slots.get(peer_ident)
                    .expect("Expected the peer dependency ident to be listed in the next peer slots");

                self.peer_dependency_links
                    .entry(root.clone())
                    .or_default()
                    .entry(peer_ident.clone())
                    .or_default();
            }

            let virtualized_resolution = self.resolution_tree.locator_resolutions
                .get_mut(&operation.virtualized_locator).unwrap();

            for missing_peer_dependency in &virtualized_resolution.missing_peer_dependencies {
                virtualized_resolution.dependencies.remove(missing_peer_dependency);
            }

            // No need to keep track of the original package after it's been virtualized
            self.resolution_tree.optional_builds.remove(&operation.physical_locator);
        }
    }

    fn resolve_peer_dependencies(&mut self, parent_descriptor: &Descriptor, parent_locator: &Locator, peer_slots: &BTreeMap<Ident, Locator>, top_locator: &Locator, is_optional: bool) {
        if self.resolution_stack.len() > 1000 {
            return;
        }

        self.resolution_stack.push(parent_locator.clone());
        self.resolve_peer_dependencies_impl(parent_descriptor, parent_locator, peer_slots, top_locator, is_optional);
        self.resolution_stack.pop();
    }
}
