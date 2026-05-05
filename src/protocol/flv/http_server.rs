//! HTTP-FLV server for streaming FLV over HTTP

use super::{FlvEncoder, FlvError, FlvResult};
use crate::media::router::{StreamId, StreamSubscriber};
use crate::media::{MediaFrame, StreamRouter};
use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

/// HTTP-FLV server configuration
#[derive(Debug, Clone)]
pub struct HttpFlvConfig {
    /// Server bind address
    pub bind_addr: String,
    /// CORS allow origin
    pub cors_origin: Option<String>,
    /// Buffer size for each connection
    pub buffer_size: usize,
    /// Connection timeout
    pub timeout: Duration,
    /// Enable gzip compression
    pub enable_gzip: bool,
}

impl Default for HttpFlvConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".to_string(),
            cors_origin: Some("*".to_string()),
            buffer_size: 1024,
            timeout: Duration::from_secs(30),
            enable_gzip: false,
        }
    }
}

/// HTTP-FLV server for streaming media over HTTP
///
/// This server allows browsers and players to consume live streams
/// using the FLV format over HTTP connections.
pub struct HttpFlvServer {
    router: Arc<StreamRouter>,
    config: HttpFlvConfig,
}

impl HttpFlvServer {
    pub fn new(router: Arc<StreamRouter>, config: HttpFlvConfig) -> Self {
        Self { router, config }
    }

    pub fn with_defaults(router: Arc<StreamRouter>) -> Self {
        Self::new(router, HttpFlvConfig::default())
    }

    /// Start the HTTP server
    pub async fn run(&self) -> FlvResult<()> {
        let app = self.create_router();

        let listener = tokio::net::TcpListener::bind(&self.config.bind_addr)
            .await
            .map_err(|e| FlvError::Io(e))?;

        info!(addr = %self.config.bind_addr, "HTTP-FLV server starting");

        axum::serve(listener, app)
            .await
            .map_err(|e| FlvError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    fn create_router(&self) -> axum::Router {
        use axum::routing::get;
        use tower_http::cors::{Any, CorsLayer};

        let state = ServerState {
            router: Arc::clone(&self.router),
            config: self.config.clone(),
        };

        let cors = if let Some(ref origin) = self.config.cors_origin {
            if origin == "*" {
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([http::Method::GET, http::Method::OPTIONS])
                    .allow_headers(Any)
            } else {
                CorsLayer::new()
                    .allow_origin(tower_http::cors::AllowOrigin::exact(
                        origin.parse().expect("Invalid CORS origin")
                    ))
                    .allow_methods([http::Method::GET, http::Method::OPTIONS])
                    .allow_headers(Any)
            }
        } else {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([http::Method::GET, http::Method::OPTIONS])
                .allow_headers(Any)
        };

        axum::Router::new()
            .route("/live/:stream", get(handle_live_stream))
            .route("/health", get(health_check))
            .layer(cors)
            .with_state(state)
    }
}

/// Server state shared across handlers
#[derive(Clone)]
struct ServerState {
    router: Arc<StreamRouter>,
    config: HttpFlvConfig,
}

/// Handle live stream request
async fn handle_live_stream(
    axum::extract::State(state): axum::extract::State<ServerState>,
    axum::extract::Path(stream_name): axum::extract::Path<String>,
) -> axum::response::Response {
    let stream_id = StreamId::new(stream_name.clone());

    // Check if stream exists
    if !state.router.has_stream(&stream_id) {
        return axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from(format!(
                "Stream '{}' not found",
                stream_name
            )))
            .unwrap();
    }

    // Subscribe to stream
    let subscriber = match state.router.subscribe(&stream_id) {
        Ok(sub) => sub,
        Err(e) => {
            return axum::response::Response::builder()
                .status(500)
                .body(axum::body::Body::from(format!(
                    "Failed to subscribe: {}",
                    e
                )))
                .unwrap();
        }
    };

    info!(stream = %stream_name, "New HTTP-FLV subscriber");

    // Create response stream
    let config = state.config.clone();
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(config.buffer_size);

    // Spawn encoder task
    tokio::spawn(async move {
        if let Err(e) = stream_flv(subscriber, tx, config).await {
            error!(stream = %stream_name, error = %e, "FLV streaming error");
        }
        info!(stream = %stream_name, "HTTP-FLV subscriber disconnected");
    });

    let stream = ReceiverStream::new(rx);

    // Build response with proper headers
    // Note: CORS is now handled by tower-http middleware
    let builder = axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "video/x-flv")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive");

    builder.body(axum::body::Body::from_stream(stream)).unwrap()
}

/// Stream FLV data to subscriber
async fn stream_flv(
    subscriber: StreamSubscriber,
    tx: mpsc::Sender<Result<Bytes, std::io::Error>>,
    config: HttpFlvConfig,
) -> FlvResult<()> {
    let has_video = true;
    let has_audio = true;

    let mut encoder = FlvEncoder::new(has_video, has_audio);

    // Send header
    if let Some(header) = encoder.header() {
        tx.send(Ok(header)).await.map_err(|_| {
            FlvError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Channel closed",
            ))
        })?;
    }

    // Stream frames - StreamRouter already sends sequence headers first for new subscribers
    loop {
        match tokio::time::timeout(config.timeout, subscriber.recv()).await {
            Ok(Ok(frame)) => {
                match encoder.encode_frame(&frame) {
                    Ok(Some(data)) => {
                        if tx.send(Ok(data)).await.is_err() {
                            break; // Client disconnected
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(error = %e, "Failed to encode frame");
                    }
                }
            }
            Ok(Err(_)) => break, // Channel closed
            Err(_) => {
                warn!("Stream timeout");
                break;
            }
        }
    }

    Ok(())
}

/// Health check endpoint
async fn health_check() -> axum::response::Response {
    axum::response::Response::builder()
        .status(200)
        .body(axum::body::Body::from("OK"))
        .unwrap()
}

/// HTTP-FLV client for consuming streams
pub struct HttpFlvClient {
    client: reqwest::Client,
    base_url: String,
}

impl HttpFlvClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    /// Start consuming a stream
    pub async fn consume(&self, stream_name: &str) -> FlvResult<mpsc::Receiver<MediaFrame>> {
        let url = format!("{}/live/{}", self.base_url, stream_name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| FlvError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if !response.status().is_success() {
            return Err(FlvError::InvalidData(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let (tx, rx) = mpsc::channel(1024);

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut decoder = super::decoder::FlvDecoder::new();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        decoder.push(&bytes);

                        loop {
                            match decoder.parse_next() {
                                Ok(Some(frame)) => {
                                    if tx.send(frame).await.is_err() {
                                        return;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => {
                                    error!(error = %e, "FLV decode error");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "HTTP stream error");
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

use futures::StreamExt;

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running server
    // Run with: cargo test --features integration

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_http_flv_server() {
        let router = Arc::new(StreamRouter::with_defaults());
        let server = HttpFlvServer::with_defaults(router);

        // Start server in background
        tokio::spawn(async move {
            server.run().await.unwrap();
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Test health endpoint
        let client = reqwest::Client::new();
        let response = client
            .get("http://localhost:8080/health")
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());
    }
}
