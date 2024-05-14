use std::collections::{HashMap, HashSet};

use itertools::Itertools;

use crate::{primitives::{Descriptor, Ident, Locator, Range, Reference}, resolver::Resolution, serialize::Serialized};

#[derive(Clone, Debug)]
pub struct TreeResolver {
    pub descriptor_to_locator: HashMap<Descriptor, Locator>,
    pub locator_resolutions: HashMap<Locator, Resolution>,

    accessible_locators: HashSet<Locator>,
    virtual_stack: HashMap<Locator, u8>,
    resolution_stack: Vec<Locator>,
    optional_builds: HashSet<Locator>,
    virtual_dependents: HashMap<Descriptor, HashSet<Locator>>,
    original_workspace_definitions: HashMap<Locator, Resolution>,
    peer_dependency_links: HashMap<Locator, HashMap<Ident, HashSet<Locator>>>,
    peer_dependency_dependents: HashMap<Locator, HashSet<Locator>>,
    virtual_instances: HashMap<Locator, HashMap<(Ident, Vec<Locator>), Descriptor>>,
    volatile_descriptor: HashSet<Descriptor>,
}

impl TreeResolver {
    pub fn new(resolutions: HashMap<Descriptor, Resolution>, root_descriptors: Vec<Descriptor>) -> TreeResolver {
        let mut resolver = TreeResolver {
            descriptor_to_locator: HashMap::new(),
            locator_resolutions: HashMap::new(),

            accessible_locators: HashSet::new(),
            virtual_stack: HashMap::new(),
            resolution_stack: vec![],
            optional_builds: HashSet::new(),
            virtual_dependents: HashMap::new(),
            original_workspace_definitions: HashMap::new(),
            peer_dependency_links: HashMap::new(),
            peer_dependency_dependents: HashMap::new(),
            virtual_instances: HashMap::new(),
            volatile_descriptor: HashSet::new(),
        };

        for (descriptor, resolution) in resolutions.iter().sorted_by_cached_key(|(d, _)| d.ident.clone()) {
            resolver.descriptor_to_locator.insert(descriptor.clone(), resolution.locator.clone());
            resolver.locator_resolutions.insert(resolution.locator.clone(), resolution.clone());

            if let Reference::Workspace(_) = resolution.locator.reference {
                resolver.original_workspace_definitions.insert(resolution.locator.clone(), resolution.clone());
            }
        }

        for root_descriptor in root_descriptors.iter().sorted() {
            let resolution = resolver.descriptor_to_locator
                .get(&root_descriptor)
                .cloned().unwrap();

            resolver.volatile_descriptor.remove(&root_descriptor);
            resolver.resolve_peer_dependencies(
                &root_descriptor,
                resolution.clone(),
                &HashMap::new(),
                &resolution,
                false,
            );
        }

        resolver
    }

    fn resolve_peer_dependencies_impl(&mut self, parent_descriptor: &Descriptor, parent_locator: Locator, peer_slots: &HashMap<Ident, Locator>, top_locator: &Locator, is_optional: bool) {
        if !is_optional {
            self.optional_builds.remove(&parent_locator);
        }

        if !self.accessible_locators.insert(parent_locator.clone()) {
            return;
        }

        struct VirtualOperation {
            physical_locator: Locator,

            virtualized_descriptor: Descriptor,
            virtualized_locator: Locator,

            missing_peer_dependencies: HashSet<Ident>,
            next_peer_slots: HashMap<Ident, Locator>,

            is_optional: bool,
        }

        let mut virtual_operations = vec![];

        let parent_dependencies: Vec<_> = self.locator_resolutions.get(&parent_locator)
            .expect(format!("Expected locator resolution for {:?}", parent_locator).as_str())
            .dependencies.values().cloned().sorted().collect();

        for dependency_descriptor in &parent_dependencies {
            let is_peer_dependency = self.locator_resolutions
                .get(&parent_locator).unwrap()
                .peer_dependencies
                .contains_key(&dependency_descriptor.ident);

            if is_peer_dependency && &parent_locator != top_locator {
                continue;
            }

            if let Range::Virtual(_, _) = dependency_descriptor.range {
                panic!("Virtual packages shouldn't be encountered when virtualizing a branch");
            }

            self.volatile_descriptor.remove(&dependency_descriptor);

            let is_optional = is_optional || self.locator_resolutions
                .get(&parent_locator).unwrap()
                .optional_dependencies
                .contains(&dependency_descriptor.ident);

            let dependency_locator = self.descriptor_to_locator.get(&dependency_descriptor)
                .expect(format!("Expected locator for descriptor: {:?}; in {:?}", dependency_descriptor, &self.descriptor_to_locator).as_str());

            let pkg = self.original_workspace_definitions.get(dependency_locator)
                .or(self.locator_resolutions.get(dependency_locator))
                .unwrap();

            let has_peer_dependencies = pkg
                .peer_dependencies
                .len() > 0;

            if !has_peer_dependencies {
                self.resolve_peer_dependencies(
                    &dependency_descriptor,
                    parent_locator.clone(),
                    peer_slots,
                    top_locator,
                    is_optional,
                );

                continue;
            }

            let virtualized_descriptor = dependency_descriptor
                .virtualized_for(&parent_locator);

            let virtualized_locator = dependency_locator
                .virtualized_for(&parent_locator);

            let mut virtualized_resolution = pkg
                .clone();

            virtualized_resolution.locator = virtualized_locator.clone();

            self.locator_resolutions.insert(
                virtualized_locator.clone(),
                virtualized_resolution.clone(),
            );

            virtual_operations.push(VirtualOperation {
                physical_locator: dependency_locator.clone(),

                virtualized_descriptor: virtualized_descriptor.clone(),
                virtualized_locator: virtualized_locator.clone(),

                missing_peer_dependencies: HashSet::new(),
                next_peer_slots: HashMap::new(),

                is_optional,
            });

            self.descriptor_to_locator.insert(
                virtualized_descriptor,
                virtualized_locator,
            );
        }

        for operation in &virtual_operations {
            let parent_resolution = self.locator_resolutions.get_mut(&parent_locator)
                .unwrap();

            parent_resolution.dependencies.insert(
                operation.virtualized_descriptor.ident.clone(),
                operation.virtualized_descriptor.clone(),
            );
        }

        for operation in &mut virtual_operations {
            let peer_dependencies: Vec<_> = self.locator_resolutions
                .get(&operation.physical_locator).unwrap()
                .peer_dependencies
                .keys().cloned().sorted().collect();

            let mut peer_dependencies_to_remove = vec![];

            for peer_ident in peer_dependencies {
                let mut peer_descriptor = self.locator_resolutions
                    .get(&parent_locator).unwrap()
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
                    Some(descriptor) => descriptor.range != Range::MissingPeerDependency,
                    None => false,
                };

                // If the peerRequest isn't provided by the parent then fall back to dependencies
                if !is_provided_by_parent {
                    let has_dependency_fallback = self.locator_resolutions
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

                self.locator_resolutions
                    .get_mut(&operation.virtualized_locator).unwrap()
                    .dependencies
                    .insert(peer_ident.clone(), peer_descriptor.clone());

                // Need to track when a virtual descriptor is set as a dependency in case
                // the descriptor will be consolidated.
                if let Range::Virtual(_, _) = peer_descriptor.range {
                    self.virtual_dependents.entry(peer_descriptor.clone()).or_default()
                        .insert(operation.virtualized_locator.clone());
                }

                if peer_descriptor.range == Range::MissingPeerDependency {
                    operation.missing_peer_dependencies.insert(peer_descriptor.ident.clone());
                }

                operation.next_peer_slots.insert(
                    peer_ident.clone(),
                    peer_slots.get(&peer_ident).unwrap_or(&operation.virtualized_locator).clone(),
                );
            }

            let virtualized_peer_dependencies = &mut self.locator_resolutions
                .get_mut(&operation.virtualized_locator).unwrap()
                .peer_dependencies;

            for peer_ident in peer_dependencies_to_remove {
                virtualized_peer_dependencies.remove(&peer_ident);
            }
        }

        let mut stable;
        loop {
            stable = true;

            for operation in &virtual_operations {
                if !self.locator_resolutions.contains_key(&operation.virtualized_locator) {
                    continue;
                }

                let virtual_instance_resolutions: Vec<_> = self.locator_resolutions
                    .get(&operation.virtualized_locator).unwrap()
                    .dependencies.values()
                    .filter_map(|d| self.descriptor_to_locator.get(d).cloned())
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

                self.descriptor_to_locator.remove(&operation.virtualized_descriptor);
                self.locator_resolutions.remove(&operation.virtualized_locator);
                self.accessible_locators.remove(&operation.virtualized_locator);

                let mut all_dependents: Vec<_> = self.virtual_dependents
                    .entry(operation.virtualized_descriptor.clone()).or_default()
                    .iter().cloned().collect();

                all_dependents.push(parent_locator.clone());

                self.virtual_dependents.remove(&operation.virtualized_descriptor);

                for dependent in all_dependents {
                    if let Some(resolution) = self.locator_resolutions.get_mut(&dependent) {
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
            if !self.locator_resolutions.contains_key(&operation.virtualized_locator) {
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
                operation.virtualized_locator.clone(),
                &operation.next_peer_slots,
                top_locator,
                operation.is_optional,
            );

            self.virtual_stack.insert(operation.physical_locator.clone(), stack_depth);
        }

        for operation in &mut virtual_operations {
            let parent_resolution = self.locator_resolutions.get(&parent_locator)
                .unwrap();

            // Regardless of whether the initial virtualized package got deduped
            // or not, we now register that *this* package is now a dependent on
            // whatever its peer dependencies have been resolved to. We'll later
            // use this information to generate warnings.
            let final_descriptor = parent_resolution.dependencies.get(&operation.virtualized_descriptor.ident).cloned()
                .expect("Expected the peer dependency to have been turned into a dependency");

            let final_resolution = self.descriptor_to_locator.get(&final_descriptor).cloned()
                .expect(format!("Expected the peer dependency to have been resolved to a locator: {:?}", final_descriptor).as_str());
            
            self.peer_dependency_dependents
                .entry(final_resolution.clone()).or_default()
                .insert(parent_locator.clone());

            if !self.locator_resolutions.contains_key(&operation.virtualized_locator) {
                continue;
            }

            let peer_dependencies = &self.locator_resolutions
                .get(&operation.virtualized_locator).unwrap()
                .peer_dependencies;

            for peer_ident in peer_dependencies.keys().sorted() {
                let root = operation.next_peer_slots.get(peer_ident)
                    .expect("Expected the peer dependency ident to be listed in the next peer slots");

                self.peer_dependency_links
                    .entry(root.clone())
                    .or_insert(HashMap::new())
                    .entry(peer_ident.clone())
                    .or_insert(HashSet::new());
            }

            let virtualized_dependencies = &mut self.locator_resolutions
                .get_mut(&operation.virtualized_locator).unwrap()
                .dependencies;

            for missing_peer_dependency in &operation.missing_peer_dependencies {
                virtualized_dependencies.remove(missing_peer_dependency);
            }
        }
    }

    fn resolve_peer_dependencies(&mut self, parent_descriptor: &Descriptor, parent_locator: Locator, peer_slots: &HashMap<Ident, Locator>, top_locator: &Locator, is_optional: bool) {
        if self.resolution_stack.len() > 1000 {
            return;
        }

        println!("Resolving peer dependencies for {}", parent_locator.to_string());

        self.resolution_stack.push(parent_locator.clone());
        self.resolve_peer_dependencies_impl(parent_descriptor, parent_locator, peer_slots, top_locator, is_optional);
        self.resolution_stack.pop();
    }
}
