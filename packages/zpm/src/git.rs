use std::collections::BTreeMap;
use std::sync::LazyLock;

use git_url_parse::GitUrl;
use regex::Regex;
use reqwest::Url;
use zpm_config::Setting;
use zpm_git::{GitRange, GitSource, GitTreeish};
use zpm_primitives::AnonymousSemverRange;
use zpm_utils::{repeat_until_ok, Path};
use zpm_utils::{FromFileString, ToFileString};

use crate::{
    error::Error,
    github,
    http::HttpConfig,
    install::InstallContext,
    script::ScriptEnvironment,
};

#[derive(Debug)]
pub enum GitOperation {
    Merge,
    Rebase,
}

impl GitOperation {
    pub fn true_theirs(&self) -> &str {
        match self {
            GitOperation::Merge => "--theirs",
            GitOperation::Rebase => "--ours",
        }
    }
}

pub async fn detect_git_operation(p: &Path) -> Result<Option<GitOperation>, Error> {
    let merge_head_check = ScriptEnvironment::new()?
        .with_cwd(p.clone())
        .run_exec("git", &["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .await?;

    if merge_head_check.success() {
        return Ok(Some(GitOperation::Merge));
    }

    let rebase_head_check = ScriptEnvironment::new()?
        .with_cwd(p.clone())
        .run_exec("git", &["rev-parse", "-q", "--verify", "REBASE_HEAD"])
        .await?;

    if rebase_head_check.success() {
        return Ok(Some(GitOperation::Rebase));
    }

    Ok(None)
}

static DIFF_PATH_NORMALIZER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?(.*)/?$").unwrap());

pub async fn diff_folders(original: &Path, user: &Path) -> Result<String, Error> {
    let diff_command = ScriptEnvironment::new()?
        // These variables aim to ignore the global git config so we get predictable output
        // https://git-scm.com/docs/git#Documentation/git.txt-codeGITCONFIGNOSYSTEMcode
        .with_env_variable("GIT_CONFIG_NOSYSTEM", "1")
        .with_env_variable("HOME", "")
        .with_env_variable("XDG_CONFIG_HOME", "")
        .with_env_variable("USERPROFILE", "")

        .run_exec("git", &[
            "-c", "core.safecrlf=false",
            "diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--ignore-cr-at-eol",
            "--full-index",
            "--no-index",
            "--no-renames",
            "--text",
            // Git quotes the paths containing backslashes in its output, which
            // would prevent us from stripping them below
            &original.to_file_string(),
            &user.to_file_string()
        ])

        .await?
        .output();

    if !diff_command.stderr.is_empty() {
        return Err(Error::DiffFailed(String::from_utf8_lossy(&diff_command.stderr).into_owned()));
    }

    if diff_command.stdout.is_empty() {
        return Err(Error::EmptyDiff);
    }

    let diff
        = String::from_utf8(diff_command.stdout)?;

    let original_path_normalized
        = DIFF_PATH_NORMALIZER.replace(original.as_str(), "/$1/").to_string();
    let user_path_normalized
        = DIFF_PATH_NORMALIZER.replace(user.as_str(), "/$1/").to_string();

    let original_path_escaped
        = regex::escape(&original_path_normalized);
    let user_path_escaped
        = regex::escape(&user_path_normalized);

    let regex
        = Regex::new(format!("(a|b)({}|{})", original_path_escaped, user_path_escaped).as_str()).unwrap();

    let diff
        = regex.replace_all(&diff, "$1/").to_string();

    Ok(diff)
}

fn glob_to_regex(glob: &str) -> String {
    let mut result = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            result.push_str(".*");
            i += 2;
        } else if chars[i] == '*' {
            result.push_str("[^/]*");
            i += 1;
        } else if chars[i] == '?' {
            result.push_str("[^/]");
            i += 1;
        } else {
            if "\\^$.|+()[]{}".contains(chars[i]) {
                result.push('\\');
            }
            result.push(chars[i]);
            i += 1;
        }
    }

    result.push('$');
    result
}

fn matches_approved_git_repository(url: &str, patterns: &[Setting<String>]) -> bool {
    if patterns.is_empty() {
        return false;
    }

    patterns.iter().any(|pattern| {
        let regex_str = glob_to_regex(&pattern.value);
        Regex::new(&regex_str).map_or(false, |re| re.is_match(url))
    })
}

pub(crate) fn validate_repo_url(url: &str, config: &HttpConfig, approved_repos: &[Setting<String>]) -> Result<(), Error> {
    let git_url
        = GitUrl::parse(url)
            .map_err(|_| Error::InvalidGitUrl(url.to_owned()))?;

    if !matches_approved_git_repository(url, approved_repos) {
        return Err(Error::ApprovedGitRepositoriesError(url.to_owned()));
    }

    if let Some(host) = git_url.host {
        let host_url
            = format!("https://{}", host);
        let host_url = Url::parse(&host_url)
            .map_err(|_| Error::InvalidUrl(host_url.to_owned()))?;

        if !config.is_network_enabled(&host_url) {
            return Err(Error::NetworkDisabledError(host_url));
        }
    }

    Ok(())
}

async fn ls_remote(repo: &GitSource, config: &HttpConfig, approved_repos: &[Setting<String>]) -> Result<BTreeMap<String, String>, Error> {
    repeat_until_ok(repo.to_urls(), |url| async move {
        validate_repo_url(&url, config, approved_repos)?;

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

pub async fn resolve_git_treeish(git_range: &GitRange, config: &HttpConfig, approved_repos: &[Setting<String>]) -> Result<String, Error> {
    // Errors from `ls_remote` (unapproved repository, disabled network,
    // unreachable remote, ...) must be reported as-is; only the ref lookup
    // below is allowed to fall through to the next candidate.
    let refs
        = ls_remote(&git_range.repo, config, approved_repos).await?;

    resolve_treeish_from_refs(&refs, &git_range.treeish)
}

fn resolve_treeish_from_refs(refs: &BTreeMap<String, String>, treeish: &GitTreeish) -> Result<String, Error> {
    match treeish {
        GitTreeish::AnythingGoes(treeish) => {
            resolve_treeish_from_refs_stricter(refs, GitTreeish::Commit(treeish.clone()))
                .or_else(|_| resolve_treeish_from_refs_stricter(refs, GitTreeish::Tag(treeish.clone())))
                .or_else(|_| resolve_treeish_from_refs_stricter(refs, GitTreeish::Head(treeish.clone())))
                .map_err(|_| Error::InvalidGitSpecifier)
        },

        _ => {
            resolve_treeish_from_refs_stricter(refs, treeish.clone())
        },
    }
}

fn resolve_treeish_from_refs_stricter(refs: &BTreeMap<String, String>, treeish: GitTreeish) -> Result<String, Error> {
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
            let mut candidates: Vec<(&String, zpm_semver::Version)> = refs.iter()
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

pub(crate) fn make_git_env() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();

    env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());

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

    let approved_repos = &project.config.settings.approved_git_repositories;
    let http_config = &project.http_client.config;

    // Validate that at least one of the source's canonical URLs is approved
    // before attempting any clone path. Without this, the GitHub tarball
    // fast-path in `download_into` would bypass `approvedGitRepositories`.
    let source_urls = source.to_urls();
    let mut last_validation_err: Option<Error> = None;
    let mut any_approved = false;
    for url in &source_urls {
        match validate_repo_url(url, http_config, approved_repos) {
            Ok(()) => { any_approved = true; break; }
            Err(err) => { last_validation_err = Some(err); }
        }
    }
    if !any_approved {
        return Err(last_validation_err.unwrap_or_else(|| {
            Error::ApprovedGitRepositoriesError(source_urls.first().cloned().unwrap_or_default())
        }));
    }

    let clone_dir
        = Path::temp_dir()?;

    if download_into(&source, commit, &clone_dir, &project.http_client).await?.is_some() {
        return Ok(clone_dir);
    }

    let _clone_permit
        = project.clone_limiter
            .acquire()
            .await
            .expect("The clone limiter semaphore should not be closed");

    git_clone_into(source, commit, &clone_dir, http_config, approved_repos).await?;
    Ok(clone_dir)
}

async fn download_into(source: &GitSource, commit: &str, download_dir: &Path, http_client: &std::sync::Arc<crate::http::HttpClient>) -> Result<Option<()>, Error> {
    if github::download_into(source, commit, download_dir, http_client).await?.is_some() {
        return Ok(Some(()));
    }

    Ok(None)
}

async fn git_clone_into(source: &GitSource, commit: &str, clone_dir: &Path, config: &HttpConfig, approved_repos: &[Setting<String>]) -> Result<(), Error> {
    repeat_until_ok(source.to_urls(), |clone_url| async move {
        validate_repo_url(&clone_url, config, approved_repos)?;

        ScriptEnvironment::new()?
            .with_env(make_git_env())
            .run_exec("git", &["clone", "-c", "core.autocrlf=false", &clone_url, &clone_dir.to_native_string()])
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

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT_HASH: &str = "b3562c2798507869edb767da869cd7b85487726d";
    const TAG_HASH: &str = "6e923f7d98a0afbe5b9c7db9d0f0029c1936746c";
    const HEAD_HASH: &str = "4bcc600d20e3a53ea27fa52c4d1fc49cc2d0eabb";

    fn refs() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HEAD".to_string(), HEAD_HASH.to_string()),
            ("refs/heads/main".to_string(), HEAD_HASH.to_string()),
            ("refs/tags/v1.0.1".to_string(), TAG_HASH.to_string()),
        ])
    }

    #[test]
    fn resolves_anything_goes_as_a_commit() {
        let result
            = resolve_treeish_from_refs(&refs(), &GitTreeish::AnythingGoes(COMMIT_HASH.to_string()));

        assert_eq!(result.unwrap(), COMMIT_HASH);
    }

    #[test]
    fn resolves_anything_goes_as_a_tag() {
        let result
            = resolve_treeish_from_refs(&refs(), &GitTreeish::AnythingGoes("v1.0.1".to_string()));

        assert_eq!(result.unwrap(), TAG_HASH);
    }

    #[test]
    fn resolves_anything_goes_as_a_head() {
        let result
            = resolve_treeish_from_refs(&refs(), &GitTreeish::AnythingGoes("main".to_string()));

        assert_eq!(result.unwrap(), HEAD_HASH);
    }

    #[test]
    fn reports_unknown_anything_goes_as_an_invalid_specifier() {
        let result
            = resolve_treeish_from_refs(&refs(), &GitTreeish::AnythingGoes("does-not-exist".to_string()));

        assert!(matches!(result, Err(Error::InvalidGitSpecifier)));
    }

    #[test]
    fn reports_strict_treeish_errors_as_is() {
        let result
            = resolve_treeish_from_refs(&refs(), &GitTreeish::Head("does-not-exist".to_string()));

        assert!(matches!(result, Err(Error::InvalidGitBranch(branch)) if branch == "does-not-exist"));
    }

    #[test]
    fn rejects_every_repository_when_no_pattern_is_approved() {
        assert!(!matches_approved_git_repository("https://github.com/yarnpkg/util-deprecate.git", &[]));
    }

    #[test]
    fn matches_approved_repository_patterns() {
        let patterns = vec![
            Setting::new("https://github.com/yarnpkg/*".to_string(), Default::default()),
        ];

        assert!(matches_approved_git_repository("https://github.com/yarnpkg/util-deprecate.git", &patterns));
        assert!(!matches_approved_git_repository("https://github.com/other/repo.git", &patterns));
        assert!(!matches_approved_git_repository("git@github.com:yarnpkg/util-deprecate.git", &patterns));
    }

    #[test]
    fn reports_the_repository_and_how_to_approve_it() {
        let message
            = Error::ApprovedGitRepositoriesError("https://github.com/yarnpkg/util-deprecate.git".to_string()).to_string();

        assert_eq!(message, "Request to 'https://github.com/yarnpkg/util-deprecate.git' has been blocked because it doesn't match any of the patterns in 'approvedGitRepositories'. To approve it, add a matching pattern (for example 'https://github.com/yarnpkg/util-deprecate.git') to 'approvedGitRepositories' in your .yarnrc.yml, or use '**' to approve all git repositories.");
    }
}
