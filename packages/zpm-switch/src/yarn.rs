use serde::Deserialize;
use serde_with::serde_as;
use zpm_parsers::JsonDocument;
use std::{collections::BTreeMap, fmt::Debug, str::FromStr};
use zpm_semver::{Range, Version, VersionRc};
use zpm_utils::{ExplicitPath, FromFileString, Path, ToFileString};

use crate::{errors::Error, http::fetch, manifest::{LocalPackageManagerReference, PackageManagerReference, VersionPackageManagerReference}, yarn_enums::{ChannelSelector, Selector}};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseLine {
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
        if let Some(bin_path) = env.strip_prefix("local:") {
            return Ok(LocalPackageManagerReference {path: Path::from_file_string(bin_path)?}.into());
        }
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

    let data: TagsPayload
        = JsonDocument::hydrate_from_slice(&response)?;

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
    if let Some(version) = option_env!("INFRA_VERSION") {
        return version.to_string();
    }

    let mut cargo_version
        = zpm_semver::Version::from_str(env!("CARGO_PKG_VERSION"))
            .expect("Failed to parse Cargo package version");

    let mut rc = cargo_version.rc
        .unwrap_or_default();

    rc.push(VersionRc::String("local".into()));

    cargo_version.rc = Some(rc);
    cargo_version.to_file_string()
}

pub fn extract_bin_meta(args: Option<Vec<String>>) -> BinMeta {
    let mut cwd = None;

    let mut args = args.unwrap_or_else(|| {
        std::env::args()
            .skip(1)
            .collect::<Vec<_>>()
    });

    while let Some(first_arg) = args.first() {
        if first_arg == "--cwd" {
            // Reject when the next token is a flag or missing,
            // otherwise `yarn --cwd --immutable` captures `--immutable`
            // as the path.
            let raw = match args.get(1) {
                Some(next) if !next.starts_with('-') => next.clone(),
                _ => {
                    eprintln!("error: `--cwd` requires a path argument");
                    std::process::exit(1);
                },
            };
            cwd = Some(Path::current_dir().unwrap().with_join_str(&raw));
            args.drain(0..2);
            continue;
        }

        if let Some(rest) = first_arg.strip_prefix("--cwd=") {
            if rest.is_empty() {
                eprintln!("error: `--cwd=` requires a path argument");
                std::process::exit(1);
            }
            let raw = rest.to_string();
            cwd = Some(Path::current_dir().unwrap().with_join_str(&raw));
            args.remove(0);
            continue;
        }

        let explicit_path
            = ExplicitPath::from_str(first_arg);

        if let Ok(explicit_path) = explicit_path {
            cwd = Some(Path::current_dir().unwrap().with_join(&explicit_path.raw_path.path));
            args.remove(0);
        }

        break;
    }

    let version
        = get_bin_version();

    BinMeta {
        cwd,
        args,
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_of(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cwd_space_separated_form_consumes_two_args() {
        let meta = extract_bin_meta(Some(vec_of(&["--cwd", "./sub", "install"])));
        assert!(meta.cwd.is_some());
        assert_eq!(meta.args, vec_of(&["install"]));
    }

    #[test]
    fn cwd_equals_form_consumes_one_arg() {
        let meta = extract_bin_meta(Some(vec_of(&["--cwd=./sub", "install"])));
        assert!(meta.cwd.is_some());
        assert_eq!(meta.args, vec_of(&["install"]));
    }

    #[cfg(windows)]
    #[test]
    fn native_windows_paths_are_explicit_path_args() {
        let meta = extract_bin_meta(Some(vec_of(&[r"D:\a\zpm\zpm\tests\acceptance-tests", "jest"])));
        assert!(meta.cwd.is_some());
        assert_eq!(meta.args, vec_of(&["jest"]));
    }

    #[test]
    fn missing_cwd_arg_does_not_get_set() {
        // No `--cwd` at all — cwd should remain unset and args
        // should pass through unchanged.
        let meta = extract_bin_meta(Some(vec_of(&["install", "--immutable"])));
        assert!(meta.cwd.is_none());
        assert_eq!(meta.args, vec_of(&["install", "--immutable"]));
    }
}
