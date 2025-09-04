use clipanion::cli;
use colored::Colorize;
use convert_case::{Case, Casing};
use zpm_utils::ToHumanString;

use crate::{error::Error, project::Project};

#[cli::command]
#[cli::path("config")]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
#[cli::description("List the project's configuration values")]
pub struct Config {
}

impl Config {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        // let project_settings
        //     = project.config.settings.to_btree_map();

        // let mut nodes = vec![];

        // let value_header
        //     = "Value:".bold().to_string();

        // for (key, value) in project_settings {
        //     let camel_key
        //         = key.to_case(Case::Camel)
        //             .truecolor(153, 204, 255)
        //             .to_string();

        //     let value_node = Node {
        //         label: format!("{} {}", value_header, value.to_print_string()),
        //         children: vec![],
        //     };

        //     nodes.push(Node {
        //         label: camel_key,
        //         children: vec![value_node],
        //     });
        // }

        // let tree = Node {
        //     label: "".to_string(),
        //     children: nodes,
        // };

        // print!("{}", tree.to_string());

        Ok(())
    }
}
