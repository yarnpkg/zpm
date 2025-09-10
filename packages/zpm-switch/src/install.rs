use std::{future::Future, process::Command};

use blake2::{Blake2s256, Digest};
use serde::Serialize;
use zpm_formats::{entries_to_disk, iter_ext::IterExt};
use zpm_utils::{get_system_string, FromFileString, IoResultExt, Path, ToFileString};

use crate::{errors::Error, http::fetch, manifest::VersionPackageManagerReference};

async fn cache<T: Serialize, R: Future<Output = Result<(), Error>>, F: FnOnce(Path) -> R>(hash: T, f: F) -> Result<Path, Error> {
    let serialized_key
        = sonic_rs::to_string(&hash).unwrap();

    let mut hasher
        = Blake2s256::new();

    hasher.update(&serialized_key);

    let digest = hasher.finalize()
        .to_vec();

    let cache_path = Path::home_dir()?
        .ok_or(Error::MissingHomeFolder)?
        .with_join_str(format!(".yarn/switch/cache/{}", hex::encode(digest)));

    let ready_path = cache_path
        .with_join_str(".ready");

    if !ready_path.fs_exists() {
        let temp_dir
            = Path::temp_dir()?;

        f(temp_dir.clone()).await?;

        temp_dir
            .with_join_str(".ready")
            .fs_write([])?;

        cache_path
            .fs_create_parent()?;

        temp_dir
            .fs_move(&cache_path)
            .ok_exists()?;
    }

    Ok(cache_path)
}

async fn install_native_from_zip(url: &str, binary_name: &str) -> Result<Command, Error> {
    let cache_path = cache(url, |p| async move {
        let zip
            = fetch(url).await?;

        let entries
            = zpm_formats::zip::entries_from_zip(&zip)?;

        let target_dir
            = p.with_join_str("bin");

        entries_to_disk(&entries, &target_dir)?;

        Ok(())
    }).await?;

    let main_file_abs = cache_path
        .with_join_str("bin")
        .with_join_str(binary_name);

    let command
        = Command::new(main_file_abs.to_path_buf());

    Ok(command)
}

async fn install_node_js_from_url(url: &str) -> Result<Command, Error> {
    let cache_path = cache(url, |p| async move {
        p.with_join_str("bin.js").fs_write(fetch(url).await?)?;
        Ok(())
    }).await?;

    let main_file_abs = cache_path
        .with_join_str("bin.js");

    let mut command
        = Command::new("node");

    command.arg(main_file_abs.to_path_buf());

    Ok(command)
}

async fn install_node_js_from_package(url: &str, main_file: Path) -> Result<Command, Error> {
    let cache_path = cache(url, |p| async move {
        let compressed_data
            = fetch(url).await?;

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
    let version
        = package_manager.version.to_file_string();
    let platform
        = get_system_string();

    let url
        = format!("https://repo.yarnpkg.com/releases/{}/{}", version, platform);

    if zpm_semver::Range::from_file_string(">=6.0.0-0").unwrap().check(&package_manager.version) {
        return install_native_from_zip(&url, "yarn-bin").await;
    }

    if zpm_semver::Range::from_file_string(">=2.0.0-0").unwrap().check(&package_manager.version) {
        return install_node_js_from_url(&url).await;
    }

    if zpm_semver::Range::from_file_string(">=0.0.0-0").unwrap().check(&package_manager.version) {
        return install_node_js_from_package(&url, Path::try_from("bin/yarn.js").unwrap()).await;
    }

    unreachable!()
}
