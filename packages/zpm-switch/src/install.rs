use std::process::Command;

use zpm_formats::{entries_to_disk, iter_ext::IterExt};
use zpm_utils::{get_system_string, FromFileString, Path};

use crate::{cache, errors::Error, http::fetch, manifest::VersionPackageManagerReference};

async fn install_native_from_zip(source: &cache::CacheKey, binary_name: &str) -> Result<Command, Error> {
    let cache_path = cache::ensure(source, |p| async move {
        let data
            = fetch(&source.to_url()).await?;

        let target_dir = p
            .with_join_str("bin");

        if cache::use_yarnpkg_endpoints() {
            // Old format: zip file from repo.yarnpkg.com
            let entries
                = zpm_formats::zip::entries_from_zip(&data)?;

            entries_to_disk(&entries, &target_dir)?;
        } else {
            // New format: tgz from npm registry
            // npm packages have structure: package/{yarn,LICENSE.md}
            // After stripping first segment: {yarn,LICENSE.md}
            let tar_data
                = zpm_formats::tar::unpack_tgz(&data)?;

            let entries
                = zpm_formats::tar::entries_from_tar(&tar_data)?
                    .into_iter()
                    .strip_first_segment()
                    .collect::<Vec<_>>();

            entries_to_disk(&entries, &target_dir)?;
        }

        Ok(())
    }).await?;

    let main_file_abs = if cache::use_yarnpkg_endpoints() {
        // Old format: binary is at bin/yarn-bin
        cache_path
            .with_join_str("bin")
            .with_join_str(binary_name)
    } else {
        // New format: binary is at bin/yarn (copied from yarn-bin)
        cache_path
            .with_join_str("bin")
            .with_join_str("yarn")
    };

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

async fn install_node_js_from_package(source: &cache::CacheKey, main_file: Path) -> Result<Command, Error> {
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
        .with_join(&main_file);

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
        return install_native_from_zip(&version_platform, "yarn-bin").await;
    }

    if zpm_semver::Range::from_file_string(">=2.0.0-0").unwrap().check(&package_manager.version) {
        return install_node_js_from_url(&version_platform).await;
    }

    if zpm_semver::Range::from_file_string(">=0.0.0-0").unwrap().check(&package_manager.version) {
        return install_node_js_from_package(&version_platform, Path::try_from("bin/yarn.js").unwrap()).await;
    }

    unreachable!()
}
