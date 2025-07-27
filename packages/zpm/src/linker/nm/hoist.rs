use indexmap::{IndexMap, set::IndexSet};
use std::time::Instant;

/// High-level node_modules hoisting algorithm recipe
///
/// 1. Take input dependency graph and start traversing it,
/// as you visit new node in the graph - clone it if there can be multiple paths
/// to access the node from the graph root to the node, e.g. essentially represent
/// the graph with a tree as you go, to make hoisting possible.
///
/// 2. You want to hoist every node possible to the top root node first,
/// then to each of its children etc, so you need to keep track what is your current
/// root node into which you are hoisting
///
/// 3. Traverse the dependency graph from the current root node and for each package name
/// that can be potentially hoisted to the current root node build a list of idents
/// in descending hoisting preference. You will check in next steps whether most preferred ident
/// for the given package name can be hoisted first, and if not, then you check the
/// less preferred ident, etc, until either some ident will be hoisted
/// or you run out of idents to check
/// (no need to convert the graph to the tree when you build this preference map).
///
/// 4. The children of the root node are already "hoisted", so you need to start
/// from the dependencies of these children. You take some child and
/// sort its dependencies so that regular dependencies without peer dependencies
/// will come first and then those dependencies that peer depend on them.
/// This is needed to make algorithm more efficient and hoist nodes which are easier
/// to hoist first and then handle peer dependent nodes.
///
/// 5. You take this sorted list of dependencies and check if each of them can be
/// hoisted to the current root node. To answer is the node can be hoisted you check
/// your constraints - require promise and peer dependency promise.
/// The possible answers can be: YES - the node is hoistable to the current root,
/// NO - the node is not hoistable to the current root
/// and DEPENDS - the node is hoistable to the root if nodes X, Y, Z are hoistable
/// to the root. The case DEPENDS happens when all the require and other
/// constraints are met, except peer dependency constraints. Note, that the nodes
/// that are not package idents currently at the top of preference list are considered
/// to have the answer NO right away, before doing any other constraint checks.
///
/// 6. When you have hoistable answer for each dependency of a node you then build
/// a list of nodes that are NOT hoistable. These are the nodes that have answer NO
/// and the nodes that DEPENDS on these nodes. All the other nodes are hoistable,
/// those that have answer YES and those that have answer DEPENDS,
/// because they are cyclically dependent on each another
///
/// 7. You hoist all the hoistable nodes to the current root and continue traversing
/// the tree. Note, you need to track newly added nodes to the current root,
/// because after you finished tree traversal you want to come back to these new nodes
/// first thing and hoist everything from each of them to the current tree root.
///
/// 8. After you have finished traversing newly hoisted current root nodes
/// it means you cannot hoist anything to the current tree root and you need to pick
/// the next node as current tree root and run the algorithm again
/// until you run out of candidates for current tree root.

type HoisterName = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoisterDependencyKind {
    Regular,
    Workspace,
    ExternalSoftLink,
}

#[derive(Debug, Clone)]
pub struct HoisterNode {
    pub id: usize,
    pub name: HoisterName,
    pub ident_name: HoisterName,
    pub reference: String,
    pub dependencies: IndexSet<usize>,
    pub peer_names: IndexSet<HoisterName>,
    pub hoist_priority: Option<i32>,
    pub dependency_kind: Option<HoisterDependencyKind>,
}

#[derive(Debug, Clone)]
pub struct HoisterTree {
    pub nodes: Vec<HoisterNode>,
    pub root: usize,
}

#[derive(Debug, Clone)]
pub struct HoisterResult {
    pub name: HoisterName,
    pub ident_name: HoisterName,
    pub references: IndexSet<String>,
    pub dependencies: Vec<Box<HoisterResult>>,
}

type HoisterLocator = String;
type HoisterIdent = String;

#[derive(Debug, Clone)]
struct HoisterWorkNode {
    id: usize,
    name: HoisterName,
    references: IndexSet<String>,
    ident: HoisterIdent,
    locator: HoisterLocator,
    dependencies: IndexMap<HoisterName, usize>,
    original_dependencies: IndexMap<HoisterName, usize>,
    hoisted_dependencies: IndexMap<HoisterName, usize>,
    peer_names: IndexSet<HoisterName>,
    decoupled: bool,
    reasons: IndexMap<HoisterName, String>,
    is_hoist_border: bool,
    hoisted_from: IndexMap<HoisterName, Vec<String>>,
    hoisted_to: IndexMap<HoisterName, String>,
    hoist_priority: i32,
    dependency_kind: HoisterDependencyKind,
}

#[derive(Debug, Clone)]

struct HoisterWorkTree {
    nodes: Vec<HoisterWorkNode>,
    root: usize,
}

/// Mapping which packages depend on a given package alias + ident. It is used to determine hoisting weight,
/// e.g. which one among the group of packages with the same name should be hoisted.
/// The package having the biggest number of parents using this package will be hoisted.
type PreferenceMap = IndexMap<String, PreferenceEntry>;

struct PreferenceEntry {
    peer_dependents: IndexSet<HoisterIdent>,
    dependents: IndexSet<HoisterIdent>,
    hoist_priority: i32,
}

#[derive(Debug, Clone, PartialEq)]
enum Hoistable {
    Yes,
    No,
    Depends,
}

#[derive(Clone)]
enum HoistInfo {
    Yes,
    No { reason: Option<String> },
    Depends { depends_on: IndexSet<usize>, reason: Option<String> },
}

impl HoistInfo {
    fn is_hoistable(&self) -> Hoistable {
        match self {
            HoistInfo::Yes => Hoistable::Yes,
            HoistInfo::No { .. } => Hoistable::No,
            HoistInfo::Depends { .. } => Hoistable::Depends,
        }
    }

    fn reason(&self) -> Option<&str> {
        match self {
            HoistInfo::Yes => None,
            HoistInfo::No { reason } => reason.as_deref(),
            HoistInfo::Depends { reason, .. } => reason.as_deref(),
        }
    }

    fn depends_on(&self) -> Option<&IndexSet<usize>> {
        match self {
            HoistInfo::Depends { depends_on, .. } => Some(depends_on),
            _ => None,
        }
    }
}

type ShadowedNodes = IndexMap<usize, IndexSet<HoisterName>>;

fn make_locator(name: &str, reference: &str) -> String {
    format!("{}@{}", name, reference)
}

fn make_ident(name: &str, reference: &str) -> String {
    let Index_idx = reference.find('#');

    // Strip virtual reference part, we don't need it for hoisting purposes
    let real_reference = if let Some(idx) = Index_idx {
        &reference[idx + 1..]
    } else {
        reference
    };

    make_locator(name, real_reference)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DebugLevel {
    None = -1,
    Perf = 0,
    Check = 1,
    Reasons = 2,
    IntensiveCheck = 9,
}

pub struct HoistOptions {
    /// Runs self-checks after hoisting is finished
    pub check: Option<bool>,
    /// Debug level
    pub debug_level: Option<DebugLevel>,
    /// Hoist borders are defined by parent node locator and its dependency name. The dependency is considered a border, nothing can be hoisted past this dependency, but dependency can be hoisted
    pub hoisting_limits: Option<IndexMap<HoisterLocator, IndexSet<HoisterName>>>,
}

impl Default for HoistOptions {
    fn default() -> Self {
        Self {
            check: None,
            debug_level: None,
            hoisting_limits: None,
        }
    }
}

struct InternalHoistOptions {
    check: bool,
    debug_level: DebugLevel,
    fast_lookup_possible: bool,
    hoisting_limits: IndexMap<HoisterLocator, IndexSet<HoisterName>>,
}

/// Hoists package tree.
///
/// The root node of a tree must has id: '.'.
/// This function does not mutate its arguments, it hoists and returns tree copy.
///
/// # Arguments
/// * `tree` - package tree (cycles in the tree are allowed)
///
/// # Returns
/// hoisted tree copy
pub fn hoist(tree: HoisterTree, opts: Option<HoistOptions>) -> Result<HoisterResult, String> {
    let opts = opts.unwrap_or_default();

    let debug_level = opts.debug_level.unwrap_or_else(|| {
        std::env::var("NM_DEBUG_LEVEL")
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .map(|n| match n {
                -1 => DebugLevel::None,
                0 => DebugLevel::Perf,
                1 => DebugLevel::Check,
                2 => DebugLevel::Reasons,
                9.. => DebugLevel::IntensiveCheck,
                _ => DebugLevel::None,
            })
            .unwrap_or(DebugLevel::None)
    });

    let check = opts.check.unwrap_or(debug_level >= DebugLevel::IntensiveCheck);
    let hoisting_limits = opts.hoisting_limits.unwrap_or_default();

    let mut options = InternalHoistOptions {
        check,
        debug_level,
        hoisting_limits,
        fast_lookup_possible: true,
    };

    let start_time = if options.debug_level >= DebugLevel::Perf {
        Some(Instant::now())
    } else {
        None
    };

    let mut tree_copy = clone_tree(tree, &options);

    let mut round = 0;
    let mut another_round_needed = true;

    while another_round_needed {
        let root_id = tree_copy.root;
        let root_work_node = &tree_copy.nodes[root_id];
        let root_locator = root_work_node.locator.clone();
        let mut root_locators = IndexSet::new();
        root_locators.insert(root_locator);

        let result = hoist_to(
            &mut tree_copy,
            vec![root_id],
            root_locators,
            IndexMap::new(),
            &mut options,
            &mut IndexSet::new(),
        );

        another_round_needed = result.another_round_needed || result.is_graph_changed;
        options.fast_lookup_possible = false;

        round += 1;
    }

    if options.debug_level >= DebugLevel::Perf {
        if let Some(start) = start_time {
            println!("hoist time: {:?}ms, rounds: {}", start.elapsed().as_millis(), round);
        }
    }

    if options.debug_level >= DebugLevel::Check {
        let prev_tree_dump = dump_dep_tree(&tree_copy);

        let root_id = tree_copy.root;
        let root_work_node = &tree_copy.nodes[root_id];
        let root_locator = root_work_node.locator.clone();
        let mut root_locators = IndexSet::new();
        root_locators.insert(root_locator);

        let is_graph_changed = hoist_to(
            &mut tree_copy,
            vec![root_id],
            root_locators,
            IndexMap::new(),
            &mut options,
            &mut IndexSet::new(),
        ).is_graph_changed;

        if is_graph_changed {
            return Err(format!(
                "The hoisting result is not terminal, prev tree:\n{}, next tree:\n{}",
                prev_tree_dump,
                dump_dep_tree(&tree_copy)
            ));
        }

        if let Some(check_log) = self_check(&tree_copy) {
            return Err(format!(
                "{}, after hoisting finished:\n{}",
                check_log,
                dump_dep_tree(&tree_copy)
            ));
        }
    }

    if options.debug_level >= DebugLevel::Reasons {
        println!("{}", dump_dep_tree(&tree_copy));
    }

    println!("tree_copy: {:#?}", tree_copy);
    println!("shrinked: {:#?}", shrink_tree(&tree_copy));

    Ok(shrink_tree(&tree_copy))
}

fn get_zero_round_used_dependencies(
    tree: &HoisterWorkTree,
    root_node_path: &[usize],
) -> IndexMap<HoisterName, usize> {
    let root_node = root_node_path[root_node_path.len() - 1];
    let mut used_dependencies = IndexMap::new();
    let mut seen_nodes = IndexSet::new();

    fn add_used_dependencies(
        tree: &HoisterWorkTree,
        node_id: usize,
        used_dependencies: &mut IndexMap<HoisterName, usize>,
        seen_nodes: &mut IndexSet<usize>,
    ) {
        if seen_nodes.contains(&node_id) {
            return;
        }

        seen_nodes.insert(node_id);

        let node = &tree.nodes[node_id];

        for dep_id in node.hoisted_dependencies.values() {
            used_dependencies.insert(tree.nodes[*dep_id].name.clone(), *dep_id);
        }

        for dep_id in node.dependencies.values() {
            let dep = &tree.nodes[*dep_id];
            if !node.peer_names.contains(&dep.name) {
                add_used_dependencies(tree, *dep_id, used_dependencies, seen_nodes);
            }
        }
    }

    add_used_dependencies(tree, root_node, &mut used_dependencies, &mut seen_nodes);

    used_dependencies
}

fn get_used_dependencies(
    tree: &HoisterWorkTree,
    root_node_path: &[usize],
) -> IndexMap<HoisterName, usize> {
    let root_node_id = root_node_path[root_node_path.len() - 1];
    let mut used_dependencies = IndexMap::new();
    let mut seen_nodes = IndexSet::new();

    let hidden_dependencies = IndexSet::new();

    fn add_used_dependencies(
        tree: &HoisterWorkTree,
        root_node_path: &[usize],
        node_id: usize,
        hidden_dependencies: IndexSet<HoisterName>,
        used_dependencies: &mut IndexMap<HoisterName, usize>,
        seen_nodes: &mut IndexSet<usize>,
    ) {
        if seen_nodes.contains(&node_id) {
            return;
        }

        seen_nodes.insert(node_id);

        let node = &tree.nodes[node_id];

        for dep_id in node.hoisted_dependencies.values() {
            let dep = &tree.nodes[*dep_id];
            if hidden_dependencies.contains(&dep.name) {
                continue;
            }

            for node_id in root_node_path {
                let node = &tree.nodes[*node_id];
                if let Some(reachable_dependency_id) = node.dependencies.get(&dep.name) {
                    let reachable_dependency = &tree.nodes[*reachable_dependency_id];
                    used_dependencies.insert(reachable_dependency.name.clone(), *reachable_dependency_id);
                }
            }
        }

        let mut children_hidden_dependencies = IndexSet::new();
        for dep_id in node.dependencies.values() {
            let dep = &tree.nodes[*dep_id];
            children_hidden_dependencies.insert(dep.name.clone());
        }

        for dep_id in node.dependencies.values() {
            let dep = &tree.nodes[*dep_id];
            if !node.peer_names.contains(&dep.name) {
                add_used_dependencies(
                    tree,
                    root_node_path,
                    *dep_id,
                    children_hidden_dependencies.clone(),
                    used_dependencies,
                    seen_nodes,
                );
            }
        }
    }

    add_used_dependencies(
        tree,
        root_node_path,
        root_node_id,
        hidden_dependencies,
        &mut used_dependencies,
        &mut seen_nodes,
    );

    used_dependencies
}

/// This method clones the node and returns cloned node copy, if the node was not previously decoupled.
///
/// The node is considered decoupled if there is no multiple parents to any node
/// on the path from the dependency graph root up to this node. This means that there are no other
/// nodes in dependency graph that somehow transitively use this node and hence node can be hoisted without
/// side effects.
///
/// The process of node decoupling is done by going from root node of the graph up to the node in concern
/// and decoupling each node on this graph path.
///
/// # Arguments
/// * `node` - original node
///
/// # Returns
/// decoupled node
fn decouple_graph_node(
    tree: &mut HoisterWorkTree,
    parent_id: usize,
    node_id: usize,
) -> usize {
    let node = &tree.nodes[node_id];
    if node.decoupled {
        return node_id;
    }

    let node = node.clone();

    // To perform node hoisting from parent node we must clone parent nodes up to the root node,
    // because some other package in the tree might depend on the parent package where hoisting
    // cannot be performed
    let mut clone = HoisterWorkNode {
        id: tree.nodes.len(),
        name: node.name.clone(),
        references: node.references.clone(),
        ident: node.ident.clone(),
        locator: node.locator.clone(),
        dependencies: node.dependencies.clone(),
        original_dependencies: node.original_dependencies.clone(),
        hoisted_dependencies: node.hoisted_dependencies.clone(),
        peer_names: node.peer_names.clone(),
        reasons: node.reasons.clone(),
        decoupled: true,
        is_hoist_border: node.is_hoist_border,
        hoist_priority: node.hoist_priority,
        dependency_kind: node.dependency_kind,
        hoisted_from: node.hoisted_from.clone(),
        hoisted_to: node.hoisted_to.clone(),
    };

    let clone_id = clone.id;

    // Update self-reference
    if let Some(self_dep_id) = clone.dependencies.get(&clone.name).cloned() {
        let self_dep = &tree.nodes[self_dep_id];
        if self_dep.ident == clone.ident {
            clone.dependencies.insert(clone.name.clone(), clone_id);
        }
    }

        tree.nodes.push(clone);

    let clone_name = tree.nodes[clone_id].name.clone();
    let parent = &mut tree.nodes[parent_id];
    parent.dependencies.insert(clone_name, clone_id);

    clone_id
}

/// Builds a map of most preferred packages that might be hoisted to the root node.
///
/// The values in the map are idents sorted by preference from most preferred to less preferred.
/// If the root node has already some version of a package, the value array will contain only
/// one element, since it is not possible for other versions of a package to be hoisted.
///
/// # Arguments
/// * `root_node` - root node
/// * `preference_map` - preference map
fn get_hoist_ident_map(
    tree: &HoisterWorkTree,
    root_node_id: usize,
    preference_map: &PreferenceMap,
) -> IndexMap<HoisterName, Vec<HoisterIdent>> {
    let root_node = &tree.nodes[root_node_id];

    let mut ident_map: IndexMap<HoisterName, Vec<HoisterIdent>> = IndexMap::new();
    ident_map.insert(root_node.name.clone(), vec![root_node.ident.clone()]);

    for dep_id in root_node.dependencies.values() {
        let dep = &tree.nodes[*dep_id];
        if !root_node.peer_names.contains(&dep.name) {
            ident_map.insert(dep.name.clone(), vec![dep.ident.clone()]);
        }
    }

    let mut key_list: Vec<_> = preference_map.keys().cloned().collect();

    key_list.sort_by(|key1, key2| {
        let entry1 = &preference_map[key1];
        let entry2 = &preference_map[key2];

        if entry2.hoist_priority != entry1.hoist_priority {
            return entry2.hoist_priority.cmp(&entry1.hoist_priority);
        }

        let entry1_usages = entry1.dependents.len() + entry1.peer_dependents.len();
        let entry2_usages = entry2.dependents.len() + entry2.peer_dependents.len();

        entry2_usages.cmp(&entry1_usages)
    });

    for key in key_list {
        let at_idx = key.find('@').unwrap_or(0) + 1;
        let name = key[..at_idx - 1].to_string();
        let ident = key[at_idx..].to_string();

        if root_node.peer_names.contains(&name) {
            continue;
        }

        let idents = ident_map.entry(name).or_insert_with(Vec::new);
        if !idents.contains(&ident) {
            idents.push(ident);
        }
    }

    ident_map
}

/// Gets regular node dependencies only and sorts them in the order so that
/// peer dependencies come before the dependency that rely on them.
///
/// # Arguments
/// * `node` - graph node
///
/// # Returns
/// sorted regular dependencies
fn get_sorted_regular_dependencies(
    tree: &HoisterWorkTree,
    node_id: usize,
) -> Vec<usize> {
    let node = &tree.nodes[node_id];
    let mut dependencies = Vec::new();
    let mut dependencies_set = IndexSet::new();

    fn add_dep(
        tree: &HoisterWorkTree,
        node: &HoisterWorkNode,
        dep_id: usize,
        dependencies: &mut Vec<usize>,
        dependencies_set: &mut IndexSet<usize>,
        seen_deps: &mut IndexSet<usize>,
    ) {
        if seen_deps.contains(&dep_id) {
            return;
        }

        seen_deps.insert(dep_id);

        let dep = &tree.nodes[dep_id];

        for peer_name in &dep.peer_names {
            if node.peer_names.contains(peer_name) {
                continue;
            }

            if let Some(peer_dep) = node.dependencies.get(peer_name) {
                if !dependencies_set.contains(peer_dep) {
                    let mut inner_seen = seen_deps.clone();
                    add_dep(tree, node, *peer_dep, dependencies, dependencies_set, &mut inner_seen);
                }
            }
        }

        if !dependencies_set.contains(&dep_id) {
            dependencies.push(dep_id);
            dependencies_set.insert(dep_id);
        }
    }

    for dep_id in node.dependencies.values() {
        let dep = &tree.nodes[*dep_id];
        if !node.peer_names.contains(&dep.name) {
            let mut seen_deps = IndexSet::new();
            add_dep(tree, node, *dep_id, &mut dependencies, &mut dependencies_set, &mut seen_deps);
        }
    }

    dependencies
}

struct HoistResult {
    another_round_needed: bool,
    is_graph_changed: bool,
}

/// Performs hoisting all the dependencies down the tree to the root node.
///
/// The algorithm used here reduces dependency graph by deduplicating
/// instances of the packages while keeping:
/// 1. Regular dependency promise: the package should require the exact version of the dependency
/// that was declared in its `package.json`
/// 2. Peer dependency promise: the package and its direct parent package
/// must use the same instance of the peer dependency
///
/// The regular and peer dependency promises are kept while performing transform
/// on tree branches of packages at a time:
/// `root package` -> `parent package 1` ... `parent package n` -> `dependency`
/// We check wether we can hoist `dependency` to `root package`, this boils down basically
/// to checking:
/// 1. Wether `root package` does not depend on other version of `dependency`
/// 2. Wether all the peer dependencies of a `dependency` had already been hoisted from all `parent packages`
///
/// If many versions of the `dependency` can be hoisted to the `root package` we choose the most used
/// `dependency` version in the project among them.
///
/// This function mutates the tree.
///
/// # Arguments
/// * `tree` - package dependencies graph
/// * `root_node` - root node to hoist to
/// * `root_node_path` - root node path in the tree
/// * `root_node_path_locators` - a set of locators for nodes that lead from the top of the tree up to root node
/// * `options` - hoisting options
fn hoist_to(
    tree: &mut HoisterWorkTree,
    root_node_path: Vec<usize>,
    root_node_path_locators: IndexSet<HoisterLocator>,
    parent_shadowed_nodes: ShadowedNodes,
    options: &mut InternalHoistOptions,
    seen_nodes: &mut IndexSet<usize>,
) -> HoistResult {
    let root_node_id = root_node_path[root_node_path.len() - 1];

    if seen_nodes.contains(&root_node_id) {
        return HoistResult {
            another_round_needed: false,
            is_graph_changed: false,
        };
    }

    seen_nodes.insert(root_node_id);

    let preference_map = build_preference_map(tree, root_node_id);
    let mut hoist_ident_map = get_hoist_ident_map(tree, root_node_id, &preference_map);

    let used_dependency_ids = if tree.root == root_node_id {
        IndexMap::new()
    } else if options.fast_lookup_possible {
        get_zero_round_used_dependencies(tree, &root_node_path)
    } else {
        get_used_dependencies(tree, &root_node_path)
    };

    let mut another_round_needed = false;
    let mut is_graph_changed = false;

    let mut hoist_idents: IndexMap<HoisterName, HoisterIdent> = hoist_ident_map
        .iter()
        .map(|(k, v)| (k.clone(), v[0].clone()))
        .collect();

    let mut shadowed_nodes: ShadowedNodes = IndexMap::new();

    let mut was_state_changed = true;
    while was_state_changed {
        let result = hoist_graph(
            tree,
            &root_node_path,
            &root_node_path_locators,
            &used_dependency_ids,
            &hoist_idents,
            &hoist_ident_map,
            &parent_shadowed_nodes,
            &mut shadowed_nodes,
            options,
        );

        if result.is_graph_changed {
            is_graph_changed = true;
        }
        if result.another_round_needed {
            another_round_needed = true;
        }

        was_state_changed = false;

        let root_node = &tree.nodes[root_node_id];
        for (name, idents) in &mut hoist_ident_map {
            if idents.len() > 1 && !root_node.dependencies.contains_key(name) {
                hoist_idents.shift_remove(name);
                idents.remove(0);
                if idents.len() > 1 {
                    hoist_idents.insert(name.clone(), idents[1].clone());
                    was_state_changed = true;
                }
            }
        }
    }

    let dependencies: Vec<_> = tree.nodes[root_node_id].dependencies.values().cloned().collect();

    for dependency_id in dependencies {
        let dependency = &tree.nodes[dependency_id];

        if tree.nodes[root_node_id].peer_names.contains(&dependency.name) {
            continue;
        }
        if root_node_path_locators.contains(&dependency.locator) {
            continue;
        }

        let mut new_path = root_node_path.clone();
        new_path.push(dependency_id);

        let mut new_locators = root_node_path_locators.clone();
        new_locators.insert(dependency.locator.clone());

        let result = hoist_to(
            tree,
            new_path,
            new_locators,
            shadowed_nodes.clone(),
            options,
            seen_nodes,
        );

        if result.is_graph_changed {
            is_graph_changed = true;
        }
        if result.another_round_needed {
            another_round_needed = true;
        }
    }

    HoistResult {
        another_round_needed,
        is_graph_changed,
    }
}

fn has_unhoisted_dependencies(tree: &HoisterWorkTree, node_id: usize) -> bool {
    let node = &tree.nodes[node_id];

    for (sub_name, sub_dependency_id) in &node.dependencies {
        if node.peer_names.contains(sub_name) {
            continue;
        }

        let sub_dependency = &tree.nodes[*sub_dependency_id];

        if sub_dependency.ident != node.ident {
            return true;
        }
    }

    false
}

fn get_node_hoist_info(
    tree: &HoisterWorkTree,
    root_node_id: usize,
    root_node_path_locators: &IndexSet<HoisterLocator>,
    node_path: &[usize],
    node_id: usize,
    used_dependency_ids: &IndexMap<HoisterName, usize>,
    hoist_idents: &IndexMap<HoisterName, HoisterIdent>,
    hoist_ident_map: &IndexMap<HoisterName, Vec<HoisterIdent>>,
    shadowed_nodes: &mut ShadowedNodes,
    output_reason: bool,
    fast_lookup_possible: bool,
) -> HoistInfo {
    let mut reason: Option<String> = None;
    let mut depends_on: IndexSet<usize> = IndexSet::new();

    let reason_root = if output_reason {
        Some(
            root_node_path_locators
                .iter()
                .map(|x| pretty_print_locator(Some(x)))
                .collect::<Vec<_>>()
                .join("→")
        )
    } else {
        None
    };

    let node = &tree.nodes[node_id];

    let parent_node_id = node_path[node_path.len() - 1];
    let parent_node = &tree.nodes[parent_node_id];

    // We cannot hoist self-references
    let is_self_reference = node.ident == parent_node.ident;

    let mut is_hoistable = true;
    if is_hoistable {
        is_hoistable = !is_self_reference;
        if output_reason && !is_hoistable {
            reason = Some("- self-reference".to_string());
        }
    }

    if is_hoistable {
        is_hoistable = node.dependency_kind != HoisterDependencyKind::Workspace;
        if output_reason && !is_hoistable {
            reason = Some("- workspace".to_string());
        }
    }

    if is_hoistable && node.dependency_kind == HoisterDependencyKind::ExternalSoftLink {
        is_hoistable = !has_unhoisted_dependencies(tree, node_id);
        if output_reason && !is_hoistable {
            reason = Some("- external soft link with unhoisted dependencies".to_string());
        }
    }

    let root_node = &tree.nodes[root_node_id];

    if is_hoistable {
        is_hoistable = !root_node.peer_names.contains(&node.name);
        if output_reason && !is_hoistable {
            let original_dependency_id = root_node.original_dependencies.get(&node.name)
                .expect("Expected the original dependency ID to be set");

            let original_dependency = &tree.nodes[*original_dependency_id];

            reason = Some(format!(
                "- cannot shadow peer: {} at {}",
                pretty_print_locator(Some(&original_dependency.locator)),
                reason_root.as_ref().unwrap()
            ));
        }
    }

    if is_hoistable {
        let used_dep = used_dependency_ids.get(&node.name)
            .map(|id| &tree.nodes[*id]);

        let mut is_name_available = used_dep.map_or(true, |dep| dep.ident == node.ident);
        if output_reason && !is_name_available {
            reason = Some(format!(
                "- filled by: {} at {}",
                pretty_print_locator(Some(&used_dep.unwrap().locator)),
                reason_root.as_ref().unwrap()
            ));
        }

        if is_name_available {
            for idx in (1..=node_path.len() - 1).rev() {
                let parent_id = node_path[idx];
                let parent = &tree.nodes[parent_id];

                if let Some(parent_dep_id) = parent.dependencies.get(&node.name) {
                    let parent_dep = &tree.nodes[*parent_dep_id];
                    if parent_dep.ident == node.ident {
                        continue;
                    }

                    is_name_available = false;

                    let shadowed_names = shadowed_nodes.entry(parent_node_id).or_insert_with(IndexSet::new);
                    shadowed_names.insert(node.name.clone());

                    if output_reason {
                        reason = Some(format!(
                            "- filled by {} at {}",
                            pretty_print_locator(Some(&parent_dep.locator)),
                            node_path[..idx]
                                .iter()
                                .map(|id| pretty_print_locator(Some(&tree.nodes[*id].locator)))
                                .collect::<Vec<_>>()
                                .join("→")
                        ));
                    }

                    break;
                }
            }
        }

        is_hoistable = is_name_available;
    }

    if is_hoistable {
        let hoisted_ident = hoist_idents.get(&node.name);
        is_hoistable = hoisted_ident.map_or(false, |ident| ident == &node.ident);

        if output_reason && !is_hoistable {
            if let Some(idents) = hoist_ident_map.get(&node.name) {
                reason = Some(format!(
                    "- filled by: {} at {}",
                    pretty_print_locator(Some(&idents[0])),
                    reason_root.as_ref().unwrap()
                ));
            }
        }
    }

    if is_hoistable {
        let mut are_peer_deps_satisfied = true;

        let mut check_list: IndexSet<_> = node.peer_names.iter().cloned().collect();
        for idx in (1..=node_path.len() - 1).rev() {
            let parent_id = node_path[idx];
            let parent = &tree.nodes[parent_id];

            let mut to_remove = Vec::new();
            for name in &check_list {
                if parent.peer_names.contains(name) && parent.original_dependencies.contains_key(name) {
                    continue;
                }

                if let Some(parent_dep_node_id) = parent.dependencies.get(name) {
                    let parent_dep_node = &tree.nodes[*parent_dep_node_id];
                    if root_node.dependencies.get(name) != Some(parent_dep_node_id) {
                        if idx == node_path.len() - 1 {
                            depends_on.insert(*parent_dep_node_id);
                        } else {
                            are_peer_deps_satisfied = false;

                            if output_reason {
                                reason = Some(format!(
                                    "- peer dependency {} from parent {} was not hoisted to {}",
                                    pretty_print_locator(Some(&parent_dep_node.locator)),
                                    pretty_print_locator(Some(&parent.locator)),
                                    reason_root.as_ref().unwrap()
                                ));
                            }
                        }
                    }
                }

                to_remove.push(name.clone());
            }

            for name in to_remove {
                check_list.remove(&name);
            }

            if !are_peer_deps_satisfied {
                break;
            }
        }

        is_hoistable = are_peer_deps_satisfied;
    }

    if is_hoistable && !fast_lookup_possible {
        for orig_dep_id in node.hoisted_dependencies.values() {
            let orig_dep = &tree.nodes[*orig_dep_id];

            let used_dep_id = used_dependency_ids.get(&orig_dep.name)
                .or_else(|| root_node.dependencies.get(&orig_dep.name));

            if let Some(used_dep_id) = used_dep_id {
                let used_dep = &tree.nodes[*used_dep_id];
                if orig_dep.ident != used_dep.ident {
                    is_hoistable = false;

                    if output_reason {
                        reason = Some(format!(
                            "- previously hoisted dependency mismatch, needed: {}, available: {}",
                            pretty_print_locator(Some(&orig_dep.locator)),
                            pretty_print_locator(Some(&used_dep.locator))
                        ));
                    }

                    break;
                }
            }
        }
    }

    if !depends_on.is_empty() {
        HoistInfo::Depends { depends_on, reason }
    } else if is_hoistable {
        HoistInfo::Yes
    } else {
        HoistInfo::No { reason }
    }
}

fn get_aliased_locator(node: &HoisterWorkNode) -> String {
    format!("{}@{}", node.name, node.locator)
}

/// Performs actual graph transformation, by hoisting packages to the root node.
///
/// # Arguments
/// * `tree` - dependency tree
/// * `root_node_path` - root node path in the tree
/// * `root_node_path_locators` - a set of locators for nodes that lead from the top of the tree up to root node
/// * `used_dependencies` - map of dependency nodes from parents of root node used by root node and its children via parent lookup
/// * `hoist_idents` - idents that should be attempted to be hoisted to the root node
fn hoist_graph(
    tree: &mut HoisterWorkTree,
    root_node_path: &[usize],
    root_node_path_locators: &IndexSet<HoisterLocator>,
    used_dependency_ids: &IndexMap<HoisterName, usize>,
    hoist_idents: &IndexMap<HoisterName, HoisterIdent>,
    hoist_ident_map: &IndexMap<HoisterName, Vec<HoisterIdent>>,
    parent_shadowed_nodes: &ShadowedNodes,
    shadowed_nodes: &mut ShadowedNodes,
    options: &InternalHoistOptions,
) -> HoistResult {
    let root_node_id = root_node_path[root_node_path.len() - 1];

    let mut seen_nodes = IndexSet::new();

    let mut another_round_needed = false;
    let mut is_graph_changed = false;

    fn hoist_node_dependencies(
        tree: &mut HoisterWorkTree,
        root_node_id: usize,
        root_node_path: &[usize],
        root_node_path_locators: &IndexSet<HoisterLocator>,
        node_path: Vec<usize>,
        locator_path: Vec<HoisterLocator>,
        aliased_locator_path: Vec<String>,
        parent_node_id: usize,
        new_node_ids: &mut IndexSet<usize>,
        used_dependency_ids: &IndexMap<HoisterName, usize>,
        hoist_idents: &IndexMap<HoisterName, HoisterIdent>,
        hoist_ident_map: &IndexMap<HoisterName, Vec<HoisterIdent>>,
        parent_shadowed_nodes: &ShadowedNodes,
        shadowed_nodes: &mut ShadowedNodes,
        options: &InternalHoistOptions,
        seen_nodes: &mut IndexSet<usize>,
        another_round_needed: &mut bool,
        is_graph_changed: &mut bool,
    ) {
        if seen_nodes.contains(&parent_node_id) {
            return;
        }

        let parent_node = &tree.nodes[parent_node_id];
        let parent_aliased = get_aliased_locator(parent_node);

        let mut next_locator_path = locator_path.clone();
        next_locator_path.push(parent_aliased.clone());

        let mut next_aliased_locator_path = aliased_locator_path.clone();
        next_aliased_locator_path.push(parent_aliased.clone());

        let mut dependant_tree: IndexMap<HoisterName, IndexSet<HoisterName>> = IndexMap::new();
        let mut hoist_infos = IndexMap::new();

        let sorted_deps = get_sorted_regular_dependencies(tree, parent_node_id);

        for sub_dependency_id in &sorted_deps {
            let sub_dependency = &tree.nodes[*sub_dependency_id];

            let mut full_node_path = vec![root_node_id];
            full_node_path.extend_from_slice(&node_path);
            full_node_path.push(parent_node_id);

            let hoist_info = get_node_hoist_info(
                tree,
                root_node_id,
                root_node_path_locators,
                &full_node_path,
                *sub_dependency_id,
                used_dependency_ids,
                hoist_idents,
                hoist_ident_map,
                shadowed_nodes,
                options.debug_level >= DebugLevel::Reasons,
                options.fast_lookup_possible,
            );

            hoist_infos.insert(*sub_dependency_id, hoist_info.clone());

            if let HoistInfo::Depends { depends_on, .. } = &hoist_info {
                for node_id in depends_on {
                    let node = &tree.nodes[*node_id];
                    let node_dependants = dependant_tree.entry(node.name.clone()).or_insert_with(IndexSet::new);
                    node_dependants.insert(sub_dependency.name.clone());
                }
            }
        }

        let mut unhoistable_nodes = IndexSet::new();

        fn add_unhoistable_node(
            tree: &HoisterWorkTree,
            parent_node_id: usize,
            node_id: usize,
            reason: String,
            dependant_tree: &IndexMap<HoisterName, IndexSet<HoisterName>>,
            hoist_infos: &mut IndexMap<usize, HoistInfo>,
            unhoistable_nodes: &mut IndexSet<usize>,
            output_reason: bool,
        ) {
            if unhoistable_nodes.contains(&node_id) {
                return;
            }

            let node = &tree.nodes[node_id];
            let parent_node = &tree.nodes[parent_node_id];

            unhoistable_nodes.insert(node_id);
            hoist_infos.insert(node_id, HoistInfo::No { reason: Some(reason.clone()) });

            if let Some(dependants) = dependant_tree.get(&node.name) {
                for dependant_name in dependants {
                    if let Some(dep_id) = parent_node.dependencies.get(dependant_name) {
                        let new_reason = if output_reason {
                            format!(
                                "- peer dependency {} from parent {} was not hoisted",
                                pretty_print_locator(Some(&node.locator)),
                                pretty_print_locator(Some(&parent_node.locator))
                            )
                        } else {
                            String::new()
                        };

                        add_unhoistable_node(
                            tree,
                            parent_node_id,
                            *dep_id,
                            new_reason,
                            dependant_tree,
                            hoist_infos,
                            unhoistable_nodes,
                            output_reason,
                        );
                    }
                }
            }
        }

        for (node_id, hoist_info) in hoist_infos.clone() {
            if hoist_info.is_hoistable() == Hoistable::No {
                add_unhoistable_node(
                    tree,
                    parent_node_id,
                    node_id,
                    hoist_info.reason().unwrap_or("").to_string(),
                    &dependant_tree,
                    &mut hoist_infos,
                    &mut unhoistable_nodes,
                    options.debug_level >= DebugLevel::Reasons,
                );
            }
        }

        let mut were_nodes_hoisted = false;
        for node_id in hoist_infos.keys() {
            if unhoistable_nodes.contains(node_id) {
                continue;
            }

            *is_graph_changed = true;
            were_nodes_hoisted = true;

            let node = &tree.nodes[*node_id];
            let node_name = node.name.clone();

            if let Some(shadowed_names) = parent_shadowed_nodes.get(&parent_node_id) {
                if shadowed_names.contains(&node_name) {
                    *another_round_needed = true;
                }
            }

            let hoisted_node_id = tree.nodes[root_node_id].dependencies.get(&node_name).cloned();

            let parent_node = &mut tree.nodes[parent_node_id];
            parent_node.dependencies.remove(&node_name);
            parent_node.hoisted_dependencies.insert(node_name.clone(), *node_id);
            parent_node.reasons.remove(&node_name);

            if options.debug_level >= DebugLevel::Reasons {
                let hoisted_from = locator_path
                    .iter()
                    .chain(std::iter::once(&parent_node.locator))
                    .map(|x| pretty_print_locator(Some(x)))
                    .collect::<Vec<_>>()
                    .join("→");

                let root_node = &mut tree.nodes[root_node_id];
                let hoisted_from_array = root_node.hoisted_from.entry(node_name.clone()).or_insert_with(Vec::new);
                hoisted_from_array.push(hoisted_from);

                let pretty_locator_string = root_node_path
                    .iter()
                    .map(|id| pretty_print_locator(Some(&tree.nodes[*id].locator)))
                    .collect::<Vec<_>>()
                    .join("→");

                let parent_node = &mut tree.nodes[parent_node_id];
                parent_node.hoisted_to.insert(node_name.clone(), pretty_locator_string);
            }

            // Add hoisted node to root node, in case it is not already there
            if hoisted_node_id.is_none() {
                let node_ident = tree.nodes[*node_id].ident.clone();
                let root_node = &mut tree.nodes[root_node_id];
                // Avoid adding other version of root node to itself
                if root_node.ident != node_ident {
                    root_node.dependencies.insert(node_name.clone(), *node_id);
                    new_node_ids.insert(*node_id);
                }
            } else if let Some(hoisted_node_id) = hoisted_node_id {
                let node = &tree.nodes[*node_id];
                let references: Vec<_> = node.references.iter().cloned().collect();
                let hoisted_node = &mut tree.nodes[hoisted_node_id];
                for reference in references {
                    hoisted_node.references.insert(reference);
                }
            }
        }

        if tree.nodes[parent_node_id].dependency_kind == HoisterDependencyKind::ExternalSoftLink && were_nodes_hoisted {
            *another_round_needed = true;
        }

        if options.check {
            if let Some(check_log) = self_check(tree) {
                let path_str = std::iter::once(root_node_id)
                    .chain(node_path.iter().cloned())
                    .chain(std::iter::once(parent_node_id))
                    .map(|id| pretty_print_locator(Some(&tree.nodes[id].locator)))
                    .collect::<Vec<_>>()
                    .join("→");

                panic!(
                    "{}, after hoisting dependencies of {}:\n{}",
                    check_log,
                    path_str,
                    dump_dep_tree(tree)
                );
            }
        }

        let children = get_sorted_regular_dependencies(tree, parent_node_id);
        for node_id in children {
            if !unhoistable_nodes.contains(&node_id) {
                continue;
            }

            let hoist_info = hoist_infos.get(&node_id).unwrap();

            let node_name = tree.nodes[node_id].name.clone();
            let node_ident = tree.nodes[node_id].ident.clone();
            let parent_has_reason = tree.nodes[parent_node_id].reasons.contains_key(&node_name);

            let hoistable_ident = hoist_idents.get(&node_name);
            if (hoistable_ident.map_or(false, |i| i == &node_ident) || !parent_has_reason)
                && hoist_info.is_hoistable() != Hoistable::Yes {
                let reason = hoist_info.reason().unwrap_or("").to_string();
                let parent_node = &mut tree.nodes[parent_node_id];
                parent_node.reasons.insert(node_name.clone(), reason);
            }

            let node = &tree.nodes[node_id];
            let node_aliased = get_aliased_locator(node);

            if !node.is_hoist_border && !next_aliased_locator_path.contains(&node_aliased) {
                seen_nodes.insert(parent_node_id);

                let decoupled_node = decouple_graph_node(tree, parent_node_id, node_id);

                let mut new_node_path = node_path.clone();
                new_node_path.push(parent_node_id);

                hoist_node_dependencies(
                    tree,
                    root_node_id,
                    root_node_path,
                    root_node_path_locators,
                    new_node_path,
                    next_locator_path.clone(),
                    next_aliased_locator_path.clone(),
                    decoupled_node,
                    new_node_ids,
                    used_dependency_ids,
                    hoist_idents,
                    hoist_ident_map,
                    parent_shadowed_nodes,
                    shadowed_nodes,
                    options,
                    seen_nodes,
                    another_round_needed,
                    is_graph_changed,
                );

                seen_nodes.remove(&parent_node_id);
            }
        }
    }

    let aliased_root_node_path_locators: Vec<_> = root_node_path
        .iter()
        .map(|x| get_aliased_locator(&tree.nodes[*x]))
        .collect();

    let mut next_new_nodes: IndexSet<_> = get_sorted_regular_dependencies(tree, root_node_id)
        .into_iter()
        .collect();

    while !next_new_nodes.is_empty() {
        let new_nodes = next_new_nodes;
        next_new_nodes = IndexSet::new();

        for dep_id in new_nodes {
            let dep = &tree.nodes[dep_id];
            if dep.locator == tree.nodes[root_node_id].locator || dep.is_hoist_border {
                continue;
            }

            let decoupled_dependency = decouple_graph_node(tree, root_node_id, dep_id);
            hoist_node_dependencies(
                tree,
                root_node_id,
                root_node_path,
                root_node_path_locators,
                vec![],
                root_node_path_locators.iter().cloned().collect(),
                aliased_root_node_path_locators.clone(),
                decoupled_dependency,
                &mut next_new_nodes,
                used_dependency_ids,
                hoist_idents,
                hoist_ident_map,
                parent_shadowed_nodes,
                shadowed_nodes,
                options,
                &mut seen_nodes,
                &mut another_round_needed,
                &mut is_graph_changed,
            );
        }
    }

    HoistResult {
        another_round_needed,
        is_graph_changed,
    }
}

fn self_check(tree: &HoisterWorkTree) -> Option<String> {
    let mut log = Vec::new();

    let mut seen_nodes = IndexSet::new();
    let mut parents = IndexSet::new();

    fn check_node(
        tree: &HoisterWorkTree,
        node_id: usize,
        parent_dep_ids: IndexMap<HoisterName, usize>,
        parent_id: usize,
        log: &mut Vec<String>,
        seen_nodes: &mut IndexSet<usize>,
        parents: &mut IndexSet<usize>,
    ) {
        if seen_nodes.contains(&node_id) {
            return;
        }

        seen_nodes.insert(node_id);

        if parents.contains(&node_id) {
            return;
        }

        let node = &tree.nodes[node_id];
        let mut cloned_dep_ids = parent_dep_ids.clone();

        for dep_id in node.dependencies.values() {
            let dep = &tree.nodes[*dep_id];
            if !node.peer_names.contains(&dep.name) {
                cloned_dep_ids.insert(dep.name.clone(), *dep_id);
            }
        }

        let pretty_print_tree_path = || {
            parents.iter()
                .chain(std::iter::once(&node_id))
                .map(|id| pretty_print_locator(Some(&tree.nodes[*id].locator)))
                .collect::<Vec<_>>()
                .join("→")
        };

        for orig_dep_id in node.original_dependencies.values() {
            let orig_dep = &tree.nodes[*orig_dep_id];
            let dep_id = cloned_dep_ids.get(&orig_dep.name);

            if node.peer_names.contains(&orig_dep.name) {
                let parent_dep_id = parent_dep_ids.get(&orig_dep.name);

                let parent_dep = parent_dep_id.map(|id| &tree.nodes[*id]);

                if parent_dep.is_none() || parent_dep_id != dep_id || parent_dep.map_or(true, |d| d.ident != orig_dep.ident) {
                    log.push(format!(
                        "{} - broken peer promise: expected {} but found {}",
                        pretty_print_tree_path(),
                        orig_dep.ident,
                        parent_dep.map_or("none".to_string(), |d| d.ident.clone())
                    ));
                }
            } else {
                let parent = &tree.nodes[parent_id];

                let hoisted_from = parent.hoisted_from.get(&node.name);
                let original_hoisted_to = node.hoisted_to.get(&orig_dep.name);

                let pretty_hoisted_from = hoisted_from
                    .map(|h| format!(" hoisted from {}", h.join(", ")))
                    .unwrap_or_default();
                let pretty_original_hoisted_to = original_hoisted_to
                    .map(|h| format!(" hoisted to {}", h))
                    .unwrap_or_default();
                let pretty_node_path = format!("{}{}", pretty_print_tree_path(), pretty_hoisted_from);

                if dep_id.is_none() {
                    log.push(format!(
                        "{} - broken require promise: no required dependency {}{}found",
                        pretty_node_path,
                        orig_dep.name,
                        pretty_original_hoisted_to
                    ));
                } else {
                    let dep = &tree.nodes[*dep_id.unwrap()];
                    if dep.ident != orig_dep.ident {
                        log.push(format!(
                            "{} - broken require promise for {}{}: expected {}, but found: {}",
                            pretty_node_path,
                            orig_dep.name,
                            pretty_original_hoisted_to,
                            orig_dep.ident,
                            dep.ident
                        ));
                    }
                }
            }
        }

        parents.insert(node_id);

        for dep_id in node.dependencies.values() {
            let dep = &tree.nodes[*dep_id];
            if !node.peer_names.contains(&dep.name) {
                check_node(tree, *dep_id, cloned_dep_ids.clone(), node_id, log, seen_nodes, parents);
            }
        }

        parents.remove(&node_id);
    }

    let root_deps = tree.nodes[tree.root].dependencies.clone();
    check_node(tree, tree.root, root_deps, tree.root, &mut log, &mut seen_nodes, &mut parents);

    if log.is_empty() {
        None
    } else {
        Some(log.join("\n"))
    }
}

/// Creates a clone of package tree with extra fields used for hoisting purposes.
///
/// # Arguments
/// * `tree` - package tree clone
fn clone_tree(tree: HoisterTree, options: &InternalHoistOptions) -> HoisterWorkTree {
    let work_nodes: Vec<_> = tree.nodes.iter().map(|node| {
        let hoist_priority = node.hoist_priority.unwrap_or(0);
        let dependency_kind = node.dependency_kind.unwrap_or(HoisterDependencyKind::Regular);

        let dependencies: IndexMap<_, _> = node.dependencies.iter()
            .map(|&id| (tree.nodes[id].name.clone(), id))
            .collect();

        HoisterWorkNode {
            id: node.id,
            name: node.name.clone(),
            references: std::iter::once(node.reference.clone()).collect(),
            locator: make_locator(&node.ident_name, &node.reference),
            ident: make_ident(&node.ident_name, &node.reference),
            dependencies: dependencies.clone(),
            original_dependencies: dependencies,
            hoisted_dependencies: IndexMap::new(),
            peer_names: node.peer_names.clone(),
            reasons: IndexMap::new(),
            decoupled: true,
            is_hoist_border: false,
            hoist_priority,
            dependency_kind,
            hoisted_from: IndexMap::new(),
            hoisted_to: IndexMap::new(),
        }
    }).collect();

    let mut work_tree = HoisterWorkTree {
        nodes: work_nodes,
        root: tree.root,
    };

    for idx in 0..work_tree.nodes.len() {
        let work_node = &work_tree.nodes[idx];
        let dependencies_nm_hoisting_limits = options.hoisting_limits.get(&work_node.locator);

        let deps: Vec<_> = work_node.dependencies.values().cloned().collect();
        for dependency in deps {
            let is_hoist_border = dependencies_nm_hoisting_limits
                .map_or(false, |limits| limits.contains(&work_tree.nodes[dependency].name));

            // Mael: I noticed when refactoring from a tree to a flat array that
            // we only used to set the isHoistBorder flag the first time we see
            // the dependency node (because we were only setting the flag when the
            // node was being created). I suppose this was a mistake and the
            // package should be marked an hoist border if any of its parents
            // declare it as such; to confirm with @larixer?
            work_tree.nodes[dependency].is_hoist_border |= is_hoist_border;
        }

        let mut seen_coupled_nodes = IndexSet::new();

        fn mark_node_coupled(
            work_nodes: &mut [HoisterWorkNode],
            id: usize,
            seen_coupled_nodes: &mut IndexSet<usize>,
        ) {
            if seen_coupled_nodes.contains(&id) {
                return;
            }

            seen_coupled_nodes.insert(id);

            work_nodes[id].decoupled = false;

            let deps: Vec<_> = work_nodes[id].dependencies.values()
                .filter(|&&dep_id| !work_nodes[id].peer_names.contains(&work_nodes[dep_id].name))
                .cloned()
                .collect();

            for dep_id in deps {
                mark_node_coupled(work_nodes, dep_id, seen_coupled_nodes);
            }
        }

        mark_node_coupled(&mut work_tree.nodes, idx, &mut seen_coupled_nodes);
    }

    work_tree
}

fn get_ident_name(locator: &HoisterLocator) -> String {
    let at_idx = locator.find('@').unwrap_or(0) + 1;
    locator[..at_idx - 1].to_string()
}

/// Creates a clone of hoisted package tree with extra fields removed
///
/// # Arguments
/// * `tree` - stripped down hoisted package tree clone
fn shrink_tree(tree: &HoisterWorkTree) -> HoisterResult {
    let root_node = &tree.nodes[tree.root];

    let mut tree_copy = HoisterResult {
        name: root_node.name.clone(),
        ident_name: get_ident_name(&root_node.locator),
        references: root_node.references.clone(),
        dependencies: Vec::new(),
    };

    let mut seen_nodes = IndexSet::new();
    seen_nodes.insert(tree.root);

    fn add_node(
        tree: &HoisterWorkTree,
        node_id: usize,
        parent_work_node_id: usize,
        parent_node: &mut HoisterResult,
        seen_nodes: &mut IndexSet<usize>,
    ) {
        let is_seen = seen_nodes.contains(&node_id);
        let node = &tree.nodes[node_id];

        let mut result_node = if parent_work_node_id == node_id {
            parent_node.clone()
        } else {
            HoisterResult {
                name: node.name.clone(),
                ident_name: get_ident_name(&node.locator),
                references: node.references.clone(),
                dependencies: Vec::new(),
            }
        };

        if !is_seen {
            seen_nodes.insert(node_id);

            let deps: Vec<_> = node.dependencies.iter()
                .filter(|(name, _)| !node.peer_names.contains(*name))
                .map(|(_, &id)| id)
                .collect();

            for dep_id in deps {
                add_node(tree, dep_id, node_id, &mut result_node, seen_nodes);
            }

            seen_nodes.shift_remove(&node_id);
        }

        parent_node.dependencies.push(Box::new(result_node));
    }

    let deps: Vec<_> = root_node.dependencies.values().cloned().collect();
    for dep_id in deps {
        add_node(tree, dep_id, tree.root, &mut tree_copy, &mut seen_nodes);
    }

    tree_copy
}

/// Builds mapping, where key is an alias + dependent package ident and the value is the list of
/// parent package idents who depend on this package.
///
/// # Arguments
/// * `root_node` - package tree root node
///
/// # Returns
/// preference map
fn build_preference_map(tree: &HoisterWorkTree, root_node_id: usize) -> PreferenceMap {
    let mut preference_map = PreferenceMap::new();

    fn get_preference_key(node: &HoisterWorkNode) -> String {
        format!("{}@{}", node.name, node.ident)
    }

    fn get_or_create_preference_entry<'a>(
        preference_map: &'a mut PreferenceMap,
        node: &HoisterWorkNode,
    ) -> &'a mut PreferenceEntry {
        let key = get_preference_key(node);

        preference_map.entry(key).or_insert_with(|| PreferenceEntry {
            dependents: IndexSet::new(),
            peer_dependents: IndexSet::new(),
            hoist_priority: 0,
        })
    }

    let mut seen_nodes = IndexSet::new();
    seen_nodes.insert(tree.root);

    fn add_dependent(
        tree: &HoisterWorkTree,
        dependent_id: usize,
        node_id: usize,
        preference_map: &mut PreferenceMap,
        seen_nodes: &mut IndexSet<usize>,
    ) {
        let dependent = &tree.nodes[dependent_id];
        let node = &tree.nodes[node_id];

        let is_seen = seen_nodes.contains(&node_id);

        let entry = get_or_create_preference_entry(preference_map, node);
        entry.dependents.insert(dependent.ident.clone());

        if !is_seen {
            seen_nodes.insert(node_id);

            for dep_id in node.dependencies.values() {
                let dep = &tree.nodes[*dep_id];

                let entry = get_or_create_preference_entry(preference_map, dep);
                entry.hoist_priority = entry.hoist_priority.max(dep.hoist_priority);

                if node.peer_names.contains(&dep.name) {
                    entry.peer_dependents.insert(node.ident.clone());
                } else {
                    add_dependent(tree, node_id, *dep_id, preference_map, seen_nodes);
                }
            }
        }
    }

    let root_node = &tree.nodes[root_node_id];

    for dep_id in root_node.dependencies.values() {
        let dep = &tree.nodes[*dep_id];

        if !root_node.peer_names.contains(&dep.name) {
            add_dependent(tree, root_node_id, *dep_id, &mut preference_map, &mut seen_nodes);
        }
    }

    preference_map
}

fn pretty_print_locator(locator: Option<&HoisterLocator>) -> String {
    let locator = match locator {
        Some(l) => l,
        None => return "none".to_string(),
    };

    let idx = locator.find('@').unwrap_or(0) + 1;

    let mut name = locator[..idx - 1].to_string();
    if name.ends_with("$wsroot$") {
        name = format!("wh:{}", name.replace("$wsroot$", ""));
    }

    let reference = &locator[idx..];
    if reference.is_empty() {
        return name;
    }
    if reference == "workspace:." {
        return ".".to_string();
    }

    let source_version = reference.split('#').nth(1).unwrap_or(reference);
    let mut version = source_version.replace("npm:", "");

    if reference.starts_with("virtual") {
        name = format!("v:{}", name);
    }

    if version.starts_with("workspace") {
        name = format!("w:{}", name);
        version = String::new();
    }

    format!("{}{}", name, if version.is_empty() { String::new() } else { format!("@{}", version) })
}

const MAX_NODES_TO_DUMP: usize = 50000;

/// Pretty-prints dependency tree in the `yarn why`-like format
///
/// The function is used for troubleshooting purposes only.
///
/// # Arguments
/// * `pkg` - node_modules tree
///
/// # Returns
/// sorted node_modules tree
fn dump_dep_tree(tree: &HoisterWorkTree) -> String {
    let mut node_count = 0;

    fn dump_package(
        tree: &HoisterWorkTree,
        pkg_id: usize,
        parents: &mut IndexSet<usize>,
        prefix: &str,
        node_count: &mut usize,
    ) -> String {
        if *node_count > MAX_NODES_TO_DUMP || parents.contains(&pkg_id) {
            return String::new();
        }

        *node_count += 1;
        parents.insert(pkg_id);

        let pkg = &tree.nodes[pkg_id];

        let mut dependencies: Vec<_> = pkg.dependencies.values()
            .cloned()
            .collect();

        dependencies.sort_by(|&n_id1, &n_id2| {
            let n1 = &tree.nodes[n_id1];
            let n2 = &tree.nodes[n_id2];
            n1.name.cmp(&n2.name)
        });

        let mut str = String::new();

        for (idx, dep_id) in dependencies.iter().enumerate() {
            let dep = &tree.nodes[*dep_id];
            if pkg.peer_names.contains(&dep.name) {
                continue;
            }

            let reason = pkg.reasons.get(&dep.name);
            let ident_name = get_ident_name(&dep.locator);

            let marker = if parents.contains(dep_id) { ">" } else { "" };
            let alias = if ident_name != dep.name { format!("a:{}:", dep.name) } else { String::new() };
            let locator = pretty_print_locator(Some(&dep.locator));
            let reason_str = reason.map(|r| format!(" {}", r)).unwrap_or_default();

            str.push_str(&format!(
                "{}{}{}{}{}{}\n",
                prefix,
                if idx < dependencies.len() - 1 { "├─" } else { "└─" },
                marker,
                alias,
                locator,
                reason_str
            ));

            let new_prefix = format!(
                "{}{}",
                prefix,
                if idx < dependencies.len() - 1 { "│ " } else { "  " }
            );

            str.push_str(&dump_package(tree, *dep_id, parents, &new_prefix, node_count));
        }

        parents.remove(&pkg_id);

        str
    }

    let mut parents = IndexSet::new();
    let mut tree_dump = dump_package(tree, tree.root, &mut parents, "", &mut node_count);

    if node_count > MAX_NODES_TO_DUMP {
        tree_dump.push_str("\nTree is too large, part of the tree has been dumped.\n");
    }

    tree_dump
}
