use std::sync::{Arc, LazyLock};

use regex::Regex;
use reqwest::StatusCode;
use zpm_formats::iter_ext::IterExt;
use zpm_git::GitSource;
use zpm_utils::Path;

use crate::{
    error::Error,
    http::HttpClient,
};

static GITHUB_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^https://github.com/([^/#]+)/([^/#]+?)\\.git$").unwrap()
});

pub struct GitHubUrl {
    owner: String,
    name: String,
}

pub fn parse_github_url(s: &str) -> Option<GitHubUrl> {
    if let Some(captures) = GITHUB_PATTERN.captures(s) {
        return Some(GitHubUrl {
            owner: captures.get(1).unwrap().as_str().to_string(),
            name: captures.get(2).unwrap().as_str().to_string(),
        });
    }

    None
}

pub fn public_tarball_url(owner: &str, repository: &str, commit: &str) -> String {
    format!("https://github.com/{}/{}/archive/{}.tar.gz", owner, repository, commit)
}

pub async fn download_into(source: &GitSource, commit: &str, download_dir: &Path, http_client: &Arc<HttpClient>) -> Result<Option<()>, Error> {
    let GitSource::GitHub {owner, repository} = source else {
        return Ok(None);
    };

    let response
        = http_client.get(public_tarball_url(owner, &repository, commit))?.send().await;

    let tgz_data = match response {
        Ok(response) => {
            response.bytes().await.map_err(|_| Error::ReplaceMe)?
        },

        Err(err) if err.status() == Some(StatusCode::NOT_FOUND) => {
            return Ok(None);
        },

        Err(err) => {
            return Err(Error::RemoteRegistryError(Arc::new(err)));
        },
    };

    let tar_data
        = zpm_formats::tar::unpack_tgz(&tgz_data)?;

    let entries
        = zpm_formats::tar_iter::TarIterator::new(&tar_data)
            .filter_map(|entry| entry.ok())
            .strip_first_segment()
            .collect::<Vec<_>>();

    zpm_formats::entries_to_disk(&entries, download_dir)?;

    Ok(Some(()))
}
