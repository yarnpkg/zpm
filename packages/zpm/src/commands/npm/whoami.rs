use clipanion::cli;
use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::FromFileString;

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions, NpmHttpParams, get_authorization, get_registry},
    project::Project,
};

/// Print the npm username for the current authentication settings.
///
/// With `-s,--scope`, Yarn checks the registry and credentials configured for that scope.
///
/// With `--publish`, Yarn checks the publish registry configured through `npmPublishRegistry` or the regular npm registry fallback.
///
#[cli::command]
#[cli::path("npm", "whoami")]
#[cli::category("Npm-related commands")]
pub struct Whoami {
    /// Query credentials for the registry configured for this scope
    #[cli::option("-s,--scope")]
    scope: Option<String>,

    /// Query credentials for the publish registry
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
            = get_authorization(&GetAuthorizationOptions {
                configuration: &project.config,
                http_client: &project.http_client,
                registry: &registry,
                ident: ident.as_ref(),
                auth_mode: AuthorizationMode::AlwaysAuthenticate,
                allow_oidc: false,
            }).await?;

        let Some(authorization) = authorization else {
            return Err(Error::AuthenticationError(
                "No authentication configured".to_string()
            ));
        };

        let response = http_npm::get(&NpmHttpParams {
            http_client: &project.http_client,
            registry: &registry,
            path: "/-/whoami",
            authorization: Some(&authorization),
            otp: None,
        }).await?;

        #[derive(Deserialize)]
        struct WhoamiResponse {
            username: String,
        }

        let whoami: WhoamiResponse
            = JsonDocument::hydrate_from_slice(&response[..])?;

        println!("{}", whoami.username);

        Ok(())
    }
}
