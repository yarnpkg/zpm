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
    fn write_file_string<W: std::fmt::Write>(&self, out: &mut W) -> std::fmt::Result {
        match self {
            GitTreeish::AnythingGoes(treeish) => out.write_str(treeish),
            GitTreeish::Head(head) => write!(out, "head={}", head),
            GitTreeish::Commit(commit) => write!(out, "commit={}", commit),
            GitTreeish::Semver(range) => {
                out.write_str("semver=")?;
                range.write_file_string(out)
            },
            GitTreeish::Tag(tag) => write!(out, "tag={}", tag),
        }
    }
}
