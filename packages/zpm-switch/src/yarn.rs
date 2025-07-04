use serde::Deserialize;
use serde_with::serde_as;
use std::{collections::BTreeMap, str::FromStr};
use zpm_macros::parse_enum;
use zpm_semver::{Range, Version};
use zpm_utils::{impl_serialization_traits, ExplicitPath, FromFileString, Path, RawPath, ToFileString, ToHumanString};

use crate::{errors::Error, http::fetch, manifest::{PackageManagerReference, VersionPackageManagerReference}, yarn_enums::{ChannelSelector, Selector}};

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

pub async fn get_default_yarn_version(release_line: Option<crate::yarn_enums::ReleaseLine>) -> Result<PackageManagerReference, Error> {
    if let Ok(env) = std::env::var("YARNSW_DEFAULT") {
        return Ok(PackageManagerReference::from_file_string(&env)?);
    }

    let channel_selector
        = release_line.unwrap_or(crate::yarn_enums::ReleaseLine::Default)
            .stable();

    let version
      = resolve_channel_selector(&channel_selector).await?;

    Ok(VersionPackageManagerReference {version}.into())
}

pub async fn resolve_selector(selector: &Selector) -> Result<Version, Error> {
  match selector {
    Selector::Channel(params) => {
      resolve_channel_selector(params).await
    },

    Selector::Range(params) => {
      resolve_semver_range(&params.range).await
    },
  }
}

pub async fn resolve_semver_range(range: &Range) -> Result<Version, Error> {
    let response
        = fetch("https://repo.yarnpkg.com/releases").await?;

    let data: TagsPayload = sonic_rs::from_slice(&response)
        .unwrap();

    let highest = data.release_lines.iter()
        .flat_map(|(_, release_line)| &release_line.tags)
        .filter(|v| range.check(*v))
        .max()
        .ok_or(Error::FailedToRetrieveLatestYarnTag)?;

    Ok(highest.clone())
}

pub async fn resolve_channel_selector(channel_selector: &ChannelSelector) -> Result<Version, Error> {
    let release_line = channel_selector.release_line.as_ref()
        .unwrap_or(&crate::yarn_enums::ReleaseLine::Classic)
        .to_file_string();

    let channel = channel_selector.channel.as_ref()
        .unwrap_or(&crate::yarn_enums::Channel::Stable)
        .to_file_string();

    let channel_url
        = format!("https://repo.yarnpkg.com/channels/{}/{}", release_line, channel);

    let response
        = fetch(&channel_url).await?;

    let version_str
        = std::str::from_utf8(&response)?
            .trim();

    let version
        = Version::from_str(version_str)?;

    Ok(version)
}

#[derive(Debug)]
pub struct BinMeta {
    pub cwd: Option<Path>,
    pub args: Vec<String>,
    pub version: String,
}

pub fn get_bin_version() -> String {
    option_env!("INFRA_VERSION")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

pub fn extract_bin_meta() -> BinMeta {
    let mut cwd = None;

    let mut args = std::env::args()
        .skip(1)
        .collect::<Vec<_>>();

    if let Some(first_arg) = args.first() {
        let explicit_path
            = ExplicitPath::from_str(first_arg);

        if let Ok(explicit_path) = explicit_path {
            cwd = Some(explicit_path.raw_path.path);
            args.remove(0);
        }
    }

    let version
        = get_bin_version();

    BinMeta {
        cwd,
        args,
        version,
    }
}
