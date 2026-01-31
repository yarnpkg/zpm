use rkyv::Archive;

use zpm_utils::{DataType, FromFileString, QueryString, QueryStringValue, ToFileString, ToHumanString, UnwrapInfallible};

use crate::{range::PrepareParams, Error, GitRange, GitSource, GitTreeish};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct GitReference {
    pub repo: GitSource,
    pub commit: String,
    pub prepare_params: PrepareParams,
}

impl GitReference {
    pub fn to_git_range(&self) -> GitRange {
        GitRange {
            repo: self.repo.clone(),
            treeish: GitTreeish::Commit(self.commit.clone()),
            prepare_params: self.prepare_params.clone(),
        }
    }
}

impl FromFileString for GitReference {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let mut parts
            = src.splitn(2, '#');

        let repo_str
            = parts.next()
                .ok_or_else(|| Error::InvalidGitUrl(src.to_string()))?;

        let qs_str
            = parts.next()
                .ok_or_else(|| Error::InvalidGitUrl(src.to_string()))?;

        let qs
            = QueryString::from_file_string(qs_str)?;

        let mut commit
            = None;
        let mut prepare_params
            = PrepareParams::default();

        for (key, value) in qs.fields {
            if let QueryStringValue::String(value) = value {
                match key.as_str() {
                    "commit" => {
                        commit = Some(value.to_string())
                    },

                    "cwd" => {
                        prepare_params.cwd = Some(value.to_string())
                    },

                    "workspace" => {
                        prepare_params.workspace = Some(value.to_string())
                    },

                    _ => {
                        // Skip unknown query string parameters
                    },
                };
            }
        }

        let commit
            = commit
                .ok_or_else(|| Error::InvalidGitUrl(src.to_string()))?;

        Ok(GitReference {
            repo: GitSource::from_file_string(&repo_str).unwrap_infallible(),
            commit,
            prepare_params,
        })
    }
}

impl ToFileString for GitReference {
    fn write_file_string<W: std::fmt::Write>(&self, out: &mut W) -> std::fmt::Result {
        self.repo.write_file_string(out)?;
        out.write_str("#")?;

        let commit = urlencoding::encode(&self.commit);
        write!(out, "commit={}", commit)?;

        if let Some(cwd) = &self.prepare_params.cwd {
            out.write_str("&")?;
            let cwd = urlencoding::encode(cwd);
            write!(out, "cwd={}", cwd)?;
        }

        if let Some(workspace) = &self.prepare_params.workspace {
            out.write_str("&")?;
            let workspace = urlencoding::encode(workspace);
            write!(out, "workspace={}", workspace)?;
        }

        Ok(())
    }
}

impl ToHumanString for GitReference {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::Custom(135, 175, 255).colorize(&buffer)
    }
}
