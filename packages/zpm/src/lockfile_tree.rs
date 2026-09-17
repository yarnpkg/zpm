use std::collections::{BTreeMap, BTreeSet, HashMap};

use itertools::Itertools;
use zpm_primitives::{Descriptor, Ident, Locator, WorkspaceIdentReference};
use zpm_utils::{Hash64, Hash64Writer, ToFileString, scc_tarjan_pearce_core};

use crate::{
    install::{DependencyNormalizer, normalize_resolutions_with},
    lockfile::{Lockfile, hash_workspace_dependencies},
    project::Project,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TreeNode<'a> {
    Workspace(&'a Ident),
    Package(&'a Locator),
}

impl ToFileString for TreeNode<'_> {
    fn to_file_string(&self) -> String {
        match self {
            TreeNode::Workspace(ident) => Locator::new((*ident).clone(), WorkspaceIdentReference {ident: (*ident).clone()}.into()).to_file_string(),
            TreeNode::Package(locator) => locator.to_file_string(),
        }
    }
}

/**
 * Reconstructs the dependency tree described by a lockfile, without having
 * to run the resolution. Everything comes from the lockfile except for the
 * workspaces: their dependencies aren't persisted (only their hash is), so
 * whoever walks the tree is expected to pull them from the project after
 * having checked that the hashes match.
 */
pub struct LockfileTree<'a> {
    project: &'a Project,
    lockfile: &'a Lockfile,
    normalizer: DependencyNormalizer<'a>,
}

impl<'a> LockfileTree<'a> {
    pub fn new(project: &'a Project, lockfile: &'a Lockfile) -> Self {
        let normalizer
            = DependencyNormalizer::from_lockfile(lockfile, project.root_workspace().locator());

        Self {
            project,
            lockfile,
            normalizer,
        }
    }

    pub fn resolve(&self, descriptor: &Descriptor) -> Option<TreeNode<'a>> {
        if let Some(locator) = self.lockfile.recorded_resolution(descriptor) {
            return Some(TreeNode::Package(locator));
        }

        // Workspaces are never stored in the lockfile, regardless of the
        // type of range that led to them.
        let workspace
            = self.project.try_workspace_by_descriptor(descriptor).ok().flatten()?;

        Some(TreeNode::Workspace(&workspace.name))
    }

    /**
     * Returns the descriptors a package depends on. The lockfile stores the
     * dependencies as they are declared by the packages, so we need to apply
     * the same transformations as the ones that got applied by the install.
     */
    pub fn package_dependencies(&self, locator: &Locator) -> Option<Vec<Descriptor>> {
        let entry
            = self.lockfile.recorded_entry(locator)?;

        let (dependencies, _)
            = normalize_resolutions_with(&self.normalizer, &entry.resolution).ok()?;

        let dependencies = dependencies.into_values()
            .chain(entry.resolution.variants.iter().cloned())
            .collect();

        Some(dependencies)
    }
}

/**
 * Large projects have many more edges than they have nodes (their workspaces
 * tend to all depend on the same packages), so we refer to the nodes through
 * sequential ids to keep the cost of each edge as small as possible.
 */
#[derive(Default)]
struct NodeTable<'a> {
    ids: HashMap<TreeNode<'a>, usize>,
    nodes: Vec<TreeNode<'a>>,
}

impl<'a> NodeTable<'a> {
    /// Returns the id of the node, along with whether it's the first time we see it.
    fn register(&mut self, node: TreeNode<'a>) -> (usize, bool) {
        if let Some(id) = self.ids.get(&node) {
            return (*id, false);
        }

        let id
            = self.nodes.len();

        self.ids.insert(node, id);
        self.nodes.push(node);

        (id, true)
    }
}

struct TreeComparison<'a> {
    base: &'a Lockfile,
    current: &'a Lockfile,

    base_tree: LockfileTree<'a>,
    current_tree: LockfileTree<'a>,

    // When both lockfiles were generated with the same rules, identical
    // entries are guaranteed to yield identical dependencies; it saves us
    // from having to normalize the same dependencies twice.
    same_rules: bool,

    table: NodeTable<'a>,
    dependents: Vec<Vec<usize>>,

    // What the descriptors resolve to, provided both lockfiles agree on it
    resolutions: HashMap<Descriptor, Option<usize>>,

    dirty: Vec<usize>,
    queue: Vec<(usize, Vec<Descriptor>)>,
}

impl<'a> TreeComparison<'a> {
    fn new(project: &'a Project, base: &'a Lockfile, current: &'a Lockfile) -> Self {
        let same_rules
            = base.project.catalogs == current.project.catalogs
                && base.project.dependency_overrides == current.project.dependency_overrides
                && base.project.package_extensions == current.project.package_extensions;

        Self {
            base,
            current,

            base_tree: LockfileTree::new(project, base),
            current_tree: LockfileTree::new(project, current),

            same_rules,

            table: NodeTable::default(),
            dependents: Vec::new(),

            resolutions: HashMap::new(),

            dirty: Vec::new(),
            queue: Vec::new(),
        }
    }

    fn register(&mut self, node: TreeNode<'a>) -> usize {
        let (id, is_new)
            = self.table.register(node);

        if is_new {
            self.dependents.push(Vec::new());

            // The workspaces are walked by whoever drives the comparison,
            // since their dependencies can't be found in the lockfiles.
            if let TreeNode::Package(locator) = node {
                self.schedule(id, locator);
            }
        }

        id
    }

    fn schedule(&mut self, id: usize, locator: &'a Locator) {
        let has_same_entry = self.same_rules
            && self.base.recorded_entry(locator).map(|entry| &entry.resolution) == self.current.recorded_entry(locator).map(|entry| &entry.resolution);

        let dependencies = self.current_tree.package_dependencies(locator)
            .filter(|dependencies| has_same_entry || self.base_tree.package_dependencies(locator).as_ref() == Some(dependencies));

        match dependencies {
            Some(dependencies) => self.queue.push((id, dependencies)),
            None => self.dirty.push(id),
        }
    }

    fn resolve(&mut self, descriptor: &Descriptor) -> Option<usize> {
        if let Some(resolution) = self.resolutions.get(descriptor) {
            return *resolution;
        }

        let node = self.current_tree.resolve(descriptor)
            .filter(|node| self.base_tree.resolve(descriptor).as_ref() == Some(node));

        let resolution = match node {
            Some(node) => Some(self.register(node)),
            None => None,
        };

        self.resolutions.insert(descriptor.clone(), resolution);
        resolution
    }

    fn visit<'b>(&mut self, id: usize, dependencies: impl IntoIterator<Item = &'b Descriptor>) {
        for descriptor in dependencies {
            let Some(dependency) = self.resolve(descriptor) else {
                // No need to go further; we know this node changed
                self.dirty.push(id);
                break;
            };

            self.dependents[dependency].push(id);
        }
    }
}

/**
 * Returns the workspaces whose dependency tree isn't the same in the two
 * lockfiles (typically the one from the working tree and the one from a base
 * commit).
 *
 * The workspace hashes stored in the lockfiles let us find the workspaces
 * whose own dependencies changed without having to walk anything. The other
 * workspaces have the same dependencies on both sides, so we walk the two
 * trees side by side until we find a descriptor that resolves differently.
 */
pub fn find_changed_workspaces(project: &Project, base: &Lockfile, current: &Lockfile) -> BTreeSet<Ident> {
    let islands
        = workspace_islands(project);
    let workspace_dependencies
        = project.workspace_dependencies();

    let mut comparison
        = TreeComparison::new(project, base, current);

    // We need all workspaces to be registered before we start walking, so
    // that they aren't mistaken for packages when found as dependencies.
    for ident in workspace_dependencies.keys() {
        comparison.register(TreeNode::Workspace(ident));
    }

    for (ident, dependencies) in &workspace_dependencies {
        let id
            = comparison.register(TreeNode::Workspace(ident));

        // The lockfile from the working tree may be stale, so we also check
        // that its hash matches the dependencies we're about to walk.
        let dependencies = dependencies.as_ref().ok().filter(|dependencies| {
            let hash
                = hash_workspace_dependencies(dependencies);

            base.project.workspaces.get(ident) == Some(&hash)
                && current.project.workspaces.get(ident) == Some(&hash)
        });

        let Some(dependencies) = dependencies else {
            comparison.dirty.push(id);
            continue;
        };

        // Islands have their own resolution table, in which a package always
        // resolves to the same version regardless of who depends on it. We
        // compare those tables as a whole rather than walk them; the only
        // dependencies we need to track are those on other workspaces.
        if let Some(island_id) = islands.get(ident) {
            if base.islands.get(island_id) != current.islands.get(island_id) {
                comparison.dirty.push(id);
                continue;
            }

            for descriptor in dependencies.values() {
                if let Some(workspace) = project.try_workspace_by_descriptor(descriptor).ok().flatten() {
                    let dependency
                        = comparison.register(TreeNode::Workspace(&workspace.name));

                    comparison.dependents[dependency].push(id);
                }
            }

            continue;
        }

        comparison.visit(id, dependencies.values());
    }

    while let Some((id, dependencies)) = comparison.queue.pop() {
        comparison.visit(id, &dependencies);
    }

    // A node is affected as soon as anything it depends on is, no matter how deep
    let mut changed_workspaces
        = BTreeSet::new();
    let mut affected
        = vec![false; comparison.table.nodes.len()];

    while let Some(id) = comparison.dirty.pop() {
        if std::mem::replace(&mut affected[id], true) {
            continue;
        }

        comparison.dirty.extend(&comparison.dependents[id]);

        if let TreeNode::Workspace(ident) = comparison.table.nodes[id] {
            changed_workspaces.insert(ident.clone());
        }
    }

    changed_workspaces
}

/**
 * Computes for each workspace a hash covering its whole dependency tree, as
 * described by the lockfile.
 */
pub fn compute_workspace_tree_hashes(project: &Project, lockfile: &Lockfile) -> BTreeMap<Ident, Hash64> {
    let tree
        = LockfileTree::new(project, lockfile);

    let islands
        = workspace_islands(project);
    let workspace_dependencies
        = project.workspace_dependencies();

    let mut table
        = NodeTable::default();
    let mut dependencies_by_node: Vec<Vec<usize>>
        = Vec::new();

    // Some nodes have more to them than what the graph can express
    let mut extra_hash_segments: HashMap<usize, BTreeSet<String>>
        = HashMap::new();

    let mut resolutions: HashMap<Descriptor, Option<usize>>
        = HashMap::new();

    let mut queue
        = Vec::new();

    for (ident, dependencies) in &workspace_dependencies {
        let (id, _)
            = table.register(TreeNode::Workspace(ident));

        dependencies_by_node.push(Vec::new());

        let mut dependencies = dependencies.as_ref()
            .map_or_else(|_| vec![], |dependencies| dependencies.values().cloned().collect_vec());

        // Islands have their own resolution table which we hash as a whole;
        // the only dependencies that we need to walk are the workspaces.
        if let Some(island_id) = islands.get(ident) {
            dependencies.retain(|descriptor| {
                project.try_workspace_by_descriptor(descriptor).ok().flatten().is_some()
            });

            let island_segments = lockfile.islands.get(island_id).into_iter().flatten()
                .map(|(descriptor, locator)| format!("{} -> {}", descriptor.to_file_string(), locator.to_file_string()));

            extra_hash_segments.entry(id)
                .or_default()
                .extend(island_segments);
        }

        queue.push((id, dependencies));
    }

    while let Some((id, dependencies)) = queue.pop() {
        for descriptor in dependencies {
            let dependency = match resolutions.get(&descriptor) {
                Some(dependency) => *dependency,

                None => {
                    let dependency = tree.resolve(&descriptor).map(|node| {
                        let (dependency, is_new)
                            = table.register(node);

                        if is_new {
                            dependencies_by_node.push(Vec::new());

                            // All the workspaces have been queued upfront
                            if let TreeNode::Package(locator) = node {
                                queue.push((dependency, tree.package_dependencies(locator).unwrap_or_default()));
                            }
                        }

                        dependency
                    });

                    resolutions.insert(descriptor.clone(), dependency);
                    dependency
                },
            };

            match dependency {
                Some(dependency) => {
                    dependencies_by_node[id].push(dependency);
                },

                // Descriptors that can't be found in the lockfile still contribute to the
                // hash of the nodes that depend on them, so that they don't go unnoticed.
                None => {
                    extra_hash_segments.entry(id)
                        .or_default()
                        .insert(descriptor.to_file_string());
                },
            }
        }
    }

    let hashes
        = compute_node_hashes(&table.nodes, &dependencies_by_node, &extra_hash_segments);

    workspace_dependencies.keys()
        .filter_map(|ident| Some((ident.clone(), hashes[*table.ids.get(&TreeNode::Workspace(ident))?].clone())))
        .collect()
}

fn workspace_islands(project: &Project) -> BTreeMap<Ident, String> {
    let mut islands
        = BTreeMap::new();

    for (island_id, island) in &project.config.settings.unstable_islands {
        for workspace in &project.workspaces {
            if island.workspaces.iter().any(|glob| glob.value.check(&workspace.name)) {
                islands.insert(workspace.name.clone(), island_id.clone());
            }
        }
    }

    islands
}

fn compute_node_hashes(
    nodes: &[TreeNode<'_>],
    dependencies_by_node: &[Vec<usize>],
    extra_hash_segments: &HashMap<usize, BTreeSet<String>>,
) -> Vec<Hash64> {
    let sccs
        = scc_tarjan_pearce_core(dependencies_by_node);

    let mut scc_by_node
        = vec![0; nodes.len()];

    for (scc_id, scc) in sccs.iter().enumerate() {
        for id in scc {
            scc_by_node[*id] = scc_id;
        }
    }

    let mut scc_hashes: Vec<Hash64>
        = Vec::with_capacity(sccs.len());

    // The components are returned in reverse topological order, so by the
    // time we process one all the components it depends on have their hash.
    for (scc_id, scc) in sccs.iter().enumerate() {
        let mut member_strings
            = scc.iter()
                .map(|id| nodes[*id].to_file_string())
                .collect_vec();

        member_strings.sort();

        let mut extra_strings
            = scc.iter()
                .flat_map(|id| extra_hash_segments.get(id))
                .flatten()
                .collect_vec();

        extra_strings.sort();
        extra_strings.dedup();

        let mut external_sccs
            = scc.iter()
                .flat_map(|id| &dependencies_by_node[*id])
                .map(|dependency| scc_by_node[*dependency])
                .filter(|dependency_scc_id| *dependency_scc_id != scc_id)
                .collect_vec();

        external_sccs.sort();
        external_sccs.dedup();

        let mut external_hashes
            = external_sccs.into_iter()
                .filter_map(|dependency_scc_id| scc_hashes.get(dependency_scc_id))
                .collect_vec();

        external_hashes.sort();
        external_hashes.dedup();

        let mut hash_writer
            = Hash64Writer::new();

        for s in &member_strings {
            hash_writer.update(s);
        }

        for s in extra_strings {
            hash_writer.update(s);
        }

        for h in external_hashes {
            hash_writer.update(h.to_file_string());
        }

        scc_hashes.push(hash_writer.finalize());
    }

    scc_by_node.into_iter()
        .map(|scc_id| scc_hashes[scc_id].clone())
        .collect()
}
