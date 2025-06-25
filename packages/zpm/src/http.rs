use std::{net::ToSocketAddrs, sync::Arc, time::Duration};

use reqwest::{Client, Response};

use crate::{config::Config, error::Error};

pub struct HttpClient {
    http_retry: usize,
    client: Client,
}

impl HttpClient {
    pub fn new(config: &Config) -> Result<Arc<Self>, Error> {
        let sock_addrs = format!("registry.npmjs.org:443").to_socket_addrs()
            .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?
            .collect::<Vec<_>>();

        let client = reqwest::Client::builder()
            // TODO: Can we avoid hardcoding the DNS resolution? If we don't I get
            // errors due to exhausting the amount of open files when running an
            // install with a lockfile but without cache. I suspect something is
            // not configured properly in the DNS resolver pool.
            .resolve_to_addrs("registry.npmjs.org", &sock_addrs)

            // Connection pooling settings
            .pool_max_idle_per_host(config.user.http_retry.value as usize)
            .pool_idle_timeout(Duration::from_secs(60))

            // Timeout settings
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))

            // HTTP/2 settings (helps with connection reuse)
            .http2_keep_alive_interval(Duration::from_secs(30))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)

            // Enable connection keep-alive
            .tcp_keepalive(Duration::from_secs(60))

            .use_rustls_tls()
            .hickory_dns(true)
            .build()
            .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?;

        Ok(Arc::new(Self {
            http_retry: config.user.http_retry.value as usize,
            client,
        }))
    }

    pub async fn get(&self, url: &str) -> Result<Response, Error> {
        let mut retry_count = 0;

        loop {
            let response
                = self.client.get(url).send().await?;

            if response.status().is_success() || retry_count >= self.http_retry {
                return Ok(response.error_for_status()?);
            }

            retry_count += 1;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}
