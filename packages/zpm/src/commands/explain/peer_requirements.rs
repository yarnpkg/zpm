use clipanion::cli;
use zpm_utils::{DataType, ToHumanString, tree};

use crate::{
    error::Error,
    peer_requirements::{self, PeerRequestNode, PeerWarningKind},
    project::Project,
};

/// Explain a set of peer requirements
///
/// A peer requirement represents all peer requests that a subject must satisfy when providing a requested package to requesters.
///
/// When the hash argument is specified, this command prints a detailed explanation of the peer requirement corresponding to the hash and whether it is satisfied or not.
///
/// When used without arguments, this command lists all peer requirements and the corresponding hash that can be used to get detailed information about a given requirement.
///
/// **Note:** A hash is a seven-letter code consisting of the letter 'p' followed by six characters that can be obtained from peer dependency warnings or from the list of all peer requirements (`yarn explain peer-requirements`).
///
/// ## Examples
///
/// Explain the corresponding peer requirement for a hash
///
/// `yarn explain peer-requirements p8d4da8`
///
/// List all peer requirements
///
/// `yarn explain peer-requirements`
///
#[cli::command]
#[cli::path("explain", "peer-requirements")]
#[cli::category("Dependency management")]
pub struct ExplainPeerRequirements {
    /// The hash of the peer requirement to explain (e.g. `p1a4ed5`).
    /// Omit to list every peer requirement in the project.
    hash: Option<String>,
}

impl ExplainPeerRequirements {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        project.lazy_install().await?;

        let install_state = project
            .install_state
            .as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let data = peer_requirements::compute(&project, install_state);

        match &self.hash {
            Some(hash) => self.explain_one(&data, hash),
            None => self.explain_all(&data),
        }
    }

    fn explain_one(&self, data: &peer_requirements::PeerData, hash: &str) -> Result<(), Error> {
        let Some(node) = data.nodes.get(hash) else {
            return Err(Error::ConflictingOptions(format!(
                "No peer dependency requirements found for hash: \"{}\"", hash,
            )));
        };

        let warning = data.warnings.iter().find(|w| w.hash == node.hash);
        let mark = if warning.is_some() { "✘" } else { "✓" };

        println!(
            "Package {} is requested to provide {} by its descendants",
            node.subject.to_print_string(),
            node.ident.to_print_string(),
        );
        println!();

        let mut root_children: Vec<tree::Node<'_>> = Vec::new();
        root_children.push(tree::Node {
            label: Some(node.subject.to_print_string()),
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(
                node.requests.values().map(make_request_node).collect(),
            )),
        });

        let root = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(root_children)),
        };

        print!("{}", tree::TreeRenderer::new().render(&root, false));
        println!();

        match (&node.provided, &node.provided_version) {
            (None, _) | (Some(_), None) => {
                let problem = if warning.is_some() { "" } else { ", but all peer requests are optional" };
                println!(
                    "{} Package {} does not provide {}{}.",
                    mark,
                    node.subject.to_print_string(),
                    node.ident.to_print_string(),
                    problem,
                );
            },
            (Some(_), Some(version)) => {
                let trailer = if warning.is_some() {
                    "which does not satisfy all requests."
                } else {
                    "which satisfies all requests"
                };
                println!(
                    "{} Package {} provides {} with version {}, {}",
                    mark,
                    node.subject.to_print_string(),
                    node.ident.to_print_string(),
                    version.to_print_string(),
                    trailer,
                );

                if let Some(w) = warning {
                    if let PeerWarningKind::NodeNotCompatible { range } = &w.kind {
                        match range {
                            Some(r) => println!("  The combined requested range is {}", DataType::Range.colorize(r)),
                            None => println!("  Unfortunately, the requested ranges have no overlap"),
                        }
                    }
                }
            },
        }

        Ok(())
    }

    fn explain_all(&self, data: &peer_requirements::PeerData) -> Result<(), Error> {
        // Stable order: subject locator, then ident.
        let mut entries: Vec<&peer_requirements::PeerRequirementNode> = data.nodes.values().collect();
        entries.sort_by(|a, b| {
            use zpm_utils::ToFileString;
            (a.subject.to_file_string(), a.ident.to_file_string())
                .cmp(&(b.subject.to_file_string(), b.ident.to_file_string()))
        });

        for node in entries {
            if !node.is_root && !has_any_requests(node) {
                continue;
            }

            let warning = data.warnings.iter().find(|w| w.hash == node.hash);
            let mark = if warning.is_some() { "✘" } else { "✓" };

            let total_requests = total_request_count(node);
            let and_others = match total_requests {
                0 | 1 => String::new(),
                2 => " and 1 other dependency".to_string(),
                n => format!(" and {} other dependencies", n - 1),
            };

            // Falls back to the subject if no explicit request is
            // recorded (shouldn't happen for a legitimate node).
            let first_requester_label = node.requests.values().next()
                .map(|r| r.requester.to_print_string())
                .unwrap_or_else(|| node.subject.to_print_string());

            let message = match &node.provided {
                Some(_) => {
                    let version_str = node.provided_version.as_ref()
                        .map(|v| v.to_print_string())
                        .unwrap_or_else(|| DataType::Reference.colorize("0.0.0"));
                    format!(
                        "{} → {} {} provides {} ({}) to {}{}",
                        DataType::Code.colorize(&node.hash),
                        mark,
                        node.subject.to_print_string(),
                        node.ident.to_print_string(),
                        version_str,
                        first_requester_label,
                        and_others,
                    )
                },
                None => {
                    format!(
                        "{} → {} {} doesn't provide {} to {}{}",
                        DataType::Code.colorize(&node.hash),
                        mark,
                        node.subject.to_print_string(),
                        node.ident.to_print_string(),
                        first_requester_label,
                        and_others,
                    )
                },
            };

            println!("{}", message);
        }

        Ok(())
    }
}

fn make_request_node(request: &PeerRequestNode) -> tree::Node<'_> {
    let label = format!(
        "{} (wants {})",
        request.requester.to_print_string(),
        request.range.to_print_string(),
    );

    let children: Vec<tree::Node<'_>> = request.children.values()
        .map(make_request_node)
        .collect();

    tree::Node {
        label: Some(label),
        value: None,
        children: if children.is_empty() {
            None
        } else {
            Some(tree::TreeNodeChildren::Vec(children))
        },
    }
}

fn has_any_requests(node: &peer_requirements::PeerRequirementNode) -> bool {
    !node.requests.is_empty()
}

fn total_request_count(node: &peer_requirements::PeerRequirementNode) -> usize {
    fn rec(req: &PeerRequestNode) -> usize {
        1 + req.children.values().map(rec).sum::<usize>()
    }
    node.requests.values().map(rec).sum()
}
