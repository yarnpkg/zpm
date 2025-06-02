use std::sync::{Arc, LazyLock};

use reqwest::{Client, Response};

use crate::error::Error;

static HTTP_CLIENT: LazyLock<Result<Client, Error>> = LazyLock::new(|| {
    // let sock_addrs = format!("registry.npmjs.org:443").to_socket_addrs()
    //     .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?
    //     .collect::<Vec<_>>();

    let client = reqwest::Client::builder()
    //    .resolve_to_addrs("registry.npmjs.org", &sock_addrs)
        .use_rustls_tls()
        .build()
        .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?;

    Ok(client)
});

pub fn http_client() -> Result<Client, Error> {
    HTTP_CLIENT.clone()
}

pub async fn http_get(url: &str) -> Result<Response, Error> {
    let client
        = http_client()?;

    let response = client.get(url).send().await?
        .error_for_status()?;

    Ok(response)
}

pub fn is_too_many_open_files(err: &dyn std::error::Error) -> bool {
    let mut source = err.source();

    while let Some(err) = source {
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            if io_err.raw_os_error() == Some(24) {
                return true;
            }
        }

        source = err.source();
    }

    false
}
