use std::sync::LazyLock;

use fancy_regex::Regex;

mod error;
mod range;
mod reference;
mod source;
mod treeish;

pub use crate::{
    error::Error,
    range::GitRange,
    reference::GitReference,
    source::GitSource,
    treeish::GitTreeish,
};

pub(crate) static GH_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:github:|(?:https):\/\/github\.com\/|git(?:\+ssh)?:\/\/(?:git@)?github\.com\/|git@github\.com:)?(?!\.{1,2}\/)([a-zA-Z0-9._-]+)\/(?!\.{1,2}(?:#|$))([a-zA-Z0-9._-]+?)(?:\.git)?(#.*)?$").unwrap());
pub(crate) static GH_TARBALL_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?:\/\/github\.com\/(?!\.{1,2}\/)([a-zA-Z0-9._-]+)\/(?!\.{1,2}(?:#|$))([a-zA-Z0-9._-]+?)\/tarball\/(.+)?$").unwrap());

pub(crate) static GH_URL_SET: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    Regex::new(r"^ssh:").unwrap(),
    Regex::new(r"^git(?:\+[^:]+)?:").unwrap(),

    // `git+` is optional, `.git` is required
    Regex::new(r"^(?:git\+)?https?:[^#]+\/[^#]+(?:\.git)(?:#.*)?$").unwrap(),

    Regex::new(r"^git@[^#]+\/[^#]+\.git(?:#.*)?$").unwrap(),
    // Also match git@github.com:user/repo format (with colon)
    Regex::new(r"^git@github\.com:[^/]+/[^/]+(?:\.git)?(?:#.*)?$").unwrap(),

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
