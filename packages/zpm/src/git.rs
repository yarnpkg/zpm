use std::{collections::BTreeMap, fmt::{self, Display, Formatter}, sync::LazyLock};

use arca::Path;
use bincode::{Decode, Encode};
use colored::Colorize;
use fancy_regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, prepare::PrepareParams};

static NEW_STYLE_GIT_SELECTOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]+=").unwrap());

static GH_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:github:|https:\/\/github\.com\/|git:\/\/github\.com\/)?(?!\.{1,2}\/)([a-zA-Z0-9._-]+)\/(?!\.{1,2}(?:#|$))([a-zA-Z0-9._-]+?)(?:\.git)?(#.*)?$").unwrap());
static GH_TARBALL_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?:\/\/github\.com\/(?!\.{1,2}\/)([a-zA-Z0-9._-]+)\/(?!\.{1,2}(?:#|$))([a-zA-Z0-9._-]+?)\/tarball\/(.+)?$").unwrap());

static GH_URL_SET: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    Regex::new(r"^ssh:").unwrap(),
    Regex::new(r"^git(?:\+[^:]+)?:").unwrap(),
  
    // `git+` is optional, `.git` is required
    Regex::new(r"^(?:git\+)?https?:[^#]+\/[^#]+(?:\.git)(?:#.*)?$").unwrap(),
  
    Regex::new(r"^git@[^#]+\/[^#]+\.git(?:#.*)?$").unwrap(),
  
    Regex::new(r"^(?:github:|https:\/\/github\.com\/)?(?!\.{1,2}\/)([a-zA-Z._0-9-]+)\/(?!\.{1,2}(?:#|$))([a-zA-Z._0-9-]+?)(?:\.git)?(?:#.*)?$").unwrap(),
    // GitHub `/tarball/` URLs
    Regex::new(r"^https?:\/\/github\.com\/(?!\.{1,2}\/)([a-zA-Z0-9._-]+)\/(?!\.{1,2}(?:#|$))([a-zA-Z0-9._-]+?)\/tarball\/(.+)?$").unwrap(),
]);

pub fn is_git_url<P: AsRef<str>>(url: P) -> bool {
    GH_URL_SET.iter().any(|r| r.is_match(url.as_ref()).unwrap())
}

pub fn normalize_git_url<P: AsRef<str>>(url: P) -> String {
    let mut normalized = url.as_ref().to_string();

    if normalized.starts_with("git+https:") {
        normalized = normalized[4..].to_string();
    }

    normalized = GH_URL.replace(&normalized, "https://github.com/$1/$2.git$3").to_string();
    normalized = GH_TARBALL_URL.replace(&normalized, "https://github.com/$1/$2.git#$3").to_string();

    normalized
}

#[derive(Clone, Debug, Decode, Deserialize, Encode, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum GitTreeish {
    AnythingGoes(String),
    Branch(String),
    Commit(String),
    Semver(zpm_semver::Range),
    Tag(String),
}

impl Display for GitTreeish {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            GitTreeish::AnythingGoes(treeish) => write!(f, "{}", treeish),
            GitTreeish::Branch(branch) => write!(f, "branch={}", branch),
            GitTreeish::Commit(commit) => write!(f, "commit={}", commit),
            GitTreeish::Semver(range) => write!(f, "semver={}", range),
            GitTreeish::Tag(tag) => write!(f, "tag={}", tag),
        }
    }
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GitRange {
    pub repo: String,
    pub treeish: GitTreeish,
    pub prepare_params: PrepareParams,
}

impl FromFileString for GitRange {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        if !is_git_url(src) {
            return Err(Error::InvalidGitUrl(src.to_string()));
        }

        let normalized = normalize_git_url(src);
        extract_git_range(normalized)
    }
}

impl ToFileString for GitRange {
    fn to_file_string(&self) -> String {
        let mut params = vec![];

        params.push(match &self.treeish {
            GitTreeish::AnythingGoes(treeish) => treeish.to_string(),
            GitTreeish::Branch(branch) => format!("branch={}", branch),
            GitTreeish::Commit(commit) => format!("commit={}", commit),
            GitTreeish::Semver(range) => format!("semver={}", range),
            GitTreeish::Tag(tag) => format!("tag={}", tag),
        });

        if let Some(cwd) = &self.prepare_params.cwd {
            params.push(format!("cwd={}", urlencoding::encode(cwd)));
        }

        if let Some(workspace) = &self.prepare_params.workspace {
            params.push(format!("workspace={}", urlencoding::encode(workspace)));
        }

        format!("{}#{}", self.repo, params.join("&"))
    }
}

impl ToHumanString for GitRange {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(135, 175, 255).to_string()
    }
}

impl_serialization_traits!(GitRange);

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GitReference {
    pub repo: String,
    pub commit: String,
    pub prepare_params: PrepareParams,
}

impl FromFileString for GitReference {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let mut parts = src.splitn(2, '#');

        let repo = parts.next().unwrap().to_string();
        let qs = parts.next().unwrap();

        let mut commit = None;
        let mut prepare_params = PrepareParams::default();

        for pair in qs.split('&') {
            if let Some(eq_index) = pair.find('=') {
                let (key, value) = pair.split_at(eq_index);
                let value = urlencoding::decode(&value[1..]).unwrap();

                match key {
                    "commit" =>
                        commit = Some(value.to_string()),

                    "cwd" =>
                        prepare_params.cwd = Some(value.to_string()),

                    "workspace" =>
                        prepare_params.workspace = Some(value.to_string()),

                    _ => {},
                };
            }
        }

        let commit = commit
            .expect("Expected a commit to always be present in a git reference");

        Ok(GitReference {
            repo,
            commit,
            prepare_params,
        })
    }
}

impl ToFileString for GitReference {
    fn to_file_string(&self) -> String {
        let mut params = vec![
            format!("commit={}", urlencoding::encode(&self.commit)),
        ];

        if let Some(cwd) = &self.prepare_params.cwd {
            params.push(format!("cwd={}", urlencoding::encode(cwd)));
        }

        format!("{}#{}", self.repo, params.join("&"))
    }
}

impl ToHumanString for GitReference {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(135, 175, 255).to_string()
    }
}

impl_serialization_traits!(GitReference);

pub fn extract_git_range<P: AsRef<str>>(url: P) -> Result<GitRange, Error> {
    let url = url.as_ref();

    let hash_index = url.find('#');
    if hash_index.is_none() {
        return Ok(GitRange {
            repo: url.to_string(),
            treeish: GitTreeish::Branch("HEAD".to_string()),
            prepare_params: PrepareParams::default(),
        });
    }

    let (repo, subsequent) = url.split_at(hash_index.unwrap());
    let subsequent = &subsequent[1..];

    // New-style: "#commit=abcdef&workspace=foobar"
    if NEW_STYLE_GIT_SELECTOR.is_match(subsequent).unwrap() {
        let mut treeish = GitTreeish::Commit("HEAD".to_string());
        let mut prepare_params = PrepareParams::default();

        for pair in subsequent.split('&') {
            if let Some(eq_index) = pair.find('=') {
                let (key, value) = pair.split_at(eq_index);
                let value = urlencoding::decode(&value[1..]).unwrap();

                match key {
                    "branch" =>
                        treeish = GitTreeish::Branch(value.to_string()),

                    "commit" =>
                        treeish = GitTreeish::Commit(value.to_string()),

                    "semver" =>
                        treeish = GitTreeish::Semver(zpm_semver::Range::from_file_string(value.as_ref())?),

                    "tag" =>
                        treeish = GitTreeish::Tag(value.to_string()),

                    "cwd" =>
                        prepare_params.cwd = Some(value.to_string()),

                    "workspace" =>
                        prepare_params.workspace = Some(value.to_string()),

                    _ => {},
                }
            }
        }

        return Ok(GitRange {
            repo: repo.to_string(),
            treeish,
            prepare_params,
        });
    }

    // Old-style: "#commit:abcdef"
    let treeish = if let Some(colon_index) = subsequent.find(':') {
        let (kind, subsequent) = subsequent.split_at(colon_index);
        let subsequent = &subsequent[1..];

        match kind {
            "branch" => GitTreeish::Branch(subsequent.to_string()),
            "commit" => GitTreeish::Commit(subsequent.to_string()),
            "semver" => GitTreeish::Semver(zpm_semver::Range::from_file_string(subsequent)?),
            "tag" => GitTreeish::Tag(subsequent.to_string()),
            _ => GitTreeish::Commit(subsequent.to_string()),
        }
    } else {
        GitTreeish::AnythingGoes(subsequent.to_string())
    };

    Ok(GitRange {
        repo: repo.to_string(),
        treeish,
        prepare_params: PrepareParams::default(),
    })
}

async fn ls_remote(repo: &str) -> Result<BTreeMap<String, String>, Error> {
    let output = tokio::process::Command::new("git")
        .arg("ls-remote")
        .arg(repo)
        .output()
        .await
        .map_err(|_| Error::GitError)?;

    let output = String::from_utf8(output.stdout).unwrap();
    let mut refs = BTreeMap::new();

    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let hash = parts.next().unwrap();
        let name = parts.next().unwrap();

        refs.insert(name.to_string(), hash.to_string());
    }

    Ok(refs)
}

pub async fn resolve_git_treeish(git_range: &GitRange) -> Result<String, Error> {
    match &git_range.treeish {
        GitTreeish::AnythingGoes(treeish) => {
            if let Ok(result) = resolve_git_treeish_stricter(&git_range.repo, GitTreeish::Commit(treeish.clone())).await {
                Ok(result)
            } else if let Ok(result) = resolve_git_treeish_stricter(&git_range.repo, GitTreeish::Tag(treeish.clone())).await {
                Ok(result)
            } else if let Ok(result ) = resolve_git_treeish_stricter(&git_range.repo, GitTreeish::Branch(treeish.clone())).await {
                Ok(result)
            } else {
                Err(Error::InvalidGitSpecifier)
            }
        }

        _ => resolve_git_treeish_stricter(&git_range.repo, git_range.treeish.clone()).await
    }
}

async fn resolve_git_treeish_stricter(repo: &str, treeish: GitTreeish) -> Result<String, Error> {
    let refs = ls_remote(repo).await?;

    match treeish {
        GitTreeish::AnythingGoes(_) => {
            unreachable!();
        },

        GitTreeish::Branch(branch) => {
            let ref_name = if branch == "HEAD" {
                "HEAD".to_string()
            } else {
                format!("refs/heads/{}", branch)
            };

            let head = refs.get(&ref_name)
                .ok_or(Error::InvalidGitBranch(branch))?;

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
                Err(Error::NoCandidatesFound(tag.to_string()))
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

pub async fn clone_repository(url: &str, commit: &str) -> Result<Path, Error> {
    let git_range = extract_git_range(url)?;

    let normalized_repo_url = normalize_git_url(git_range.repo);

    let clone_dir
        = Path::temp_dir()?;

    Command::new("git")
        .envs(make_git_env())
        .arg("clone")
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg(&normalized_repo_url)
        .arg(clone_dir.to_string())
        .output().await?
        .status
        .success()
        .then_some(())
        .ok_or_else(|| Error::RepositoryCloneFailed(normalized_repo_url.clone()))?;

    Command::new("git")
        .envs(make_git_env())
        .arg("checkout")
        .arg(commit)
        .current_dir(clone_dir.to_string())
        .output().await?
        .status
        .success()
        .then_some(())
        .ok_or_else(|| Error::RepositoryCheckoutFailed(normalized_repo_url.clone(), commit.to_string()))?;

    Ok(clone_dir)
}
