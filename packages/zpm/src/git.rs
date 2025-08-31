use std::{collections::BTreeMap, fmt::{self, Display, Formatter}, future::Future, sync::LazyLock};

use git_url_parse::GitUrl;
use reqwest::Url;
use zpm_utils::Path;
use bincode::{Decode, Encode};
use colored::Colorize;
use fancy_regex::Regex;
use serde::{Deserialize, Serialize};
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, github, http::HttpConfig, install::InstallContext, prepare::PrepareParams, primitives::range::AnonymousSemverRange, script::ScriptEnvironment};

static NEW_STYLE_GIT_SELECTOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]+=").unwrap());




impl_serialization_traits!(GitReference);


// Iterate over the values of the parameter; return the first result that succeeds, or the last error.
async fn repeat_until_ok<I, T, E, A, F>(values: Vec<I>, f: F) -> Result<T, E>
    where A: Future<Output = Result<T, E>>, F: Fn(I) -> A,
{
    let mut last_error = None;

    for value in values {
        let result
            = f(value).await;

        match result {
            Ok(value) => {
                return Ok(value);
            },

            Err(error) => {
                last_error = Some(error);
            },
        }
    }

    Err(last_error.unwrap())
}

fn validate_repo_url(url: &str, config: &HttpConfig) -> Result<(), Error> {
    let git_url = GitUrl::parse(url)
        .map_err(|_| Error::InvalidGitUrl(url.to_owned()))?;

    let Some(host) = git_url.host else {
        return Ok(());
    };

    let url = format!("https://{}", host);

    let url = Url::parse(&url)
        .map_err(|_| Error::InvalidUrl(url.to_owned()))?;

    let url_settings = config.url_settings(&url);
    if url_settings.enable_network == Some(false) {
        return Err(Error::NetworkDisabledError(url));
    }

    Ok(())
}

async fn ls_remote(repo: &GitSource, config: &HttpConfig) -> Result<BTreeMap<String, String>, Error> {
    repeat_until_ok(repo.to_urls(), |url| async move {
        validate_repo_url(&url, config)?;

        let output = ScriptEnvironment::new()?
            .with_env(make_git_env())
            .run_exec("git", &["ls-remote", &url])
            .await?
            .ok()?
            .output();

        let output = String::from_utf8(output.stdout).unwrap();
        let mut refs = BTreeMap::new();

        for line in output.lines() {
            let mut parts = line.split_whitespace();
            let hash = parts.next().unwrap();
            let name = parts.next().unwrap();

            refs.insert(name.to_string(), hash.to_string());
        }

        Ok(refs)
    }).await
}

pub async fn resolve_git_treeish(git_range: &GitRange, config: &HttpConfig) -> Result<String, Error> {
    match &git_range.treeish {
        GitTreeish::AnythingGoes(treeish) => {
            if let Ok(result) = resolve_git_treeish_stricter(&git_range.repo, GitTreeish::Commit(treeish.clone()), config).await {
                Ok(result)
            } else if let Ok(result) = resolve_git_treeish_stricter(&git_range.repo, GitTreeish::Tag(treeish.clone()), config).await {
                Ok(result)
            } else if let Ok(result) = resolve_git_treeish_stricter(&git_range.repo, GitTreeish::Head(treeish.clone()), config).await {
                Ok(result)
            } else {
                Err(Error::InvalidGitSpecifier)
            }
        },

        _ => {
            resolve_git_treeish_stricter(&git_range.repo, git_range.treeish.clone(), config).await
        },
    }
}

async fn resolve_git_treeish_stricter(repo: &GitSource, treeish: GitTreeish, config: &HttpConfig) -> Result<String, Error> {
    let refs = ls_remote(repo, config).await?;

    match treeish {
        GitTreeish::AnythingGoes(_) => {
            unreachable!();
        },

        GitTreeish::Head(head) => {
            let ref_name = if head == "HEAD" {
                "HEAD".to_string()
            } else {
                format!("refs/heads/{}", head)
            };

            let head = refs.get(&ref_name)
                .ok_or(Error::InvalidGitBranch(head))?;

            Ok(head.to_string())
        }

        GitTreeish::Commit(commit) => {
            if commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(commit)
            } else {
                Err(Error::InvalidGitCommit(commit))
            }
        }

        GitTreeish::Semver(tag) => {
            let mut candidates: Vec<(String, zpm_semver::Version)> = refs.into_iter()
                .filter(|(k, _)| k.starts_with("refs/tags/") && !k.ends_with("^{}"))
                .filter_map(|(k, _)| zpm_semver::Version::from_file_string(&k[10..]).ok().map(|v| (k, v)))
                .filter(|(_, v)| tag.check(v))
                .collect();

            candidates.sort_by(|(_, v1), (_, v2)| {
                v2.cmp(v1)
            });

            if let Some((k, _)) = candidates.first() {
                Ok(k.to_string())
            } else {
                Err(Error::NoCandidatesFound(AnonymousSemverRange {
                    range: tag,
                }.into()))
            }
        }

        GitTreeish::Tag(tag) => {
            let ref_name = format!("refs/tags/{}", tag);

            let head = refs.get(&ref_name)
                .ok_or(Error::InvalidGitBranch(tag))?;

            Ok(head.to_string())
        }
    }
}

fn make_git_env() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();

    if let Err(std::env::VarError::NotPresent) = std::env::var("GIT_SSH_COMMAND") {
        let ssh = std::env::var("GIT_SSH").unwrap_or("ssh".to_string());
        let ssh_command = format!("{} -o BatchMode=yes", ssh);

        env.insert("GIT_SSH_COMMAND".to_string(), ssh_command);
    }

    env
}

pub async fn clone_repository(context: &InstallContext<'_>, source: &GitSource, commit: &str) -> Result<Path, Error> {
    let project = context.project
        .expect("The project is required for cloning repositories");

    let clone_dir
        = Path::temp_dir()?;

    if download_into(&source, commit, &clone_dir, &project.http_client).await?.is_some() {
        return Ok(clone_dir);
    }

    git_clone_into(source, commit, &clone_dir, &project.http_client.config).await?;
    Ok(clone_dir)
}

async fn download_into(source: &GitSource, commit: &str, download_dir: &Path, http_client: &std::sync::Arc<crate::http::HttpClient>) -> Result<Option<()>, Error> {
    if github::download_into(source, commit, download_dir, http_client).await?.is_some() {
        return Ok(Some(()));
    }

    Ok(None)
}

async fn git_clone_into(source: &GitSource, commit: &str, clone_dir: &Path, config: &HttpConfig) -> Result<(), Error> {
    repeat_until_ok(source.to_urls(), |clone_url| async move {
        validate_repo_url(&clone_url, config)?;

        ScriptEnvironment::new()?
            .with_env(make_git_env())
            .run_exec("git", &["clone", "-c", "core.autocrlf=false", &clone_url, clone_dir.as_str()])
            .await?
            .ok()?;

        Ok::<(), Error>(())
    }).await?;

    ScriptEnvironment::new()?
        .with_cwd(clone_dir.clone())
        .with_env(make_git_env())
        .run_exec("git", &["checkout", commit])
        .await?
        .ok()?;

    Ok(())
}
