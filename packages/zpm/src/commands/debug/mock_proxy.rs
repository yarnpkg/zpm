use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use clipanion::cli;
use http_body_util::Full;
use hyper::{body::Bytes, server::conn::http1, service::service_fn, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use reqwest::Url;
use tokio::{net::TcpListener, sync::RwLock};

use crate::error::Error;

#[cli::command]
#[cli::path("debug", "mock-proxy")]
pub struct MockProxy {
    /// The remote URL to proxy requests to (e.g., https://registry.npmjs.org)
    url: String,

    /// The port to listen on (default: 0 for auto-assign)
    #[cli::option("-p,--port", default = 0)]
    port: u16,
}

#[derive(Clone)]
struct CachedResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct ProxyState {
    remote_url: Url,
    local_addr: String,
    cache: RwLock<HashMap<String, CachedResponse>>,
    client: reqwest::Client,
}

impl MockProxy {
    pub async fn execute(&self) -> Result<(), Error> {
        let remote_url
            = Url::parse(&self.url)?;

        let addr
            = SocketAddr::from(([127, 0, 0, 1], self.port));

        let listener
            = TcpListener::bind(addr).await
                .map_err(|e| Error::SerializationError(format!("Failed to bind: {e}")))?;

        let local_addr
            = listener.local_addr()
                .map_err(|e| Error::SerializationError(format!("Failed to get local addr: {e}")))?;

        let local_addr_string
            = format!("http://localhost:{}", local_addr.port());

        println!("Mock proxy server listening on {local_addr_string}");
        println!("Proxying requests to {}", self.url);
        println!("Press Ctrl+C to stop");

        let state
            = Arc::new(ProxyState {
                remote_url,
                local_addr: local_addr_string,
                cache: RwLock::new(HashMap::new()),
                client: reqwest::Client::new(),
            });

        loop {
            let (stream, _)
                = listener.accept().await
                    .map_err(|e| Error::SerializationError(format!("Failed to accept: {e}")))?;

            let io
                = TokioIo::new(stream);
            let state
                = state.clone();

            tokio::spawn(async move {
                let service
                    = service_fn(move |req| {
                        let state = state.clone();
                        handle_request(state, req)
                    });

                let connection_result
                    = http1::Builder::new()
                        .serve_connection(io, service)
                        .await;

                if let Err(err) = connection_result {
                    println!("Error serving connection: {err}");
                }
            });
        }
    }
}

fn not_allowed() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Full::new(Bytes::from("Only GET requests are supported")))
        .unwrap()
}

fn proxy_error(message: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Full::new(Bytes::from(format!("Proxy error: {message}"))))
        .unwrap()
}

async fn handle_request(state: Arc<ProxyState>, req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method
        = req.method().clone();

    let path = req.uri().path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/")
        .to_string();

    if method != Method::GET {
        return Ok(not_allowed());
    }

    {
        let cache
            = state.cache.read().await;

        if let Some(cached) = cache.get(&path) {
            println!("[CACHE] {path}");
            return Ok(build_response(cached, &state));
        }
    }

    println!("[FETCH] {path}");

    let remote_url
        = format!("{}{}", state.remote_url.as_str().trim_end_matches('/'), path);

    match fetch_and_cache(&state, &path, &remote_url).await {
        Ok(cached) => {
            Ok(build_response(&cached, &state))
        },

        Err(e) => {
            println!("[ERROR] {path}: {e}");
            Ok(proxy_error(e))
        }
    }
}

async fn fetch_and_cache(state: &ProxyState, path: &str, remote_url: &str) -> Result<CachedResponse, String> {
    let response = state.client
        .get(remote_url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status
        = response.status().as_u16();

    let headers: HashMap<String, String>
        = response.headers()
            .iter()
            .filter_map(|(name, value)| {
                let name
                    = name.as_str().to_lowercase();

                // Skip transfer-encoding as we'll send the full body
                if name == "transfer-encoding" {
                    return None;
                }

                value.to_str().ok().map(|v| (name, v.to_string()))
            })
            .collect();

    let content_type
        = headers.get("content-type")
            .map(|s| s.as_str())
            .unwrap_or("");

    let body_bytes
        = response.bytes().await
            .map_err(|e| format!("Failed to read body: {e}"))?;

    // If it's JSON, perform URL replacement
    let body = if content_type.contains("application/json") {
        let body_str
            = String::from_utf8_lossy(&body_bytes);

        let remote_base
            = state.remote_url.as_str().trim_end_matches('/');
        let replaced
            = body_str.replace(remote_base, &state.local_addr);

        replaced.into_bytes()
    } else {
        body_bytes.to_vec()
    };

    let cached = CachedResponse {
        status,
        headers,
        body,
    };

    // Store in cache
    {
        let mut cache
            = state.cache.write().await;

        cache.insert(
            path.to_string(),
            cached.clone(),
        );
    }

    Ok(cached)
}

fn build_response(cached: &CachedResponse, state: &ProxyState) -> Response<Full<Bytes>> {
    let mut builder
        = Response::builder()
            .status(StatusCode::from_u16(cached.status).unwrap_or(StatusCode::OK));

    let remote_base
        = state.remote_url.as_str().trim_end_matches('/');

    for (name, value) in &cached.headers {
        // Update content-length since we may have modified the body
        if name == "content-length" {
            builder = builder.header(name, cached.body.len().to_string());
        } else {
            let value
                = value.replace(remote_base, &state.local_addr);

            builder = builder.header(name, value);
        }
    }

    builder.body(Full::new(Bytes::from(cached.body.clone()))).unwrap()
}
