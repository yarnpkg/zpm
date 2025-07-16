use std::{net::ToSocketAddrs, sync::Arc, time::Duration};

use reqwest::{Client, Response, Url};
use wax::Program;

use crate::{config::Config, config_fields::GlobField, error::Error};

pub struct HttpClient {
    http_retry: usize,
    unsafe_http_whitelist: Vec<GlobField>,
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
            unsafe_http_whitelist: config.project.unsafe_http_whitelist.value.clone(),
            client,
        }))
    }

    pub async fn get(&self, url: impl AsRef<str>) -> Result<Response, Error> {
        let url = url.as_ref();

        let url = Url::parse(url)
            .map_err(|_| Error::InvalidUrl(url.to_owned()))?;

        // TODO: Avoid recreating the glob matcher for every request.
        // This requires the HttpClient to be generic over the lifetime of the Config.
        if url.scheme() == "http" && !self.unsafe_http_whitelist.iter().any(|glob| glob.value.to_matcher().is_match(url.host_str().unwrap())) {
            return Err(Error::UnsafeHttpError(url));
        }

        let mut retry_count = 0;

        loop {
            let response
                = self.client.get(url.clone()).send().await;

            let is_failure = match &response {
                Ok(response) => response.status().is_server_error() || matches!(response.status().as_u16(), 408 | 413 | 429),
                Err(_) => true,
            };

            if is_failure && retry_count < self.http_retry {
                retry_count += 1;

                let sleep_duration
                    = 2_u64.saturating_pow(retry_count as u32);
                let bounded_sleep_duration
                    = std::cmp::min(sleep_duration, 10);

                tokio::time::sleep(Duration::from_secs(bounded_sleep_duration)).await;
                continue;
            }

            return Ok(response?.error_for_status()?);
        }
    }

    // TODO: Don't expose the client directly, instead provide methods that ensure the configuration is respected.
    pub fn client(&self) -> &Client {
        &self.client
    }
}
