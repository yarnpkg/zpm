use std::sync::LazyLock;

use reqwest::Client;
use zpm_utils::is_ci;

use crate::errors::Error;

static HTTP_CLIENT: LazyLock<Result<Client, Error>> = LazyLock::new(|| {
    let builder = reqwest::Client::builder();

    #[cfg(not(all(target_arch = "wasm64", target_vendor = "browserpod")))]
    let builder = builder.use_rustls_tls();

    let client = builder
        .build()?;

    Ok(client)
});

pub fn http_client() -> Result<Client, Error> {
    HTTP_CLIENT.clone()
}

fn get_npm_registry_server() -> String {
    std::env::var("YARNSW_NPM_REGISTRY_SERVER")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string())
}

fn is_npm_registry_request(url: &str) -> bool {
    let registry = get_npm_registry_server();
    let normalized_registry
        = registry.trim_end_matches('/');
    let normalized_url
        = url.trim_end_matches('/');

    normalized_url == normalized_registry
        || normalized_url.starts_with(&format!("{normalized_registry}/"))
}

fn attach_auth_headers(request: &mut reqwest::RequestBuilder) {
    if let Ok(token) = std::env::var("YARNSW_NPM_AUTH_TOKEN") {
        if let Some(cloned_request) = request.try_clone() {
            *request = cloned_request.bearer_auth(token);
        }

        return;
    }

    if let Ok(mut auth_ident) = std::env::var("YARNSW_NPM_AUTH_IDENT") {
        if auth_ident.contains(':') {
            auth_ident = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth_ident);
        }

        if let Some(cloned_request) = request.try_clone() {
            *request = cloned_request.header(
                reqwest::header::AUTHORIZATION,
                format!("Basic {auth_ident}")
            );
        }
    }
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

    let mut request
        = client.get(url)
            .header("User-Agent", "zpm-switch")
            .header("X-Switch-CI", is_ci_header);

    if is_npm_registry_request(url) {
        attach_auth_headers(&mut request);
    }

    let request
        = request.send().await?;

    let status
        = request.status();

    if !status.is_success() {
        return Err(Error::HttpStatus(status, url.to_string()));
    }

    let data
        = request.bytes().await?;

    Ok(data.to_vec())
}
