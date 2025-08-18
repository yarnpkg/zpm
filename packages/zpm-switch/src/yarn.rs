use serde::Deserialize;
use serde_with::serde_as;
use std::{collections::BTreeMap, fmt::Debug, str::FromStr, time::SystemTime};
use zpm_semver::{Range, Version};
use zpm_utils::{ExplicitPath, FromFileString, Path, ToFileString};

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

    Selector::Version(params) => {
      Ok(params.version.clone())
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

    let highest = data.release_lines.values()
        .flat_map(|release_line| &release_line.tags)
        .filter(|v| range.check(*v))
        .max()
        .ok_or(Error::FailedToResolveYarnRange(range.clone()))?;

    Ok(highest.clone())
}

pub async fn resolve_channel_selector(channel_selector: &ChannelSelector) -> Result<Version, Error> {
    let release_line = channel_selector.release_line.as_ref()
        .unwrap_or(&crate::yarn_enums::ReleaseLine::Classic)
        .to_file_string();

    let channel = channel_selector.channel.as_ref()
        .unwrap_or(&crate::yarn_enums::Channel::Stable)
        .to_file_string();

    let today
        = chrono::Utc::now();

    let channel_path
        = Path::temp_root_dir()?
            .with_join_str(&format!("yswitch-{}-{}-{}", release_line, channel, today.format("%Y%m%d")));

    if let Ok(version_str) = channel_path.fs_read_text_async().await {
        let version
            = Version::from_str(&version_str)?;

        return Ok(version);
    }

    let channel_url
        = format!("https://repo.yarnpkg.com/channels/{}/{}", release_line, channel);

    let response
        = fetch(&channel_url).await?;

    let version_str
        = std::str::from_utf8(&response)?
            .trim();

    let version
        = Version::from_str(version_str)?;

    channel_path
        .fs_write_text(&version_str)?;

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
            cwd = Some(Path::current_dir().unwrap().with_join(&explicit_path.raw_path.path));
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
