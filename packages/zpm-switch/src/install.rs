use std::{process::Command, str::FromStr};

use serde::Deserialize;
use zpm_formats::{entries_to_disk, iter_ext::IterExt};
use zpm_parsers::JsonDocument;
use zpm_utils::{get_system_string, FromFileString, Path};

use crate::{cache, errors::Error, http::fetch, manifest::VersionPackageManagerReference};

async fn install_native_from_zpm(source: &cache::CacheKey, binary_name: &Path) -> Result<Command, Error> {
    let cache_path = cache::ensure(source, |p| async move {
        if let Some(npm_url) = source.to_npm_url() {
            let tgz_data
                = fetch(&npm_url).await?;

            let tar_data
                = zpm_formats::tar::unpack_tgz(&tgz_data)?;

            let entries
                = zpm_formats::tar::entries_from_tar(&tar_data)?
                    .into_iter()
                    .strip_first_segment()
                    .collect::<Vec<_>>();

            let package_json
                = entries.iter()
                    .find(|entry| entry.name.basename() == Some("package.json"))
                    .expect("Expected a package manifest entry to exist");

            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Bin {
                yarn: Path,
            }

            #[derive(Deserialize)]
            struct PackageJson {
                bin: Bin,
            }

            let package_json_data: PackageJson
                = JsonDocument::hydrate_from_slice(&package_json.data.as_ref())?;

            let mut bin_entry
                = entries.into_iter()
                    .find(|entry| entry.name == package_json_data.bin.yarn)
                    .expect("Expected the main bin entry to be found");

            bin_entry.name
                = binary_name.clone();

            let target_dir = p
                .with_join_str("bin");

            entries_to_disk(&[bin_entry], &target_dir)?;
        } else {
            let repo_url
                = source.to_url();

            let zip_data
                = fetch(&repo_url).await?;

            let entries
                = zpm_formats::zip::entries_from_zip(&zip_data)?;

            let bin_entry
                = entries.into_iter()
                    .find(|entry| entry.name == binary_name.clone())
                    .expect("Expected the main bin entry to be found");

            let target_dir = p
                .with_join_str("bin");

            entries_to_disk(&[bin_entry], &target_dir)?;
        }

        Ok(())
    }).await?;

    let main_file_abs = cache_path
        .with_join_str("bin")
        .with_join(&binary_name);

    let command
        = Command::new(main_file_abs.to_path_buf());

    Ok(command)
}

async fn install_node_js_from_url(source: &cache::CacheKey) -> Result<Command, Error> {
    let cache_path = cache::ensure(source, |p| async move {
        p.with_join_str("bin.js").fs_write(fetch(&source.to_url()).await?)?;
        Ok(())
    }).await?;

    let main_file_abs = cache_path
        .with_join_str("bin.js");

    let mut command
        = Command::new("node");

    command.arg(main_file_abs.to_path_buf());

    Ok(command)
}

async fn install_node_js_from_package(source: &cache::CacheKey, main_file: &Path) -> Result<Command, Error> {
    let cache_path = cache::ensure(source, |p| async move {
        let compressed_data
            = fetch(&source.to_url()).await?;

        let data
            = zpm_formats::tar::unpack_tgz(&compressed_data)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&data)?
                .into_iter()
                .strip_first_segment()
                .collect::<Vec<_>>();

        zpm_formats::entries_to_disk(&entries, &p)?;

        Ok(())
    }).await?;

    let main_file_abs = cache_path
        .with_join(main_file);

    let mut command
        = Command::new("node");

    command.arg(main_file_abs.to_path_buf());

    Ok(command)
}

pub async fn install_package_manager(package_manager: &VersionPackageManagerReference) -> Result<Command, Error> {
    let version_platform = cache::CacheKey {
        cache_version: cache::CACHE_VERSION,
        version: package_manager.version.clone(),
        platform: get_system_string().to_string(),
    };

    if zpm_semver::Range::from_file_string(">=6.0.0-0").unwrap().check(&package_manager.version) {
        return install_native_from_zpm(&version_platform, &Path::from_str("yarn-bin").unwrap()).await;
    }

    if zpm_semver::Range::from_file_string(">=2.0.0-0").unwrap().check(&package_manager.version) {
        return install_node_js_from_url(&version_platform).await;
    }

    if zpm_semver::Range::from_file_string(">=0.0.0-0").unwrap().check(&package_manager.version) {
        return install_node_js_from_package(&version_platform, &Path::from_str("bin/yarn.js").unwrap()).await;
    }

    unreachable!()
}
