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
            .pool_max_idle_per_host(config.user.network_concurrency.value as usize)
            .pool_idle_timeout(Duration::from_secs(30))

            // Timeout settings
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(300))

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
                = self.client.get(url).send().await;

            let is_failure = match &response {
                Ok(response) => response.status().is_server_error() || matches!(response.status().as_u16(), 408 | 413 | 429),
                Err(_) => true,
            };

            if is_failure && retry_count < self.http_retry {
                retry_count += 1;

                let sleep_duration
                    = 1000_u64 * 2_u64.pow(retry_count as u32);

                tokio::time::sleep(Duration::from_millis(sleep_duration)).await;
                continue;
            }

            return Ok(response?.error_for_status()?);
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}
