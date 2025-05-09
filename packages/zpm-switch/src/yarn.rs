use serde::{de, Deserialize};
use serde_with::serde_as;
use std::collections::BTreeMap;
use zpm_semver::{Range, Version};
use zpm_utils::{FromFileString, RawPath};

use crate::{errors::Error, http::fetch, manifest::{PackageManagerReference, VersionPackageManagerReference}};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseLine {
    stable: Version,
    tags: Vec<Version>,
}

#[serde_as]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagsPayload {
    release_lines: BTreeMap<String, ReleaseLine>,
}

pub fn is_cwd_arg(arg: &str) -> bool {
    arg.chars().find(|c| *c == '\\' || *c == '/').is_some()
}

pub async fn fix_cwd(args: &mut Vec<String>) -> Result<(), Error> {
    // Yarn supports passing a folder as first parameter (for example `yarn path/to/workspace`), in
    // which case it'll cd into it before running the command. Since we want to take into account
    // that this folder may have a different packageManager, we have to handle it in Switch.
    //
    // We replace the folder path with `./` to avoid the nested process cd'ing twice into the same
    // relative path, which would fail (we don't outright remove the argument since otherwise it'd
    // cause a diverging behavior if the user was to run something like "yarn a/b c/d").
    //
    if let Some(first_args) = args.first() {
        let dir_separator = first_args.chars()
            .find(|c| *c == '\\' || *c == '/');

        if dir_separator.is_some() {
            std::env::set_current_dir(std::path::PathBuf::from(&first_args))?;
            args[0] = "./".to_string();
        }
    }

    Ok(())
}

pub async fn get_default_yarn_version(release_line: Option<&str>) -> Result<PackageManagerReference, Error> {
    if let Ok(env) = std::env::var("YARN_SWITCH_DEFAULT") {
        return Ok(PackageManagerReference::from_file_string(&env)?);
    }

    get_latest_stable_version(release_line).await
}

pub async fn resolve_range(range: &Range) -> Result<Version, Error> {
    let response
        = fetch("https://repo.yarnpkg.com/tags").await?;

    let data: TagsPayload = sonic_rs::from_slice(&response)
        .unwrap();

    let highest = data.release_lines.iter()
        .flat_map(|(_, release_line)| &release_line.tags)
        .filter(|v| range.check(*v))
        .max()
        .ok_or(Error::FailedToRetrieveLatestYarnTag)?;

    Ok(highest.clone())
}

pub async fn get_latest_stable_version(release_line: Option<&str>) -> Result<PackageManagerReference, Error> {
    let response
        = fetch("https://repo.yarnpkg.com/tags").await?;

    let data: TagsPayload = sonic_rs::from_slice(&response)
        .map_err(|_| Error::FailedToRetrieveLatestYarnTag)?;

    let (_, release_line) = release_line
        .map(|search_key| data.release_lines.iter().find(|(k, _)| k == &search_key))
        .unwrap_or_else(|| data.release_lines.iter().max_by_key(|(_, release_line)| &release_line.stable))
        .ok_or(Error::FailedToRetrieveLatestYarnTag)?;

    Ok(VersionPackageManagerReference {version: release_line.stable.clone()}.into())
}
