use std::{collections::HashSet, net::SocketAddr, sync::{Arc, LazyLock, OnceLock}, time::Duration};

use bytes::Bytes;
use dashmap::DashMap;
use hickory_resolver::{config::LookupIpStrategy, TokioResolver};
use http::HeaderMap;
use itertools::Itertools;
use reqwest::{dns::{self, Addrs}, header::{HeaderName, HeaderValue}, Body, Client, Method, RequestBuilder, Response, Url};
use tokio::sync::OnceCell;
use wax::Program;
use zpm_config::{Configuration, NetworkSettings, Setting};
use zpm_utils::Glob;

use crate::{
    error::Error,
    report::current_report,
};

static WARNED_HOSTNAMES: LazyLock<tokio::sync::Mutex<HashSet<String>>> = LazyLock::new(|| tokio::sync::Mutex::new(HashSet::new()));

#[derive(Debug)]
pub struct HttpConfig {
    pub enforce_unsafe_http: bool,
    pub http_retry: usize,
    pub unsafe_http_whitelist: Vec<Setting<Glob>>,
    pub slow_network_timeout: u64,

    enable_network: bool,

    network_settings: Vec<(Glob, NetworkSettings)>,
}

impl HttpConfig {
    pub fn is_network_enabled(&self, url: &Url) -> bool {
        let Some(host_str) = url.host_str() else {
            return false;
        };

        for (glob, settings) in &self.network_settings {
            if let Some(enable_network) = settings.enable_network.value {
                if glob.matcher().is_match(host_str) {
                    return enable_network;
                }
            }
        }

        self.enable_network
    }
}

#[derive(Clone)]
struct HickoryDnsResolver {
    state: Arc<OnceLock<TokioResolver>>,
    cache: DashMap<String, Arc<OnceCell<Vec<SocketAddr>>>>,
}

impl Default for HickoryDnsResolver {
    fn default() -> Self {
        Self {
            state: Arc::new(OnceLock::new()),
            cache: DashMap::new(),
        }
    }
}

impl dns::Resolve for HickoryDnsResolver {
    fn resolve(&self, name: dns::Name) -> dns::Resolving {
        let resolver
            = self.clone();

        Box::pin(async move {
            let name_str
                = name.as_str().to_string();

            let cell
                = resolver.cache
                    .entry(name_str)
                    .or_insert_with(|| Arc::new(OnceCell::new()))
                    .clone();

            let result = cell.get_or_try_init(|| async {
                let resolver_instance
                    = resolver.state.get_or_init(new_resolver);

                let lookup
                    = resolver_instance
                        .lookup_ip(name.as_str())
                        .await
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

                let addrs
                    = lookup.into_iter()
                        .map(|ip_addr| SocketAddr::new(ip_addr, 0))
                        .collect::<Vec<_>>();

                Ok::<_, std::io::Error>(addrs)
            }).await?;

            let addrs: Addrs
                = Box::new(result.clone().into_iter());

            Ok(addrs)
        })
    }
}

fn new_resolver() -> TokioResolver {
    let mut builder
        = TokioResolver::builder_tokio()
            .expect("Failed to create a DNS resolver");

    builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    builder.build()
}

pub struct HttpClient {
    pub config: HttpConfig,

    client: Client,

    /// Cache for GET requests to avoid duplicate network calls for the same URL.
    /// Uses OnceCell for each URL to handle concurrent requests to the same URL.
    get_cache: DashMap<String, Arc<OnceCell<Result<Bytes, String>>>>,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("config", &self.config)
            .field("client", &self.client)
            .field("get_cache", &format!("<{} entries>", self.get_cache.len()))
            .finish()
    }
}

#[derive(Debug)]
pub struct HttpRequest<'a> {
    client: &'a HttpClient,
    builder: RequestBuilder,
    enable_retry: bool,
    enable_status_check: bool,
    url: Url,
}

impl<'a> HttpRequest<'a> {
    pub fn new(client: &'a HttpClient, url: Url, method: Method) -> Self {
        let builder
            = client.client.request(method.clone(), url.clone());

        Self {
            builder,
            client,
            enable_retry: method == Method::GET,
            enable_status_check: true,
            url,
        }
    }

    pub fn enable_retry(mut self, enable_retry: bool) -> Self {
        self.enable_retry = enable_retry;
        self
    }

    pub fn enable_status_check(mut self, enable_status_check: bool) -> Self {
        self.enable_status_check = enable_status_check;
        self
    }

    pub async fn send(self) -> Result<Response, reqwest::Error> {
        let mut retry_count
            = 0;

        let hostname
            = self.url.host_str()
                .map(|s| s.to_string());

        loop {
            let mut fetch_future = Box::pin(async {
                self.builder.try_clone()
                    .expect("builder should be clonable")
                    .send()
                    .await
            });

            let warning_future = async {
                tokio::time::sleep(Duration::from_millis(self.client.config.slow_network_timeout)).await;

                // Check if we should warn about this hostname
                if let Some(hostname) = &hostname {
                    let should_warn
                        = WARNED_HOSTNAMES.lock().await
                            .insert(hostname.clone());

                    if should_warn {
                        current_report().await.as_mut().map(|report| {
                            report.warn(format!("Requests to {} are taking suspiciously long...", hostname));
                        });
                    }
                }
            };

            let response = tokio::select! {
                result = &mut fetch_future => result,
                _ = warning_future => {
                    // Warning was issued, now wait for the actual fetch to complete
                    fetch_future.await
                }
            };

            if self.enable_retry && retry_count < self.client.config.http_retry {
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

            return if self.enable_status_check {
                response?.error_for_status()
            } else {
                response
            };
        }
    }

    pub fn headers(&self) -> HeaderMap {
        // TODO: This is filthy
        self.builder.try_clone().unwrap().build().unwrap().headers().clone()
    }

    pub fn add_headers(mut self, headers: Option<HeaderMap>) -> Self {
        if let Some(headers) = headers {
            self.builder = self.builder.headers(headers);
        }

        self
    }

    pub fn header<K, V>(mut self, key: K, value: Option<V>) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        if let Some(value) = value {
            self.builder = self.builder.header(key, value);
        }

        self
    }

    pub fn body(mut self, body: impl Into<Body>) -> Self {
        self.builder = self.builder.body(body);
        self
    }

    pub fn try_clone(&self) -> Option<Self> {
        self.builder.try_clone().map(|builder| Self {
            client: self.client,
            builder,
            enable_retry: self.enable_retry,
            enable_status_check: self.enable_status_check,
            url: self.url.clone(),
        })
    }
}

impl HttpClient {
    pub fn new(config: &Configuration) -> Result<Arc<Self>, Error> {
        let client = reqwest::Client::builder()
            // Connection pooling settings
            .pool_max_idle_per_host(config.settings.network_concurrency.value)
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
            .dns_resolver(Arc::new(HickoryDnsResolver::default()))
            .build()
            .map_err(|err| Error::DnsResolutionError(Arc::new(err)))?;

        let config = HttpConfig {
            enforce_unsafe_http: config.settings.enforce_unsafe_http.value,
            http_retry: config.settings.http_retry.value,
            unsafe_http_whitelist: config.settings.unsafe_http_whitelist.clone(),
            slow_network_timeout: config.settings.slow_network_timeout.value,

            enable_network: config.settings.enable_network.value,

            network_settings: config.settings.network_settings.clone()
                .into_iter()
                // Sort the config by key length to match on the most specific pattern.
                .sorted_by_cached_key(|(glob, _)| -(glob.raw().len() as isize))
                .collect(),
        };

        Ok(Arc::new(Self {
            client,
            config,
            get_cache: DashMap::new(),
        }))
    }

    pub fn request(&self, url: impl AsRef<str>, method: Method) -> Result<HttpRequest, Error> {
        let url
            = url.as_ref();

        let mut url
            = Url::parse(url.as_ref())
                .map_err(|_| Error::InvalidUrl(url.to_owned()))?;

        if !self.config.is_network_enabled(&url) {
            return Err(Error::NetworkDisabledError(url));
        }

        if !self.config.enforce_unsafe_http {
            if url.scheme() == "http" {
                let is_explicitly_allowed
                    = self.config.unsafe_http_whitelist
                        .iter()
                        .any(|glob| glob.value.matcher().is_match(url.host_str().expect("\"http:\" URL should have a host")));

                if !is_explicitly_allowed {
                    return Err(Error::UnsafeHttpError(url));
                }
            }
        } else {
            let _ = url.set_scheme("http");
        }

        Ok(HttpRequest::new(self, url, method))
    }

    pub fn get(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::GET)
    }

    /// Performs a cached GET request. If the URL has already been fetched,
    /// returns the cached response bytes. Concurrent requests to the same URL
    /// will wait for the first request to complete and share the result.
    pub async fn cached_get(&self, url: impl AsRef<str>) -> Result<Bytes, Error> {
        let url_str = url.as_ref().to_string();

        let cell = self.get_cache
            .entry(url_str.clone())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();

        let result = cell.get_or_init(|| async {
            match self.get(&url_str) {
                Ok(request) => {
                    match request.send().await {
                        Ok(response) => {
                            match response.bytes().await {
                                Ok(bytes) => Ok(bytes),
                                Err(e) => Err(e.to_string()),
                            }
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }).await;

        match result {
            Ok(bytes) => Ok(bytes.clone()),
            Err(e) => Err(Error::CachedGetError(e.clone())),
        }
    }

    pub fn post(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::POST)
    }

    pub fn put(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::PUT)
    }
}
