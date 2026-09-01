use std::{collections::HashSet, future::Future, net::SocketAddr, ops::{Deref, DerefMut}, sync::{Arc, LazyLock, OnceLock}, time::Duration};

use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use hickory_resolver::{config::LookupIpStrategy, TokioResolver};
use http::HeaderMap;
use itertools::Itertools;
#[cfg(not(all(target_arch = "wasm64", target_vendor = "browserpod")))]
use reqwest::Identity;
use reqwest::{dns::{self, Addrs}, header::{HeaderName, HeaderValue}, Body, Certificate, Client, ClientBuilder, Method, Proxy, RequestBuilder, Response, Url};
use tokio::sync::{OnceCell, OwnedSemaphorePermit, Semaphore};
use wax::Program;
use zpm_config::{Configuration, NetworkSettings, Setting};
use zpm_utils::Glob;

use crate::error::Error;

static WARNED_HOSTNAMES: LazyLock<tokio::sync::Mutex<HashSet<String>>> = LazyLock::new(|| tokio::sync::Mutex::new(HashSet::new()));

#[derive(Debug)]
pub struct HttpConfig {
    pub enforce_unsafe_http: bool,
    pub http_retry: usize,
    pub http_timeout: u64,
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
    network_clients: Vec<(Glob, Client)>,
    network_semaphore: Arc<Semaphore>,

    /// Cache for GET requests to avoid duplicate network calls for the same URL.
    /// Uses OnceCell for each URL to handle concurrent requests to the same URL.
    get_cache: DashMap<String, Arc<OnceCell<Result<Bytes, Error>>>>,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("config", &self.config)
            .field("client", &self.client)
            .field("network_clients", &format!("<{} entries>", self.network_clients.len()))
            .field("network_semaphore", &self.network_semaphore)
            .field("get_cache", &format!("<{} entries>", self.get_cache.len()))
            .finish()
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    response: Response,
    permit: Option<OwnedSemaphorePermit>,
}

impl HttpResponse {
    fn new(response: Response, permit: OwnedSemaphorePermit) -> Self {
        Self {response, permit: Some(permit)}
    }

    fn release_permit(&mut self) {
        self.permit.take();
    }

    pub async fn bytes(self) -> Result<Bytes, reqwest::Error> {
        let Self {response, permit} = self;
        let result = response.bytes().await;
        drop(permit);
        result
    }

    pub async fn text(self) -> Result<String, reqwest::Error> {
        let Self {response, permit} = self;
        let result = response.text().await;
        drop(permit);
        result
    }

    pub fn error_for_status(self) -> Result<Self, reqwest::Error> {
        let Self {response, permit} = self;

        response.error_for_status()
            .map(|response| Self {response, permit})
    }
}

impl Deref for HttpResponse {
    type Target = Response;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl DerefMut for HttpResponse {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.response
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
            = client.client_for_url(&url).request(method.clone(), url.clone());

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

    /// Overrides the client-wide timeout for this specific request. It covers
    /// the whole exchange (connection included), which makes it suitable to
    /// bound requests that must not stall the command they're part of. Note
    /// that the request timeout is never allowed to exceed the global
    /// `httpTimeout` setting.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        let bounded_timeout
            = std::cmp::min(timeout, Duration::from_millis(self.client.config.http_timeout));

        self.builder = self.builder.timeout(bounded_timeout);
        self
    }

    async fn send_with<T, F, Fut>(self, consume: F) -> Result<T, reqwest::Error>
    where
        F: Fn(HttpResponse) -> Fut,
        Fut: Future<Output = Result<T, reqwest::Error>>,
    {
        let mut retry_count
            = 0;

        let hostname
            = self.url.host_str()
                .map(|s| s.to_string());

        loop {
            let permit
                = self.client.network_semaphore.clone()
                    .acquire_owned()
                    .await
                    .expect("network semaphore should remain open");

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
                        crate::report::if_active_async(|report| {
                            report.warn(format!("Requests to {} are taking suspiciously long...", hostname));
                        }).await;
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

            let is_failure = match &response {
                Ok(response) => response.status().is_server_error() || matches!(response.status().as_u16(), 408 | 413 | 429),
                Err(_) => true,
            };

            if self.enable_retry && retry_count < self.client.config.http_retry && is_failure {
                retry_count += 1;
                drop(response);
                drop(permit);

                let sleep_duration
                    = 2_u64.saturating_pow(retry_count as u32);
                let bounded_sleep_duration
                    = std::cmp::min(sleep_duration, 10);

                tokio::time::sleep(Duration::from_secs(bounded_sleep_duration)).await;
                continue;
            }

            let response
                = response?;

            let response = if self.enable_status_check {
                response.error_for_status()?
            } else {
                response
            };

            let result
                = consume(HttpResponse::new(response, permit)).await;

            if self.enable_retry && retry_count < self.client.config.http_retry && result.is_err() {
                retry_count += 1;
                drop(result);

                let sleep_duration
                    = 2_u64.saturating_pow(retry_count as u32);
                let bounded_sleep_duration
                    = std::cmp::min(sleep_duration, 10);

                tokio::time::sleep(Duration::from_secs(bounded_sleep_duration)).await;
                continue;
            }

            return result;
        }
    }

    pub async fn send(self) -> Result<HttpResponse, reqwest::Error> {
        self.send_with(|response| async move {
            Ok(response)
        }).await
    }

    pub async fn send_text(self) -> Result<String, reqwest::Error> {
        self.send_with(|response| response.text()).await
    }

    /// Buffers the response body inside the retry loop while retaining the
    /// drained response so callers can inspect its status and headers.
    pub async fn send_bytes(self) -> Result<(HttpResponse, Bytes), reqwest::Error> {
        let enable_status_check
            = self.enable_status_check;

        self.send_with(move |mut response| async move {
            if !enable_status_check
                && (response.status().is_client_error()
                    || response.status().is_server_error()
                    || response.status().as_u16() == 304)
            {
                return Ok((response, Bytes::new()));
            }

            let capacity
                = response.content_length()
                    .and_then(|length| usize::try_from(length).ok())
                    .unwrap_or_default();
            let mut body
                = BytesMut::with_capacity(capacity);

            while let Some(chunk) = response.chunk().await? {
                body.extend_from_slice(&chunk);
            }

            response.release_permit();
            Ok((response, body.freeze()))
        }).await
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
    fn build_client(config: &Configuration, network_settings: Option<&NetworkSettings>) -> Result<Client, Error> {
        let client_builder = reqwest::Client::builder();

        #[cfg(not(all(target_arch = "wasm64", target_vendor = "browserpod")))]
        let client_builder = client_builder.use_rustls_tls();

        let mut client_builder = client_builder
            // Connection pooling settings
            .pool_max_idle_per_host(config.settings.network_concurrency.value)
            .pool_idle_timeout(Duration::from_secs(30))

            // Timeout settings
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_millis(config.settings.http_timeout.value))

            // HTTP/2 settings (helps with connection reuse)
            .http2_keep_alive_interval(Duration::from_secs(30))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)

            // Enable connection keep-alive
            .tcp_keepalive(Duration::from_secs(60))

            .dns_resolver(Arc::new(HickoryDnsResolver::default()));

        if !config.settings.enable_strict_ssl.value {
            client_builder = client_builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }

        if let Some(http_proxy) = config.settings.http_proxy.value.as_ref() {
            client_builder = client_builder.proxy(Proxy::http(http_proxy)?);
        }

        if let Some(https_proxy) = config.settings.https_proxy.value.as_ref() {
            client_builder = client_builder.proxy(Proxy::https(https_proxy)?);
        }

        if let Some(ca_path) = config.settings.https_ca_file_path.value.as_ref() {
            client_builder = Self::add_root_certificate(client_builder, ca_path)?;
        }

        if let Some(settings) = network_settings {
            if let Some(ca_path) = settings.https_ca_file_path.value.as_ref() {
                client_builder = Self::add_root_certificate(client_builder, ca_path)?;
            }
        }

        let (https_cert_file_path, https_key_file_path) = match network_settings {
            Some(settings) if settings.https_cert_file_path.value.is_some() || settings.https_key_file_path.value.is_some() => (
                settings.https_cert_file_path.value.as_ref(),
                settings.https_key_file_path.value.as_ref(),
            ),

            _ => (
                config.settings.https_cert_file_path.value.as_ref(),
                config.settings.https_key_file_path.value.as_ref(),
            ),
        };

        match (https_cert_file_path, https_key_file_path) {
            (Some(cert_path), Some(key_path)) => {
                #[cfg(all(target_arch = "wasm64", target_vendor = "browserpod"))]
                {
                    let _ = (cert_path, key_path);

                    return Err(Error::ConflictingOptions("httpsCertFilePath / httpsKeyFilePath (PEM client identity) require reqwest's rustls-tls feature, currently disabled for the browserpod target".to_string()));
                }

                #[cfg(not(all(target_arch = "wasm64", target_vendor = "browserpod")))]
                {
                    let cert_content
                        = cert_path.fs_read_prealloc()?;

                    let key_content
                        = key_path.fs_read_prealloc()?;

                    let mut identity_content
                        = cert_content;

                    identity_content.push(b'\n');
                    identity_content.extend_from_slice(&key_content);

                    let identity
                        = Identity::from_pem(&identity_content)?;

                    client_builder = client_builder.identity(identity);
                }
            },

            (Some(_), None) | (None, Some(_)) => {
                return Err(Error::ConflictingOptions("httpsCertFilePath and httpsKeyFilePath must be set together".to_string()));
            },

            (None, None) => {}
        }

        let client = client_builder
            .build()
            .map_err(|err| Error::HttpError {
                inner: Arc::new(err),
                extra: Some("Failed to build HTTP client from current network/proxy/TLS settings".to_string()),
            })?;

        Ok(client)
    }

    fn add_root_certificate(client_builder: ClientBuilder, ca_path: &zpm_utils::Path) -> Result<ClientBuilder, Error> {
        let ca_content
            = ca_path.fs_read_prealloc()?;

        let certificate
            = Certificate::from_pem(&ca_content)?;

        Ok(client_builder.add_root_certificate(certificate))
    }

    fn has_tls_settings(settings: &NetworkSettings) -> bool {
        settings.https_ca_file_path.value.is_some()
            || settings.https_cert_file_path.value.is_some()
            || settings.https_key_file_path.value.is_some()
    }

    pub fn new(config: &Configuration) -> Result<Arc<Self>, Error> {
        let network_concurrency
            = config.settings.network_concurrency.value;

        let network_settings: Vec<_> = config.settings.network_settings.clone()
            .into_iter()
            // Sort the config by key length to match on the most specific pattern.
            .sorted_by_cached_key(|(glob, _)| -(glob.raw().len() as isize))
            .collect();

        let client
            = Self::build_client(config, None)?;

        let mut network_clients
            = Vec::new();

        for (glob, settings) in &network_settings {
            if Self::has_tls_settings(settings) {
                network_clients.push((glob.clone(), Self::build_client(config, Some(settings))?));
            }
        }

        let config = HttpConfig {
            enforce_unsafe_http: config.settings.enforce_unsafe_http.value,
            http_retry: config.settings.http_retry.value,
            http_timeout: config.settings.http_timeout.value,
            unsafe_http_whitelist: config.settings.unsafe_http_whitelist.clone(),
            slow_network_timeout: config.settings.slow_network_timeout.value,

            enable_network: config.settings.enable_network.value,

            network_settings,
        };

        Ok(Arc::new(Self {
            client,
            network_clients,
            network_semaphore: Arc::new(Semaphore::new(network_concurrency)),
            config,
            get_cache: DashMap::new(),
        }))
    }

    fn client_for_url(&self, url: &Url) -> &Client {
        let Some(host_str) = url.host_str() else {
            return &self.client;
        };

        for (glob, client) in &self.network_clients {
            if glob.matcher().is_match(host_str) {
                return client;
            }
        }

        &self.client
    }

    pub fn request(&self, url: impl AsRef<str>, method: Method) -> Result<HttpRequest<'_>, Error> {
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

    pub fn get(&self, url: impl AsRef<str>) -> Result<HttpRequest<'_>, Error> {
        self.request(url, Method::GET)
    }

    /// Performs a cached GET request. If the URL has already been fetched,
    /// returns the cached response bytes. Concurrent requests to the same URL
    /// will wait for the first request to complete and share the result.
    pub async fn cached_get(&self, url: impl AsRef<str>) -> Result<Bytes, Error> {
        self.cached_get_with_authorization(url, None).await
    }

    /// Performs a cached GET request with an optional Authorization header.
    /// The credential itself is never retained in the cache key.
    pub async fn cached_get_with_authorization(&self, url: impl AsRef<str>, authorization: Option<&str>) -> Result<Bytes, Error> {
        let url_str
            = url.as_ref().to_string();
        let cache_key = match authorization {
            Some(_) => format!("{}\0authenticated", url_str),
            None => url_str.clone(),
        };

        let cell = self.get_cache
            .entry(cache_key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();

        let result = cell.get_or_init(|| async {
            let request
                = self.get(&url_str)?
                    .header("authorization", authorization);

            let (_, bytes)
                = request.send_bytes().await?;

            Ok(bytes)
        }).await;

        result.clone()
    }

    pub fn post(&self, url: impl AsRef<str>) -> Result<HttpRequest<'_>, Error> {
        self.request(url, Method::POST)
    }

    pub fn put(&self, url: impl AsRef<str>) -> Result<HttpRequest<'_>, Error> {
        self.request(url, Method::PUT)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, convert::Infallible, sync::atomic::{AtomicUsize, Ordering}};

    use futures::stream;
    use http_body_util::{BodyExt, StreamBody};
    use hyper::{body::Frame, server::conn::http1, service::service_fn, Request, StatusCode};
    use hyper_util::rt::TokioIo;
    use tokio::{net::TcpListener, task::JoinHandle};
    use zpm_config::ConfigurationContext;
    use zpm_utils::LastModifiedAt;

    use super::*;

    const REQUEST_COUNT: usize = 20;
    const REQUEST_DELAY: Duration = Duration::from_millis(50);

    #[derive(Default)]
    struct ServerState {
        active: AtomicUsize,
        peak: AtomicUsize,
    }

    struct ActiveRequest(Arc<ServerState>);

    impl ActiveRequest {
        fn new(state: Arc<ServerState>) -> Self {
            let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
            state.peak.fetch_max(active, Ordering::SeqCst);
            Self(state)
        }
    }

    impl Drop for ActiveRequest {
        fn drop(&mut self) {
            self.0.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn test_client(network_concurrency: usize) -> Arc<HttpClient> {
        let context = ConfigurationContext {
            env: BTreeMap::new(),
            user_cwd: None,
            project_cwd: None,
            package_cwd: None,
        };
        let mut last_modified_at = LastModifiedAt::new();
        let mut config = Configuration::load(&context, &mut last_modified_at).unwrap();

        config.settings.enforce_unsafe_http.value = true;
        config.settings.http_retry.value = 0;
        config.settings.network_concurrency.value = network_concurrency;

        HttpClient::new(&config).unwrap()
    }

    async fn start_server() -> (String, Arc<ServerState>, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(ServerState::default());
        let server_state = state.clone();

        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let connection_state = server_state.clone();

                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let request_state = connection_state.clone();

                        async move {
                            let status = if request.uri().path() == "/failure" {
                                StatusCode::INTERNAL_SERVER_ERROR
                            } else {
                                StatusCode::OK
                            };
                            let active_request = ActiveRequest::new(request_state);
                            let body = StreamBody::new(stream::once(async move {
                                tokio::time::sleep(REQUEST_DELAY).await;
                                drop(active_request);
                                Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"ok")))
                            })).boxed();

                            Ok::<_, Infallible>(http::Response::builder()
                                .status(status)
                                .body(body)
                                .unwrap())
                        }
                    });

                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });

        (format!("http://{address}"), state, task)
    }

    #[tokio::test]
    async fn network_concurrency_limits_in_flight_response_bodies() {
        let network_concurrency = 2;
        let client = test_client(network_concurrency);
        let (server_url, state, server_task) = start_server().await;

        futures::future::try_join_all((0..REQUEST_COUNT).map(|request_index| {
            let client = client.clone();
            let url = format!("{server_url}/{request_index}");

            async move {
                client.get(url)?.send().await?.bytes().await?;
                Ok::<_, Error>(())
            }
        })).await.unwrap();

        assert_eq!(state.peak.load(Ordering::SeqCst), network_concurrency);
        assert_eq!(state.active.load(Ordering::SeqCst), 0);

        server_task.abort();
    }

    #[tokio::test]
    async fn network_concurrency_permit_is_released_after_failure() {
        let client = test_client(1);
        let (server_url, state, server_task) = start_server().await;

        assert!(client.get(format!("{server_url}/failure")).unwrap().send().await.is_err());

        tokio::time::timeout(Duration::from_secs(1), async {
            client.get(format!("{server_url}/success"))?.send().await?.bytes().await?;
            Ok::<_, Error>(())
        }).await.unwrap().unwrap();

        assert_eq!(state.peak.load(Ordering::SeqCst), 1);
        assert_eq!(state.active.load(Ordering::SeqCst), 0);

        server_task.abort();
    }
}
