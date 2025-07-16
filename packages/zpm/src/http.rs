use std::{net::ToSocketAddrs, sync::Arc, time::Duration};

use reqwest::{header::{HeaderName, HeaderValue}, Body, Client, Method, RequestBuilder, Response, Url};
use wax::Program;

use crate::{config::Config, config_fields::GlobField, error::Error};

#[derive(Debug)]
pub struct HttpClient {
    http_retry: usize,
    unsafe_http_whitelist: Vec<GlobField>,
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

            if retry_count < self.client.http_retry {
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

        Ok(Arc::new(Self {
            http_retry: config.user.http_retry.value as usize,
            unsafe_http_whitelist: config.project.unsafe_http_whitelist.value.clone(),
            client,
        }))
    }

    fn request(&self, url: impl AsRef<str>, method: Method, retry: bool) -> Result<HttpRequest, Error> {
        let url = url.as_ref();

        let url = Url::parse(url)
            .map_err(|_| Error::InvalidUrl(url.to_owned()))?;

        // TODO: Avoid recreating the glob matchers for every request.
        // This requires the HttpClient to be generic over the lifetime of the Config.
        if url.scheme() == "http"
            && !self.unsafe_http_whitelist
                .iter()
                .any(|glob| glob.value.to_matcher().is_match(url.host_str().expect("\"http:\" URL should have a host")))
        {
            return Err(Error::UnsafeHttpError(url));
        }

        Ok(HttpRequest::new(self, url, method, retry))
    }

    pub fn get(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::GET, true)
    }

    pub fn post(&self, url: impl AsRef<str>, body: impl Into<Body>) -> Result<HttpRequest, Error> {
        self.request(url, Method::POST, false)
            .map(|req| req.body(body))
    }
}
