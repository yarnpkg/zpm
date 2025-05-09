use std::sync::LazyLock;

use reqwest::Client;

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

    let request
        = client.get(url).send().await?;

    let status
        = request.status();

    if !status.is_success() {
        return Err(Error::HttpStatus(status));
    }

    let data
        = request.bytes().await?;

    Ok(data.to_vec())
}
