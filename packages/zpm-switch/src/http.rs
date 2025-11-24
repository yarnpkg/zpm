use std::sync::LazyLock;

use reqwest::Client;
use zpm_utils::is_ci;

use crate::errors::Error;

static HTTP_CLIENT: LazyLock<Result<Client, Error>> = LazyLock::new(|| {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .build()?;

    Ok(client)
});

pub fn http_client() -> Result<Client, Error> {
    HTTP_CLIENT.clone()
}

pub async fn fetch(url: &str) -> Result<Vec<u8>, Error> {
    let client
        = http_client()?;

    let is_ci_header
        = is_ci()
            .map_or_else(
                || "n/a".to_string(),
                |provider| serde_plain::to_string(&provider).unwrap()
            );

    let request
        = client.get(url)
            .header("User-Agent", "zpm-switch")
            .header("X-Switch-CI", is_ci_header)
            .send().await?;

    let status
        = request.status();

    if !status.is_success() {
        return Err(Error::HttpStatus(status, url.to_string()));
    }

    let data
        = request.bytes().await?;

    Ok(data.to_vec())
}
