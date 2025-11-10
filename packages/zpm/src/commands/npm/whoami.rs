use clipanion::cli;
use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::FromFileString;

use crate::{
    error::Error,
    http_npm::{self, get_authorization, get_registry, AuthorizationMode, NpmHttpParams},
    project::Project,
};

/// Print the username associated with the current authentication settings to the standard output.
///
/// When using `-s,--scope`, the username printed will be the one that matches the authentication settings of the registry associated with the given scope (those settings can be overriden using the `npmRegistries` map, and the registry associated with the scope is configured via the `npmScopes` map).
///
/// When using `--publish`, the registry we'll select will by default be the one used when publishing packages (`publishConfig.registry` or `npmPublishRegistry` if available, otherwise we'll fallback to the regular `npmRegistryServer`).
///
#[cli::command]
#[cli::path("npm", "whoami")]
#[cli::category("Npm-related commands")]
pub struct Whoami {
    /// Get the username for a given scope
    #[cli::option("-s,--scope")]
    scope: Option<String>,

    /// Get the username for the publish registry
    #[cli::option("--publish", default = false)]
    publish: bool,
}

impl Whoami {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let registry
            = get_registry(&project.config, self.scope.as_deref(), self.publish)?
                .to_string();

        let ident
            = self.scope.as_ref().map(|s| Ident::from_file_string(format!("@{}/*", s).as_str()).unwrap());

        let authorization
            = get_authorization(&project.config, &registry, ident.as_ref(), AuthorizationMode::AlwaysAuthenticate);

        let response = http_npm::get(&NpmHttpParams {
            http_client: &project.http_client,
            registry: &registry,
            path: "/-/whoami",
            authorization: authorization.as_deref(),
            headers: None,
        }).await?;

        #[derive(Deserialize)]
        struct WhoamiResponse {
            username: String,
        }

        let body
            = response.text().await?;
        let whoami: WhoamiResponse
            = JsonDocument::hydrate_from_str(&body)?;

        println!("{}", whoami.username);

        Ok(())
    }
}
