use clipanion::cli;
use zpm_utils::{tree, AbstractValue};

use crate::{links::list_links, errors::Error};

/// List all local links
#[cli::command]
#[cli::path("switch", "links")]
#[cli::category("Local Yarn development")]
#[derive(Debug)]
pub struct LinksListCommand {
}

impl LinksListCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let links
            = list_links()?;

        let link_nodes
            = links.into_iter()
                .map(|link| tree::Node {
                    label: None,
                    value: Some(AbstractValue::new(link.project_cwd.clone())),
                    children: Some(tree::TreeNodeChildren::Map(tree::Map::from([
                        ("linkTarget".to_string(), tree::Node {
                            label: Some("Binary path".to_string()),
                            value: Some(AbstractValue::new(link.link_target)),
                            children: None,
                        }),
                    ]))),
                })
                .collect::<Vec<_>>();

        let root_node = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(link_nodes)),
        };

        print!("{}", root_node.to_string());

        Ok(())
    }
}
