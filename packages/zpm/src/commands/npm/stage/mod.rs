use std::str::FromStr;

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions},
    project::Project,
};

pub mod approve;
pub mod list;
pub mod reject;

#[derive(Debug)]
pub struct StageId(String);

impl FromStr for StageId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = value.len() == 36 && value.bytes().enumerate().all(|(index, byte)| {
            match index {
                8 | 13 | 18 | 23 => byte == b'-',
                _ => byte.is_ascii_hexdigit(),
            }
        });

        if !valid {
            return Err(Error::InvalidNpmStageId(value.to_string()));
        }

        Ok(Self(value.to_string()))
    }
}

async fn registry_auth(project: &Project) -> Result<(String, Option<String>), Error> {
    let registry
        = http_npm::get_registry(&project.config, None, true)?.to_string();

    let authorization
        = http_npm::get_authorization(&GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry,
            ident: None,
            auth_mode: AuthorizationMode::AlwaysAuthenticate,
            allow_oidc: false,
        }).await?;

    Ok((registry, authorization))
}
