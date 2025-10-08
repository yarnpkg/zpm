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

#[cli::command]
#[cli::path("npm", "whoami")]
#[cli::category("Npm-related commands")]
#[cli::description("Get the current user's npm token")]
pub struct Whoami {
    #[cli::option("-s,--scope")]
    #[cli::description("Get the token for a given scope")]
    scope: Option<String>,

    #[cli::option("--publish", default = false)]
    #[cli::description("Login to the publish registry")]
    publish: bool,
}

impl Whoami {
    #[tokio::main]
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
