use std::{borrow::Cow, collections::BTreeMap, iter::once, str::FromStr};

use itertools::Itertools;
use serde::Serialize;
use zpm_formats::{Entry, iter_ext::IterExt};
use zpm_parsers::JsonDocument;
use zpm_primitives::{BuiltinReference, Ident, Locator};
use zpm_utils::{Path, RawPath, ToFileString};

use crate::{error::Error, fetchers::PackageData, install::{FetchResult, InstallContext}, manifest::bin::BinField, npm::NpmEntryExt};

async fn fetch_nodejs_locator<'a>(context: &InstallContext<'a>, locator: &Locator, version: &zpm_semver::Version, url: String, bin_file: Path) -> Result<FetchResult, Error> {
    let package_cache = context.package_cache
        .expect("The package cache is required for fetching npm packages");

    let package_subdir
        = locator.ident.nm_subdir();

    let cached_blob = package_cache.ensure_blob(locator.clone(), ".zip", || async {
        let version_str
            = version.to_file_string();

        let project = context.project
            .expect("The project is required for fetching a nodejs package");

        let bytes
            = project.http_client.get(&url)?
                .send().await?
                .error_for_status()?
                .bytes().await?;

        let tar_data
            = zpm_formats::tar::unpack_tgz(&bytes)?;

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct GeneratedManifest<'a> {
            name: &'a str,
            version: &'a str,
            prefer_unplugged: bool,
            bin: BinField,
        }

        let manifest = GeneratedManifest {
            name: locator.ident.as_str(),
            version: version_str.as_str(),
            prefer_unplugged: true,
            bin: BinField::Map(BTreeMap::from([(Ident::from_str("node").unwrap(), RawPath {
                raw: bin_file.to_file_string(),
                path: bin_file.clone(),
            })])),
        };

        let serialized_manifest
            = JsonDocument::to_string(&manifest)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .filter(|entry| entry.name == bin_file)
                .chain(once(Entry::new_file(Path::from_str("package.json").unwrap(), Cow::Owned(serialized_manifest.into_bytes()))))
                .prepare_npm_entries(&locator.ident.nm_subdir())
                .collect_vec();

        Ok(package_cache.bundle_entries(entries)?)
    }).await?.into_info();

    let package_directory = cached_blob.path
        .with_join(&package_subdir);

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path,
        checksum: cached_blob.checksum,
        context_directory: package_directory.clone(),
        package_directory,
    }))
}


pub async fn fetch_builtin_locator(context: &InstallContext<'_>, locator: &Locator, params: &BuiltinReference) -> Result<FetchResult, Error> {
    let version_str
        = params.version.to_file_string();

    match locator.ident.as_str() {
        "@builtin/node"
            => Ok(FetchResult::new(PackageData::Abstract)),

        "@builtin/node-linux-x64"
            => fetch_nodejs_locator(context, locator, &params.version, format!("https://nodejs.org/dist/v{}/node-v{}-linux-x64.tar.gz", version_str, version_str), Path::from_str("bin/node").unwrap()).await,
        "@builtin/node-linux-arm64"
            => fetch_nodejs_locator(context, locator, &params.version, format!("https://nodejs.org/dist/v{}/node-v{}-linux-arm64.tar.gz", version_str, version_str), Path::from_str("bin/node").unwrap()).await,
        "@builtin/node-darwin-x64"
            => fetch_nodejs_locator(context, locator, &params.version, format!("https://nodejs.org/dist/v{}/node-v{}-darwin-x64.tar.gz", version_str, version_str), Path::from_str("bin/node").unwrap()).await,
        "@builtin/node-darwin-arm64"
            => fetch_nodejs_locator(context, locator, &params.version, format!("https://nodejs.org/dist/v{}/node-v{}-darwin-arm64.tar.gz", version_str, version_str), Path::from_str("bin/node").unwrap()).await,

        _ => Err(Error::Unsupported)?,
    }
}
