use clipanion::cli;
use indexmap::IndexMap;
use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, DescriptorResolution, Locator};
use zpm_utils::{tree, AbstractValue, DataType, FromFileString, RawString};

use crate::{
    error::Error,
    http_npm::{self, NpmHttpParams},
    project::Project,
};

use super::registry_auth;

/// List staged package versions awaiting approval.
///
/// This command lists all staged versions on the configured npm publish registry.
/// When a package name is provided, only staged versions of that package are listed.
///
#[cli::command]
#[cli::path("npm", "stage", "list")]
#[cli::category("Npm-related commands")]
pub struct List {
    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Only list staged versions of this package
    package: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedPackage {
    id: String,
    package_name: String,
    version: String,
    tag: String,
    created_at: String,
}

#[derive(Deserialize)]
struct StageListResponse {
    items: Vec<StagedPackage>,
    total: usize,
}

impl List {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let (registry, authorization)
            = registry_auth(&project).await?;

        let mut items = Vec::new();
        let mut page = 0;
        let per_page = 100;

        loop {
            let mut query
                = url::form_urlencoded::Serializer::new(String::new());

            query.append_pair("page", &page.to_string());
            query.append_pair("perPage", &per_page.to_string());

            if let Some(package) = &self.package {
                query.append_pair("package", package);
            }

            let response = http_npm::get(&NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry,
                path: &format!("/-/stage?{}", query.finish()),
                authorization: authorization.as_deref(),
                otp: None,
            }).await?;

            let response: StageListResponse
                = JsonDocument::hydrate_from_slice(&response)?;

            let page_size
                = response.items.len();

            items.extend(response.items);

            if items.len() >= response.total || page_size < per_page {
                break;
            }

            page += 1;
        }

        if items.is_empty() {
            if !self.json {
                match &self.package {
                    Some(package) => println!("No staged versions found for package {}", package),
                    None => println!("No staged packages found"),
                }
            }

            return Ok(());
        }

        if !self.json {
            println!("The following packages are awaiting approval. Use {} to approve them.\n", DataType::Code.colorize("yarn npm stage approve <stageId>"));
        }

        let mut nodes = Vec::new();

        for item in items {
            let descriptor_string
                = format!("{}@{}", item.package_name, item.tag);
            let descriptor
                = Descriptor::from_file_string(&descriptor_string)
                    .map_err(|_| Error::InvalidDescriptor(descriptor_string))?;
            let locator
                = Locator::from_file_string(&format!("{}@npm:{}", item.package_name, item.version))?;

            let children = IndexMap::from([
                ("ID".to_string(), tree::Node {
                    label: Some("ID".to_string()),
                    value: Some(AbstractValue::new(RawString::new(item.id))),
                    children: None,
                }),
                ("Staged".to_string(), tree::Node {
                    label: Some("Staged on".to_string()),
                    value: Some(AbstractValue::new(RawString::new(item.created_at))),
                    children: None,
                }),
            ]);

            nodes.push(tree::Node {
                label: None,
                value: Some(AbstractValue::new(DescriptorResolution::new(descriptor, locator))),
                children: Some(tree::TreeNodeChildren::Map(children)),
            });
        }

        let root = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(nodes)),
        };

        print!("{}", tree::TreeRenderer::new().render(&root, self.json));

        Ok(())
    }
}
