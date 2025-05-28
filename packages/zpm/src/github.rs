use std::sync::{Arc, LazyLock};

use regex::Regex;
use reqwest::StatusCode;
use zpm_utils::Path;

use crate::{error::Error, http::http_client};

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

pub fn public_tarball_url(parsed: &GitHubUrl, commit: &str) -> String {
    format!("https://github.com/{}/{}/archive/{}.tar.gz", parsed.owner, parsed.name, commit)
}

pub async fn download_into(normalized_repo_url: &str, commit: &str, download_dir: &Path) -> Result<Option<()>, Error> {
    let Some(repository) = parse_github_url(normalized_repo_url) else {
        return Ok(None);
    };

    let client
        = http_client()?;

    let response
        = client.get(public_tarball_url(&repository, commit)).send().await
            .and_then(|response| response.error_for_status());

    let data = match response {
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

    let uncompressed_data
        = zpm_formats::convert::convert_tar_gz_to_tar(data)?;

    let entries
        = zpm_formats::tar_iter::TarIterator::new(&uncompressed_data)
            .collect::<Result<Vec<_>, _>>()?;

    let entries
        = zpm_formats::strip_first_segment(entries);

    zpm_formats::entries_to_disk(&entries, download_dir)?;

    Ok(Some(()))
}
