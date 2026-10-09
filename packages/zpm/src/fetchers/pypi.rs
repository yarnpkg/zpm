use zpm_primitives::{Locator, PypiRegistryReference};
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    install::{FetchResult, InstallContext},
    pypi,
    resolvers::pypi::{context_targets, fetch_project, version_files},
};

use super::PackageData;

/// Finds the artifact to download. Locators produced by the platform
/// variants embed the artifact URL; parent locators pick the artifact
/// matching the current environment.
async fn resolve_artifact_url(context: &InstallContext<'_>, params: &PypiRegistryReference) -> Result<String, Error> {
    if let Some(url) = &params.url {
        return Ok(url.0.clone());
    }

    let index_project
        = fetch_project(context, &params.ident, Some(&params.version)).await?;

    let files
        = version_files(&index_project, &params.ident, &params.version)?;

    let targets
        = context_targets(context);

    let file
        = pypi::select_pinned_file(files, &targets.current_env(), &targets)
            .ok_or_else(|| Error::InvalidResolution(format!(
                "No artifact of {}@{} is compatible with the current platform",
                params.ident.as_str(),
                params.version.to_file_string(),
            )))?;

    Ok(file.url.clone())
}

/// The cache extension of an artifact. Zip source distributions get their
/// own extension so that the linker doesn't take them for wheels (which are
/// zips too).
fn archive_extension(url: &str) -> &'static str {
    let path
        = url.split('#').next().unwrap();

    if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
        ".tar.gz"
    } else if path.ends_with(".zip") {
        ".src.zip"
    } else {
        ".zip"
    }
}

pub fn try_fetch_locator_sync(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching PyPI packages");

    let ext
        = params.url.as_ref().map_or(".zip", |url| archive_extension(&url.0));

    if is_mock_request {
        let archive_path
            = package_cache.key_path(locator, ext);

        return Ok(Some(FetchResult::new_mock(archive_path.clone(), archive_path)));
    }

    let cache_entry
        = package_cache.check_cache_entry(locator.clone(), ext)?;

    Ok(cache_entry.map(|cache_entry| FetchResult::new(PackageData::Zip {
        archive_path: cache_entry.path.clone(),
        checksum: cache_entry.checksum,
        context_directory: cache_entry.path.clone(),
        package_directory: cache_entry.path,
    })))
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &PypiRegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching PyPI packages");

    let project
        = context.project
            .expect("The project is required for fetching PyPI packages");

    let artifact_url
        = resolve_artifact_url(context, params).await?;

    let ext
        = archive_extension(&artifact_url);

    if is_mock_request {
        let archive_path
            = package_cache.key_path(locator, ext);

        return Ok(FetchResult::new_mock(archive_path.clone(), archive_path));
    }

    let authorization
        = pypi::get_artifact_authorization(&project.config, &params.ident, &artifact_url);

    let download_url
        = artifact_url.split('#').next().unwrap().to_string();

    let cached_blob
        = package_cache.ensure_blob(locator.clone(), ext, || async {
            let (_, bytes)
                = project.http_client.get(&download_url)?
                    .header("authorization", authorization.as_deref())
                    .send_bytes()
                    .await?;

            Ok(bytes.to_vec())
        }).await?.into_info();

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path.clone(),
        checksum: cached_blob.checksum,
        context_directory: cached_blob.path.clone(),
        package_directory: cached_blob.path,
    }))
}
