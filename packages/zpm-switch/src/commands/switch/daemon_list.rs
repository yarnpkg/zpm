use clipanion::cli;
use zpm_parsers::JsonDocument;
use zpm_utils::{tree, AbstractValue, ToFileString};

use crate::{daemons, errors::Error};

/// List live Yarn daemons
#[cli::command]
#[cli::path("switch", "daemon")]
#[cli::category("Daemon management")]
#[derive(Debug)]
pub struct DaemonListCommand {
    /// Output the list as JSON
    #[cli::option("--json", default = false)]
    json: bool,
}

impl DaemonListCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let daemons
            = daemons::list_live_daemons()?;

        if self.json {
            let json_output: Vec<_> = daemons
                .iter()
                .map(|d| serde_json::json!({
                    "cwd": d.project_cwd.to_file_string(),
                    "version": d.yarn_version.to_file_string(),
                    "pid": d.pid,
                    "port": d.port,
                }))
                .collect();

            println!("{}", JsonDocument::to_string_pretty(&json_output)?);
            return Ok(());
        }

        if daemons.is_empty() {
            println!("No live daemons found.");
            return Ok(());
        }

        let nodes: Vec<_> = daemons
            .iter()
            .map(|d| tree::Node {
                label: None,
                value: Some(AbstractValue::new(d.project_cwd.clone())),
                children: Some(tree::TreeNodeChildren::Map(tree::Map::from([
                    ("version".to_string(), tree::Node {
                        label: Some("Yarn version".to_string()),
                        value: Some(AbstractValue::new(d.yarn_version.clone())),
                        children: None,
                    }),
                    ("pid".to_string(), tree::Node {
                        label: Some("PID".to_string()),
                        value: Some(AbstractValue::new(d.pid as u64)),
                        children: None,
                    }),
                    ("port".to_string(), tree::Node {
                        label: Some("Port".to_string()),
                        value: Some(AbstractValue::new(d.port as u64)),
                        children: None,
                    }),
                ]))),
            })
            .collect();

        let root = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(nodes)),
        };

        print!("{}", root.to_string());

        Ok(())
    }
}
