use std::{collections::HashMap, net::SocketAddr, sync::{Arc, OnceLock}, time::Duration};

use hickory_resolver::{config::LookupIpStrategy, TokioResolver};
use itertools::Itertools;
use reqwest::{dns::{self, Addrs}, header::{HeaderName, HeaderValue}, Body, Client, Method, RequestBuilder, Response, Url};
use tokio::sync::{Mutex, broadcast};
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

enum DnsEntry {
    Cached(Vec<SocketAddr>),
    InProgress(broadcast::Receiver<Result<Vec<SocketAddr>, String>>),
}

#[derive(Clone)]
struct HickoryDnsResolver {
    state: Arc<OnceLock<TokioResolver>>,
    /// Cache for DNS resolution results to avoid concurrent lookups for the same domain
    cache: Arc<Mutex<HashMap<String, DnsEntry>>>,
}

impl Default for HickoryDnsResolver {
    fn default() -> Self {
        Self {
            state: Arc::new(OnceLock::new()),
            cache: Arc::new(Mutex::new(HashMap::new())),
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

            // Check cache and handle in-progress lookups
            let receiver = {
                let mut cache
                    = resolver.cache.lock().await;

                match cache.get_mut(&name_str) {
                    Some(DnsEntry::Cached(addrs)) => {
                        // Return cached result
                        let addrs: Addrs
                            = Box::new(addrs.clone().into_iter());

                        return Ok(addrs);
                    },

                    Some(DnsEntry::InProgress(receiver)) => {
                        // Another lookup is in progress, wait for it
                        Some(receiver.resubscribe())
                    },

                    None => {
                        let (tx, rx)
                            = broadcast::channel(1);

                        cache.insert(name_str.clone(), DnsEntry::InProgress(rx));
                        drop(cache); // Release lock before doing DNS resolution

                        // Perform DNS resolution
                        let resolver_instance
                            = resolver.state.get_or_init(new_resolver);

                        let result = resolver_instance
                            .lookup_ip(name.as_str()).await
                            .map(|lookup| lookup.into_iter().map(|ip_addr| SocketAddr::new(ip_addr, 0)).collect_vec())
                            .map_err(|e| e.to_string());

                        // Update cache with result
                        let mut cache
                            = resolver.cache.lock().await;

                        match &result {
                            Ok(addrs) => {
                                cache.insert(name_str.clone(), DnsEntry::Cached(addrs.clone()));
                            },

                            Err(_) => {
                                // Remove failed entry so it can be retried
                                cache.remove(&name_str);
                            }
                        }

                        // Notify any waiting tasks
                        let _ = tx.send(result.clone());

                        // Return the result
                        match result {
                            Ok(addrs) => {
                                return Ok(Box::new(addrs.into_iter()));
                            },

                            Err(e) => {
                                return Err(std::io::Error::new(std::io::ErrorKind::Other, e).into());
                            },
                        }
                    },
                }
            };

            // Wait for in-progress lookup
            if let Some(mut receiver) = receiver {
                match receiver.recv().await {
                    Ok(Ok(addrs)) => {
                        Ok(Box::new(addrs.into_iter()))
                    },

                    Ok(Err(e)) => {
                        Err(std::io::Error::new(std::io::ErrorKind::Other, e).into())
                    },

                    Err(_) => {
                        // Broadcast channel closed unexpectedly, retry the lookup
                        // This is a fallback that shouldn't normally happen
                        let resolver_instance
                            = resolver.state.get_or_init(new_resolver);

                        let lookup
                            = resolver_instance.lookup_ip(name.as_str()).await?;

                        let addrs_vec: Vec<SocketAddr> = lookup
                            .into_iter()
                            .map(|ip_addr| SocketAddr::new(ip_addr, 0))
                            .collect();

                        // Try to cache the result
                        let mut cache
                            = resolver.cache.lock().await;

                        cache.insert(name_str, DnsEntry::Cached(addrs_vec.clone()));

                        Ok(Box::new(addrs_vec.into_iter()))
                    },
                }
            } else {
                unreachable!("Should have a receiver when an in-progress lookup is found");
            }
        })
    }
}



/// Create a new resolver with the default configuration,
/// which reads from `/etc/resolve.conf`. The options are
/// overridden to look up for both IPv4 and IPv6 addresses
/// to work with "happy eyeballs" algorithm.
fn new_resolver() -> TokioResolver {
    let mut builder
        = TokioResolver::builder_tokio()
            .expect("Failed to create a DNS resolver");

    builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    builder.build()
}

#[derive(Debug)]
pub struct HttpClient {
    pub config: HttpConfig,

    client: Client,
}

#[derive(Debug, Clone, Copy)]
pub struct HttpRequestParams {
    pub retry: bool,
    pub strict: bool,
}

impl Default for HttpRequestParams {
    fn default() -> Self {
        Self {
            retry: false,
            strict: true,
        }
    }
}

#[derive(Debug)]
pub struct HttpRequest<'a> {
    client: &'a HttpClient,
    builder: RequestBuilder,
    params: HttpRequestParams,
}

impl<'a> HttpRequest<'a> {
    pub fn new(client: &'a HttpClient, url: Url, method: Method, params: HttpRequestParams) -> Self {
        let builder
            = client.client.request(method, url);

        Self { builder, client, params }
    }

    pub fn ack_response(response: Response, params: HttpRequestParams) -> Result<Response, reqwest::Error> {
        if params.strict {
            Ok(response.error_for_status()?)
        } else {
            Ok(response)
        }
    }

    pub async fn send(self) -> Result<Response, reqwest::Error> {
        let mut retry_count
            = 0;

        // If the request is not retriable, we should avoid cloning the builder.
        if !self.params.retry {
            return Self::ack_response(self.builder.send().await?, self.params);
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

            return Self::ack_response(response?, self.params);
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

    pub fn try_clone(&self) -> Option<Self> {
        self.builder.try_clone().map(|builder| Self {
            client: self.client,
            builder,
            params: self.params,
        })
    }
}

impl HttpClient {
    pub fn new(config: &Config) -> Result<Arc<Self>, Error> {
        let client = reqwest::Client::builder()
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
            .dns_resolver(Arc::new(HickoryDnsResolver::default()))
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

    pub fn request(&self, url: impl AsRef<str>, method: Method, params: HttpRequestParams) -> Result<HttpRequest, Error> {
        let url
            = url.as_ref();

        let url
            = Url::parse(url.as_ref())
                .map_err(|_| Error::InvalidUrl(url.to_owned()))?;

        let url_settings
            = self.config.url_settings(&url);

        if url_settings.enable_network == Some(false) {
            return Err(Error::NetworkDisabledError(url));
        }

        if url.scheme() == "http" {
            let is_explicitly_allowed
                = self.config.unsafe_http_whitelist
                    .iter()
                    .any(|glob| glob.value.matcher().is_match(url.host_str().expect("\"http:\" URL should have a host")));

            if !is_explicitly_allowed {
                return Err(Error::UnsafeHttpError(url));
            }
        }

        Ok(HttpRequest::new(self, url, method, params))
    }

    pub fn get(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::GET, HttpRequestParams {
            retry: true,
            ..Default::default()
        })
    }

    pub fn post(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::POST, HttpRequestParams {
            retry: false,
            ..Default::default()
        })
    }

    pub fn put(&self, url: impl AsRef<str>) -> Result<HttpRequest, Error> {
        self.request(url, Method::PUT, HttpRequestParams {
            retry: false,
            ..Default::default()
        })
    }
}
