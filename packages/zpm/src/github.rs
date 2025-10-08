use std::sync::Arc;

use reqwest::StatusCode;
use zpm_formats::iter_ext::IterExt;
use zpm_git::GitSource;
use zpm_utils::Path;

use crate::{
    error::Error,
    http::HttpClient,
};

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
