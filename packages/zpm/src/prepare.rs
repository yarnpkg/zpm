use arca::Path;
use bincode::{Decode, Encode};

use crate::{error::Error, script::ScriptEnvironment};

#[derive(Clone, Default, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrepareParams {
    pub cwd: Option<String>,
    pub workspace: Option<String>,
}

enum PackageManager {
    Npm,
    Pnpm,

    // Admittedly we perhaps change the `yarn pack` interface a little too often
    YarnClassic,
    YarnModern,
    YarnZpm,
}

pub async fn prepare_project(folder_path: &Path, params: &PrepareParams) -> Result<Vec<u8>, Error> {
    let package_manager = get_package_manager(folder_path)?;

    match package_manager {
        PackageManager::Npm => prepare_npm_project(folder_path).await,
        PackageManager::Pnpm => prepare_pnpm_project(folder_path, params).await,
        PackageManager::YarnClassic => prepare_yarn_zpm_project(folder_path, params).await,
        PackageManager::YarnModern => prepare_yarn_modern_project(folder_path, params).await,
        PackageManager::YarnZpm => prepare_yarn_zpm_project(folder_path, params).await,
    }
}

fn get_package_manager(folder_path: &Path) -> Result<PackageManager, Error> {
    if folder_path.with_join_str("package-lock.json").fs_exists() {
        return Ok(PackageManager::Npm);
    }
    
    let yarn_lock_path = folder_path
        .with_join_str("yarn.lock")
        .fs_read_text();

    if let Ok(yarn_lock) = yarn_lock_path {
        if yarn_lock.starts_with("{") {
            return Ok(PackageManager::YarnZpm);
        } else if yarn_lock.starts_with("__metadata") {
            return Ok(PackageManager::YarnModern);
        } else {
            return Ok(PackageManager::YarnClassic);
        }
    } else if let Err(err) = yarn_lock_path {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err.into());
        }
    }

    if folder_path.with_join_str("pnpm-lock.yaml").fs_exists() {
        return Ok(PackageManager::Pnpm);
    }

    return Ok(PackageManager::YarnZpm);
}

async fn prepare_npm_project(folder_path: &Path) -> Result<Vec<u8>, Error> {
    ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("npm", vec!["install"])
        .await
        .ok()?;

    let pack_result = ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("npm", vec!["pack"])
        .await;

    pack_result.ok()?;

    let pack_file
        = String::from_utf8(pack_result.output().stdout.clone())?;

    let pack_tgz = folder_path
        .with_join_str(pack_file.trim())
        .fs_read()?;

    Ok(pack_tgz)
}

async fn prepare_yarn_modern_project(folder_path: &Path, params: &PrepareParams) -> Result<Vec<u8>, Error> {
    ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("yarn", vec!["pack", "--install-if-needed"])
        .await
        .ok()?;

    let pack_tgz = folder_path
        .with_join_str("package.tgz")
        .fs_read()?;

    Ok(pack_tgz)
}

async fn prepare_pnpm_project(folder_path: &Path, params: &PrepareParams) -> Result<Vec<u8>, Error> {
    ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("pnpm", vec!["install"])
        .await
        .ok()?;

    let pack_result = ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("pnpm", vec!["pack"])
        .await;

    pack_result.ok()?;

    let pack_file
        = String::from_utf8(pack_result.output().stdout.clone())?;

    let pack_tgz = folder_path
        .with_join_str(pack_file.trim())
        .fs_read()?;

    Ok(pack_tgz)
}

async fn prepare_yarn_zpm_project(folder_path: &Path, params: &PrepareParams) -> Result<Vec<u8>, Error> {
    let archive_path = folder_path
        .with_join_str("archive.tgz");

    ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("yarn", vec!["install"])
        .await
        .ok()?;

    let mut pack_args = vec![];

    if let Some(workspace) = &params.workspace {
        pack_args.push("workspace");
        pack_args.push(workspace.as_str());
    }

    pack_args.push("pack");
    pack_args.push("--filename");
    pack_args.push(archive_path.as_str());

    ScriptEnvironment::new()
        .with_cwd(folder_path.clone())
        .run_exec("yarn", pack_args)
        .await
        .ok()?;

    let pack_tgz = archive_path
        .fs_read()?;

    Ok(pack_tgz)
}
