use std::{net::ToSocketAddrs, sync::Arc};

use once_cell::sync::Lazy;
use reqwest::Client;

use crate::error::Error;

static HTTP_CLIENT: Lazy<Result<Client, Error>> = Lazy::new(|| {
    let sock_addrs = format!("registry.npmjs.org:443").to_socket_addrs()
        .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?
        .collect::<Vec<_>>();

    let client = reqwest::Client::builder()
        .resolve_to_addrs("registry.npmjs.org", &sock_addrs)
        .use_rustls_tls()
        .build()
        .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?;

    Ok(client)
});

pub fn http_client() -> Result<Client, Error> {
    HTTP_CLIENT.clone()
}
