use rkyv::Archive;
use serde::{Deserialize, Serialize};
use zpm_utils::ToFileString;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub enum GitTreeish {
    AnythingGoes(String),
    Head(String),
    Commit(String),
    Semver(zpm_semver::Range),
    Tag(String),
}

impl ToFileString for GitTreeish {
    fn to_file_string(&self) -> String {
        match self {
            GitTreeish::AnythingGoes(treeish) => treeish.to_string(),
            GitTreeish::Head(head) => format!("head={}", head),
            GitTreeish::Commit(commit) => format!("commit={}", commit),
            GitTreeish::Semver(range) => format!("semver={}", range.to_file_string()),
            GitTreeish::Tag(tag) => format!("tag={}", tag),
        }
    }
}
