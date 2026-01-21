use rkyv::Archive;
use zpm_utils::{DataType, FromFileString, QueryString, QueryStringValue, ToFileString, ToHumanString, UnwrapInfallible};

use crate::{is_git_url, normalize_git_url, Error, GitSource, GitTreeish};

#[derive(Clone, Default, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct PrepareParams {
    pub cwd: Option<String>,
    pub workspace: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct GitRange {
    pub repo: GitSource,
    pub treeish: GitTreeish,
    pub prepare_params: PrepareParams,
}

impl FromFileString for GitRange {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        // TODO: I have the feeling we should do the other way around: first normalize, then validate.
        // Otherwise I'm concerned we'd forget to normalize some patterns.

        if !is_git_url(src) {
            return Err(Error::InvalidGitUrl(src.to_string()));
        }

        let normalized
            = normalize_git_url(src);

        let Some(hash_index) = normalized.find('#') else {
            return Ok(GitRange {
                repo: GitSource::from_file_string(&normalized).unwrap_infallible(),
                treeish: GitTreeish::Head("HEAD".to_string()),
                prepare_params: PrepareParams::default(),
            });
        };

        let mut treeish
            = GitTreeish::Head("HEAD".to_string());
        let mut prepare_params
            = PrepareParams::default();

        let (repo_str, qs_str)
            = normalized.split_at(hash_index);

        let qs_str
            = qs_str[1..].to_string();

        let repo
            = GitSource::from_file_string(repo_str)
                .unwrap_infallible();

        if !qs_str.contains('=') {
            treeish = GitTreeish::AnythingGoes(qs_str.to_string());
        } else {
            let qs
                = QueryString::from_file_string(&qs_str)?;

            for (key, value) in qs.fields {
                if let QueryStringValue::String(value) = value {
                    match key.as_str() {
                        "head" => {
                            treeish = GitTreeish::Head(value.to_string());
                        },

                        "commit" => {
                            treeish = GitTreeish::Commit(value.to_string());
                        },

                        "semver" => {
                            treeish = GitTreeish::Semver(zpm_semver::Range::from_file_string(value.as_ref())?);
                        },

                        "tag" => {
                            treeish = GitTreeish::Tag(value.to_string());
                        },

                        "cwd" => {
                            prepare_params.cwd = Some(value.to_string());
                        },

                        "workspace" => {
                            prepare_params.workspace = Some(value.to_string());
                        },

                        _ => {
                            // Skip unknown keys
                        },
                    }
                }
            }
        }

        return Ok(GitRange {
            repo,
            treeish,
            prepare_params,
        });
    }
}

impl ToFileString for GitRange {
    fn to_file_string(&self) -> String {
        let mut params
            = vec![];

        params.push(match &self.treeish {
            GitTreeish::AnythingGoes(treeish) => {
                treeish.to_string()
            },

            GitTreeish::Head(head) => {
                format!("head={}", head)
            },

            GitTreeish::Commit(commit) => {
                format!("commit={}", commit)
            },

            GitTreeish::Semver(range) => {
                format!("semver={}", range.to_file_string())
            },

            GitTreeish::Tag(tag) => {
                format!("tag={}", tag)
            },
        });

        if let Some(cwd) = &self.prepare_params.cwd {
            params.push(format!("cwd={}", urlencoding::encode(cwd)));
        }

        if let Some(workspace) = &self.prepare_params.workspace {
            params.push(format!("workspace={}", urlencoding::encode(workspace)));
        }

        format!("{}#{}", self.repo.to_file_string(), params.join("&"))
    }
}

impl ToHumanString for GitRange {
    fn to_print_string(&self) -> String {
        DataType::Custom(135, 175, 255).colorize(&self.to_file_string())
    }
}
