use std::sync::Arc;

use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Locator, UrlReference};

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions},
    install::{FetchResult, InstallContext},
    manifest::RemoteManifest,
    npm::NpmEntryExt,
    resolvers::Resolution,
};

use super::PackageData;

/// Extracts the registry base (scheme + host + port) from a URL.
fn get_registry_base_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port().map(|p| format!(":{}", p)).unwrap_or_default();
    Some(format!("{}://{}{}", parsed.scheme(), host, port))
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &UrlReference) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for fetching URL packages");

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching URL packages");

    let package_subdir
        = locator.ident.nm_subdir();

    // Try to get authorization for the URL's registry
    let authorization = if let Some(registry_base) = get_registry_base_from_url(&params.url) {
        http_npm::get_authorization(&GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry_base,
            ident: Some(&locator.ident),
            auth_mode: AuthorizationMode::RespectConfiguration,
            allow_oidc: false,
        }).await?
    } else {
        None
    };

    let cached_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let response = project.http_client.get(&params.url)?
            .header("authorization", authorization.as_deref())
            .send().await?;

        let tgz_data = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;
        let tar_data
            = zpm_formats::tar::unpack_tgz(&tgz_data)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .prepare_npm_entries(&package_subdir)
                .collect::<Vec<_>>();

        Ok(package_cache.bundle_entries(entries)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&cached_blob.data)?;

    let manifest: RemoteManifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest);

    let package_directory = cached_blob.info.path
        .with_join(&package_subdir);

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            archive_path: cached_blob.info.path,
            checksum: cached_blob.info.checksum,
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
