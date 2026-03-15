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

fn create_request(url: &str) -> Result<reqwest::RequestBuilder, Error> {
    let client
        = http_client()?;

    let is_ci_header
        = is_ci()
            .map_or_else(
                || "n/a".to_string(),
                |provider| serde_plain::to_string(&provider).unwrap()
            );

    Ok(
        client.get(url)
            .header("User-Agent", "zpm-switch")
            .header("X-Switch-CI", is_ci_header)
    )
}

async fn send_request(url: &str, request: reqwest::RequestBuilder) -> Result<Vec<u8>, Error> {
    let response
        = request.send().await?;

    let status
        = response.status();

    if !status.is_success() {
        return Err(Error::HttpStatus(status, url.to_string()));
    }

    let data
        = response.bytes().await?;

    Ok(data.to_vec())
}

pub(crate) async fn fetch_from_npm(url: &str) -> Result<Vec<u8>, Error> {
    let mut registry
        = std::env::var("YARN_SWITCH_NPM_REGISTRY")
            .unwrap_or("https://registry.npmjs.org/".into());

    if !registry.ends_with('/') {
        registry.push('/');
    }

    let url = registry + url;
    let mut request = create_request(&url)?;

    if let Ok(token) = std::env::var("YARN_SWITCH_NPM_AUTH_TOKEN") {
        request = request.bearer_auth(token);
    } else if let Ok(mut auth_ident) = std::env::var("YARN_SWITCH_NPM_AUTH_IDENT") {
        if auth_ident.contains(':') {
            auth_ident = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth_ident);
        }

        request = request.header(
            reqwest::header::AUTHORIZATION,
           format!("Basic {auth_ident}")
        );
    }

    send_request(
       &url, request
    ).await
}

pub async fn fetch(url: &str) -> Result<Vec<u8>, Error> {
    send_request(
       url, create_request(url)?
    ).await
}
