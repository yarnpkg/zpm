use std::collections::BTreeMap;

use clipanion::cli;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use zpm_parsers::{JsonDocument, RawJsonValue};
use zpm_primitives::{split_ident_and_selector, Ident};
use zpm_utils::FromFileString;

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions, GetPackageMetadataParams, get_authorization, get_registry},
    project::Project,
};

/// Show information about a package
///
/// This command fetches package metadata from the configured npm registry and prints the selected version manifest as JSON.
///
/// The argument can be any of the following forms:
///
/// - `<name>` — return information about the version associated with the `latest` dist-tag.
///
/// - `<name>@<version>` — return information about the specified version.
///
/// - `<name>@<range>` — return information about the highest version that satisfies the range.
///
/// - `<name>@<tag>` — return information about the version associated with the given dist-tag.
///
#[cli::command]
#[cli::path("npm", "info")]
#[cli::category("Npm-related commands")]
pub struct Info {
    /// Keep the output in JSON format
    #[cli::option("--json", default = false)]
    _json: bool,

    /// Package selector (`name`, `name@version`, `name@range`, or `name@tag`)
    package: String,
}

impl Info {
    pub async fn execute(&self) -> Result<(), Error> {
        let project = Project::new(None).await?;

        let (ident, selector) = parse_package_arg(&self.package)?;

        let registry_base
            = get_registry(&project.config, ident.scope(), false)?
                .to_string();

        let authorization
            = get_authorization(&GetAuthorizationOptions {
                configuration: &project.config,
                http_client: &project.http_client,
                registry: &registry_base,
                ident: Some(&ident),
                auth_mode: AuthorizationMode::RespectConfiguration,
                allow_oidc: false,
            }).await?;

        let bytes
            = http_npm::get_package_metadata(&GetPackageMetadataParams {
                http_client: &project.http_client,
                registry: &registry_base,
                ident: &ident,
                authorization: authorization.as_deref(),
                global_folder: &project.config.settings.global_folder.value,
                refresh_lockfile: false,
                background_writes: None,
            }).await?;

        #[derive(Deserialize)]
        struct RegistryMetadata<'a> {
            #[serde(default, rename = "dist-tags")]
            dist_tags: BTreeMap<String, zpm_semver::Version>,
            #[serde(borrow)]
            versions: BTreeMap<zpm_semver::Version, RawJsonValue<'a>>,
        }

        let registry_data: RegistryMetadata
            = JsonDocument::hydrate_from_slice(&bytes[..])?;

        let target_version = match selector {
            Selector::Latest => {
                registry_data.dist_tags.get("latest")
                    .cloned()
                    .ok_or_else(|| Error::TagNotFound("latest".to_string()))?
            },
            Selector::Tag(tag) => {
                registry_data.dist_tags.get(&tag)
                    .cloned()
                    .ok_or_else(|| Error::TagNotFound(tag))?
            },
            Selector::Range(range) => {
                registry_data.versions.keys().rev()
                    .find(|version| range.check(version))
                    .cloned()
                    .ok_or_else(|| Error::PackageNotFound(ident.clone()))?
            },
        };

        let manifest_value = registry_data.versions.get(&target_version)
            .ok_or_else(|| Error::PackageNotFound(ident.clone()))?;

        let manifest_json: JsonValue
            = JsonDocument::hydrate_from_value(manifest_value)?;

        println!("{}", serde_json::to_string(&manifest_json).unwrap());

        Ok(())
    }
}

enum Selector {
    Latest,
    Tag(String),
    Range(zpm_semver::Range),
}

fn parse_package_arg(arg: &str) -> Result<(Ident, Selector), Error> {
    let (ident_str, range_str) = split_ident_and_selector(arg);

    let ident = Ident::from_file_string(ident_str)
        .map_err(|_| Error::InvalidIdent(ident_str.to_string()))?;

    let selector = match range_str {
        None | Some("") => Selector::Latest,
        Some(range_str) => zpm_semver::Range::from_file_string(range_str)
            .map(Selector::Range)
            .unwrap_or_else(|_| Selector::Tag(range_str.to_string())),
    };

    Ok((ident, selector))
}
