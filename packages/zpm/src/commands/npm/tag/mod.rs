use std::collections::BTreeMap;

use reqwest::StatusCode;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions, NpmHttpParams},
    project::Project,
};

pub mod add;
pub mod list;
pub mod remove;

fn encode_uri_component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

fn dist_tags_path(ident: &Ident, tag: Option<&str>) -> String {
    let mut path
        = format!("/-/package/{}/dist-tags", http_npm::get_ident_url(ident));

    if let Some(tag) = tag {
        path.push('/');
        path.push_str(&encode_uri_component(tag));
    }

    path
}

/// Returns the registry and credentials used to modify the dist-tags of the
/// given package. This is the registry the package would be published to: a
/// workspace's `publishConfig.registry` applies when the package is one of the
/// project's workspaces, otherwise the publish registry configured for its
/// scope or matching package rules.
async fn publish_registry_auth(project: &Project, ident: &Ident) -> Result<(String, Option<String>), Error> {
    let workspace_manifest
        = project.workspace_by_ident(ident).ok()
            .map(|workspace| &workspace.manifest)
            .filter(|manifest| manifest.name.as_ref() == Some(ident));

    let registry = match workspace_manifest {
        Some(manifest) => http_npm::get_publish_registry(&project.config, manifest)?,
        None => http_npm::get_registry_for_ident(&project.config, Some(ident), true)?,
    }.to_string();

    let authorization
        = http_npm::get_authorization(&GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry,
            ident: Some(ident),
            auth_mode: AuthorizationMode::AlwaysAuthenticate,
            allow_oidc: false,
        }).await?;

    Ok((registry, authorization))
}

async fn get_dist_tags(project: &Project, registry: &str, ident: &Ident, authorization: Option<&str>) -> Result<BTreeMap<String, String>, Error> {
    let response = http_npm::get(&NpmHttpParams {
        http_client: &project.http_client,
        registry,
        path: &dist_tags_path(ident, None),
        authorization,
        otp: None,
    }).await;

    let bytes = match response {
        Err(Error::HttpError {inner, ..}) if inner.status() == Some(StatusCode::NOT_FOUND)
            => return Err(Error::PackageNotFound(ident.clone())),

        response
            => response?,
    };

    Ok(JsonDocument::hydrate_from_slice(&bytes)?)
}
