use std::{net::ToSocketAddrs, sync::Arc, time::Duration};

use itertools::Itertools;
use reqwest::{header::{HeaderName, HeaderValue}, Body, Client, Method, RequestBuilder, Response, Url};
use wax::Program;

use crate::{config::Config, config_fields::{Glob, GlobField}, error::Error, settings::NetworkSettings};

#[derive(Debug)]
pub struct HttpConfig {
    pub http_retry: usize,
    pub unsafe_http_whitelist: Vec<GlobField>,

    enable_network: bool,

    network_settings: Vec<(Glob, NetworkSettings)>,
}

impl HttpConfig {
    pub fn url_settings(&self, url: &Url) -> NetworkSettings {
        let url_settings
            = url.host_str()
                .map(|host_str| {
                    self.network_settings
                        .iter()
                        .fold(NetworkSettings::default(), |existing, (glob, settings)| {
                            if glob.matcher().is_match(host_str) {
                                NetworkSettings {
                                    enable_network: existing.enable_network.or(settings.enable_network),
                                }
                            } else {
                                existing
                            }
                        })
                })
                .unwrap_or_default();

        NetworkSettings {
            enable_network: url_settings.enable_network.or(Some(self.enable_network)),
        }
    }
}

#[derive(Debug)]
pub struct HttpClient {
    pub config: HttpConfig,

    client: Client,
}

#[derive(Debug)]
pub struct HttpRequest<'a> {
    client: &'a HttpClient,
    builder: RequestBuilder,

    retry: bool,
}

impl<'a> HttpRequest<'a> {
    pub fn new(client: &'a HttpClient, url: Url, method: Method, retry: bool) -> Self {
        let builder = client.client.request(method, url);

        Self { builder, client, retry }
    }

    pub async fn send(self) -> Result<Response, reqwest::Error> {
        let mut retry_count = 0;

        // If the request is not retriable, we should avoid cloning the builder.
        if !self.retry {
            return self.builder.send()
                .await?
                .error_for_status();
        }

        loop {
            let response
                = self.builder.try_clone().expect("builder should be clonable").send().await;

            if retry_count < self.client.config.http_retry {
                let is_failure = match &response {
                    Ok(response) => response.status().is_server_error() || matches!(response.status().as_u16(), 408 | 413 | 429),
                    Err(_) => true,
                };

                if is_failure {
                    retry_count += 1;

                    let sleep_duration
                        = 2_u64.saturating_pow(retry_count as u32);
                    let bounded_sleep_duration
                        = std::cmp::min(sleep_duration, 10);

                    tokio::time::sleep(Duration::from_secs(bounded_sleep_duration)).await;
                    continue;
                }
            }

            return response?.error_for_status();
        }
    }

    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        self.builder = self.builder.header(key, value);
        self
    }

    pub fn body(mut self, body: impl Into<Body>) -> Self {
        self.builder = self.builder.body(body);
        self
    }
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

        let config = HttpConfig {
            http_retry: config.user.http_retry.value as usize,
            unsafe_http_whitelist: config.project.unsafe_http_whitelist.value.clone(),

            enable_network: config.user.enable_network.value,

            network_settings: config.user.network_settings.value
                .clone()
                .into_iter()
                // Sort the config by key length to match on the most specific pattern.
                .sorted_by_cached_key(|(glob, _)| -(glob.raw().len() as isize))
                .collect(),
        };

        Ok(Arc::new(Self {
            client,
            config,
        }))
    }

    fn request(&self, url: impl AsRef<str>, method: Method, retry: bool) -> Result<HttpRequest, Error> {
        let url = url.as_ref();

        let url = Url::parse(url)
            .map_err(|_| Error::InvalidUrl(url.to_owned()))?;

        let url_settings = self.config.url_settings(&url);
        if url_settings.enable_network == Some(false) {
            return Err(Error::NetworkDisabledError(url));
        }

        if url.scheme() == "http"
            && !self.config.unsafe_http_whitelist
                .iter()
                .any(|glob| glob.value.matcher().is_match(url.host_str().expect("\"http:\" URL should have a host")))
        {
            return Err(Error::UnsafeHttpError(url));
        }

        Ok(HttpRequest::new(self, url, method, retry))
    }

    pub fn get(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::GET, true)
    }

    pub fn post(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::POST, false)
    }
}
