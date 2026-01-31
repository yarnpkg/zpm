use std::convert::Infallible;

use rkyv::Archive;
use serde::{Deserialize, Serialize};
use zpm_utils::{DataType, FromFileString, ToFileString, ToHumanString};

use crate::{normalize_git_url, GH_TARBALL_URL, GH_URL};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub enum GitSource {
    GitHub { owner: String, repository: String },
    Url(String),
}

impl GitSource {
    pub fn to_urls(&self) -> Vec<String> {
        match self {
            GitSource::GitHub { owner, repository } => {
                vec![
                    format!("git@github.com:{}/{}.git", owner, repository),
                    format!("https://github.com/{}/{}.git", owner, repository),
                ]
            },

            GitSource::Url(url) => vec![
                url.clone(),
            ],
        }
    }
}

impl FromFileString for GitSource {
    type Error = Infallible;

    fn from_file_string(value: &str) -> Result<Self, Self::Error> {
        // Normalize the URL first to handle various GitHub URL formats
        let normalized
            = normalize_git_url(value);

        // Check if it's a GitHub URL
        if let Ok(Some(captures)) = GH_URL.captures(&normalized) {
            if let (Some(owner), Some(repo)) = (captures.get(1), captures.get(2)) {
                return Ok(GitSource::GitHub {
                    owner: owner.as_str().to_string(),
                    repository: repo.as_str().to_string(),
                });
            }
        }

        // Check GitHub tarball URLs (on the original URL, not normalized)
        // TODO: Do we need this? Wouldn't tarball URLs be handled by the previous block anyway?
        if let Ok(Some(captures)) = GH_TARBALL_URL.captures(value) {
            if let (Some(owner), Some(repo)) = (captures.get(1), captures.get(2)) {
                return Ok(GitSource::GitHub {
                    owner: owner.as_str().to_string(),
                    repository: repo.as_str().to_string(),
                });
            }
        }

        // Otherwise, treat it as a generic URL
        Ok(GitSource::Url(value.to_string()))
    }
}

impl ToFileString for GitSource {
    fn write_file_string<W: std::fmt::Write>(&self, out: &mut W) -> std::fmt::Result {
        match self {
            GitSource::GitHub { owner, repository } => {
                out.write_str("github:")?;
                out.write_str(owner)?;
                out.write_str("/")?;
                out.write_str(repository)
            },

            GitSource::Url(url) => {
                out.write_str(url)
            },
        }
    }
}

impl ToHumanString for GitSource {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::Custom(135, 175, 255).colorize(&buffer)
    }
}
