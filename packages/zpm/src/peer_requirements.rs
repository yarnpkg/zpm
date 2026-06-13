//! Mirrors berry's `peerRequirementNodes` + `peerWarnings` from
//! `yarnpkg-core/sources/Project.ts`. Berry builds these during
//! virtualization; zpm's tree resolver does that earlier, so we
//! reconstruct them here by walking `install_state`.

use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{Descriptor, Ident, Locator, PeerRange, Reference};
use zpm_utils::{Hash64, ToFileString, ToHumanString};

use crate::{install::InstallState, project::Project};

#[derive(Debug, Clone)]
pub struct PeerRequestNode {
    pub requester: Locator,
    pub range: zpm_semver::Range,
    /// Keyed by physical locator for stable display order.
    pub children: BTreeMap<Locator, PeerRequestNode>,
}

#[derive(Debug, Clone)]
pub struct PeerRequirementNode {
    pub hash: String,
    /// The package supposed to provide the peer dep. For a workspace's
    /// direct deps this is the workspace itself; for transitive
    /// virtualization it's the immediate virtualized parent.
    pub subject: Locator,
    pub ident: Ident,
    /// `None` means no provider was found (missing peer).
    pub provided: Option<Descriptor>,
    pub provided_version: Option<zpm_semver::Version>,
    pub requests: BTreeMap<Locator, PeerRequestNode>,
    /// True when `subject` is a workspace locator. Berry uses this
    /// to split project-level mismatches (shown inline) from
    /// transitive ones (rolled up under an "explain peer-requirements"
    /// CTA).
    pub is_root: bool,
}

#[derive(Debug, Clone)]
pub enum PeerWarningKind {
    NodeNotProvided,
    NodeNotCompatible {
        /// Simplified intersection, or `None` when ranges don't overlap.
        range: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct PeerWarning {
    pub hash: String,
    pub kind: PeerWarningKind,
}

pub struct PeerData {
    pub nodes: BTreeMap<String, PeerRequirementNode>,
    pub warnings: Vec<PeerWarning>,
}

fn untype_ident(ident: &Ident) -> Option<Ident> {
    if ident.scope() != Some("@types") {
        return None;
    }

    let name = ident.name();

    if let Some((scope, name)) = name.split_once("__") {
        Some(Ident::new(format!("@{scope}/{name}")))
    } else {
        Some(Ident::new(name))
    }
}

pub fn compute(project: &Project, install_state: &InstallState) -> PeerData {
    let tree = &install_state.resolution_tree;

    let mut reverse_deps: BTreeMap<Locator, BTreeSet<Locator>> = BTreeMap::new();
    for (parent, resolution) in &tree.locator_resolutions {
        for desc in resolution.dependencies.values() {
            if let Some(loc) = tree.descriptor_to_locator.get(desc) {
                reverse_deps.entry(loc.clone()).or_default().insert(parent.clone());
            }
        }
    }

    let workspace_locators: BTreeSet<Locator> = project.workspaces.iter()
        .flat_map(|w| [w.locator(), w.locator_path()])
        .collect();

    // Peer slots propagate upward through virtualization: a chain
    // `lvl0 → lvl1 → lvl2` all peer-requesting `no-deps` shares the
    // same slot originating at the workspace. Berry gives every node
    // in the chain the same `subject` so warnings collapse into one
    // line; we mirror that by walking up to the top of the chain.
    let find_subject = |requester: &Locator| -> Locator {
        let mut visited: BTreeSet<Locator> = BTreeSet::new();
        let mut current = requester.clone();
        loop {
            if !visited.insert(current.clone()) {
                return current;
            }
            let Some(parents) = reverse_deps.get(&current) else {
                return current;
            };
            // Workspace parent owns the peer slot — stop here.
            if let Some(ws) = parents.iter().find(|p| workspace_locators.contains(p)) {
                return ws.clone();
            }
            let Some(next) = parents.iter().next().cloned() else {
                return current;
            };
            current = next;
        }
    };

    // Dedup grouping by physical subject so virtual instances of the
    // same "workspace provides X" don't fragment into separate entries.
    let subject_key = |loc: &Locator| -> Locator { loc.physical_locator() };

    struct Row {
        subject: Locator,
        subject_key: Locator,
        requester: Locator,
        ident: Ident,
        range: zpm_semver::Range,
        provided: Option<Descriptor>,
    }
    let mut rows: Vec<Row> = Vec::new();

    for (virtual_locator, resolution) in &tree.locator_resolutions {
        if !virtual_locator.reference.is_virtual_reference() {
            continue;
        }

        let physical = virtual_locator.physical_locator();
        let Some(phys_resolution) = install_state.normalized_resolutions.get(&physical) else {
            continue;
        };

        for (peer_ident, peer_range) in &phys_resolution.peer_dependencies {
            // Optional peers are silent in berry. Auto-injected
            // `@types/*` peers are an implementation detail, but
            // explicitly configured type peers should still be reported.
            if phys_resolution.optional_peer_dependencies.contains(peer_ident) {
                continue;
            }
            if let Some(untyped_ident) = untype_ident(peer_ident) && phys_resolution.peer_dependencies.contains_key(&untyped_ident) {
                continue;
            }

            let range = match peer_range {
                PeerRange::Semver(p) => p.range.clone(),
                // Workspace-protocol peer ranges aren't range-checkable;
                // berry drops them and so do we.
                _ => continue,
            };

            let provided = resolution.dependencies.get(peer_ident).cloned();

            let subject = find_subject(virtual_locator);

            rows.push(Row {
                subject_key: subject_key(&subject),
                subject,
                requester: virtual_locator.clone(),
                ident: peer_ident.clone(),
                range,
                provided,
            });
        }
    }

    if rows.is_empty() {
        return PeerData {
            nodes: BTreeMap::new(),
            warnings: Vec::new(),
        };
    }

    #[derive(Default)]
    struct Bucket {
        subject_repr: Option<Locator>,
        provided: Option<Descriptor>,
        /// Keyed by physical locator so duplicate virtual instances collapse.
        requesters: BTreeMap<Locator, Row>,
    }

    let mut buckets: BTreeMap<(Locator, Ident), Bucket> = BTreeMap::new();
    for row in rows {
        let key = (row.subject_key.clone(), row.ident.clone());
        let bucket = buckets.entry(key).or_default();
        if bucket.subject_repr.is_none() {
            bucket.subject_repr = Some(row.subject.clone());
        }
        if bucket.provided.is_none() {
            bucket.provided = row.provided.clone();
        }
        bucket.requesters.entry(row.requester.physical_locator()).or_insert(row);
    }

    let mut nodes: BTreeMap<String, PeerRequirementNode> = BTreeMap::new();
    let mut warnings: Vec<PeerWarning> = Vec::new();

    for ((subject_key_loc, ident), bucket) in buckets {
        let subject = bucket.subject_repr.unwrap_or(subject_key_loc.clone());
        let provided_locator = bucket.provided.as_ref()
            .and_then(|d| tree.descriptor_to_locator.get(d));
        let provided_version = provided_locator
            .and_then(|l| tree.locator_resolutions.get(l))
            .map(|r| r.version.clone());

        let is_root = matches!(
            subject.reference.physical_reference(),
            Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_) | Reference::Link(_)
        ) || workspace_locators.contains(&subject);

        // Find each requester's nearest in-bucket ancestor so nested
        // requesters land under the right parent. Every virtual locator
        // has exactly one virtualizing parent, so the walk is a chain.
        let physical_requesters: BTreeSet<Locator> = bucket.requesters.keys().cloned().collect();
        let mut nearest_request_parent: BTreeMap<Locator, Option<Locator>> = BTreeMap::new();

        for requester_phys in &physical_requesters {
            let Some(row) = bucket.requesters.get(requester_phys) else { continue };
            let mut current: &Locator = &row.requester;
            let mut parent: Option<Locator> = None;
            let mut visited: BTreeSet<&Locator> = BTreeSet::new();
            visited.insert(current);
            // Cap guards against pathological cycles; in practice the
            // walk is O(depth-in-virtual-chain).
            for _ in 0..256 {
                let Some(parents) = reverse_deps.get(current) else { break };
                let Some(next) = parents.iter().next() else { break };
                if !visited.insert(next) {
                    break;
                }
                let next_phys = next.physical_locator();
                if &next_phys != requester_phys && physical_requesters.contains(&next_phys) {
                    parent = Some(next_phys);
                    break;
                }
                current = next;
            }
            nearest_request_parent.insert(requester_phys.clone(), parent);
        }

        let mut request_nodes: BTreeMap<Locator, PeerRequestNode> = BTreeMap::new();
        for (phys, row) in &bucket.requesters {
            request_nodes.insert(phys.clone(), PeerRequestNode {
                requester: row.requester.physical_locator(),
                range: row.range.clone(),
                children: BTreeMap::new(),
            });
        }

        // Sort deepest-first so parents are still in the map when we
        // move children into them. Guarded by a visited set since the
        // chain isn't strictly a forest.
        let mut order: Vec<Locator> = bucket.requesters.keys().cloned().collect();
        order.sort_by_cached_key(|loc| {
            let mut depth = 0usize;
            let mut visited: BTreeSet<&Locator> = BTreeSet::new();
            let mut cur: &Locator = loc;
            visited.insert(cur);
            while let Some(Some(parent)) = nearest_request_parent.get(cur) {
                if !visited.insert(parent) {
                    break;
                }
                depth += 1;
                cur = parent;
            }
            std::cmp::Reverse(depth)
        });

        let mut direct_requests: BTreeMap<Locator, PeerRequestNode> = BTreeMap::new();

        for phys in order {
            let Some(node) = request_nodes.remove(&phys) else { continue };
            match nearest_request_parent.get(&phys).cloned().flatten() {
                Some(parent_phys) if request_nodes.contains_key(&parent_phys) => {
                    if let Some(parent_node) = request_nodes.get_mut(&parent_phys) {
                        parent_node.children.insert(phys, node);
                    }
                },
                _ => {
                    direct_requests.insert(phys, node);
                },
            }
        }

        // Match berry's `pXXXXXX` six-hex pattern so existing CTA copy
        // and cross-tool diagnostics line up.
        let first_requester_repr = direct_requests.keys().next()
            .cloned()
            .unwrap_or_else(|| subject.clone());
        let hash_input = format!(
            "{}|{}|{}",
            subject_key_loc.to_file_string(),
            ident.to_file_string(),
            first_requester_repr.to_file_string(),
        );
        let hash = format!("p{}", Hash64::from_data(&hash_input).mini());

        let warning_kind: Option<PeerWarningKind> = match (&bucket.provided, &provided_version) {
            (None, _) => Some(PeerWarningKind::NodeNotProvided),
            (Some(_), Some(version)) => {
                let any_unsatisfied = collect_all_requests(&direct_requests)
                    .into_iter()
                    .any(|req| !req.range.check(version));
                if any_unsatisfied {
                    let range = simplify_ranges_for_display(&direct_requests, version);
                    Some(PeerWarningKind::NodeNotCompatible { range })
                } else {
                    None
                }
            },
            // Descriptor present but unresolved — treat as missing
            // rather than fabricating a satisfied requirement.
            (Some(_), None) => Some(PeerWarningKind::NodeNotProvided),
        };

        if let Some(kind) = warning_kind {
            warnings.push(PeerWarning {
                hash: hash.clone(),
                kind,
            });
        }

        nodes.insert(hash.clone(), PeerRequirementNode {
            hash,
            subject,
            ident,
            provided: bucket.provided,
            provided_version,
            requests: direct_requests,
            is_root,
        });
    }

    PeerData {
        nodes,
        warnings,
    }
}

/// Each request (direct + transitive) paired with the direct
/// request it descends from. Mirrors berry's `allPeerRequestsWithRoot`.
pub fn iter_requests_with_root<'a>(
    direct: &'a BTreeMap<Locator, PeerRequestNode>,
) -> Vec<(&'a PeerRequestNode, &'a PeerRequestNode)> {
    let mut out = Vec::new();
    for root in direct.values() {
        push_with_root(root, root, &mut out);
    }
    out
}

fn push_with_root<'a>(
    node: &'a PeerRequestNode,
    root: &'a PeerRequestNode,
    out: &mut Vec<(&'a PeerRequestNode, &'a PeerRequestNode)>,
) {
    out.push((node, root));
    for child in node.children.values() {
        push_with_root(child, root, out);
    }
}

fn collect_all_requests<'a>(direct: &'a BTreeMap<Locator, PeerRequestNode>) -> Vec<&'a PeerRequestNode> {
    iter_requests_with_root(direct).into_iter().map(|(req, _)| req).collect()
}

/// Best-effort stand-in for berry's `semverUtils.simplifyRanges`.
/// If one requested range subsumes the others, use it as the canonical
/// description; otherwise join with ` && `, or return `None` when the
/// ranges are disjoint (nothing meaningful to print).
fn simplify_ranges_for_display(
    direct: &BTreeMap<Locator, PeerRequestNode>,
    _resolved_version: &zpm_semver::Version,
) -> Option<String> {
    let mut ranges: Vec<&zpm_semver::Range> = collect_all_requests(direct).into_iter().map(|r| &r.range).collect();
    ranges.sort_by_key(|r| r.to_file_string());
    ranges.dedup_by(|a, b| a.to_file_string() == b.to_file_string());

    // Any range accepted by all others *is* the intersection.
    let subsuming_exact = ranges.iter().find(|candidate| {
        let Some(version) = candidate.exact_version() else {
            return false;
        };
        ranges.iter().all(|other| other.check(&version))
    });

    if let Some(r) = subsuming_exact {
        return Some(r.to_file_string());
    }

    // Disjointness probe: if no candidate is accepted by every range,
    // there's no overlap.
    let representative_versions: Vec<zpm_semver::Version> = ranges.iter()
        .filter_map(|r| r.exact_version())
        .collect();

    if !representative_versions.is_empty() {
        let any_overlap = representative_versions.iter().any(|v| ranges.iter().all(|r| r.check(v)));
        if !any_overlap {
            return None;
        }
    }

    Some(ranges.iter().map(|r| r.to_file_string()).collect::<Vec<_>>().join(" && "))
}

/// Renders the per-requester label for a request, annotating with
/// `(via <root>)` when it flows through another direct requester.
pub fn render_requester_label(request: &PeerRequestNode, root: &PeerRequestNode) -> String {
    if std::ptr::eq(request, root) {
        request.requester.ident.to_print_string()
    } else {
        format!(
            "{} (via {})",
            request.requester.ident.to_print_string(),
            root.requester.ident.to_print_string(),
        )
    }
}
