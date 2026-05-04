//! HLS HTTP server for serving playlists and segments

use super::{
    HlsConfig, HlsError, HlsResult,
    m3u8::{MasterPlaylist, Variant},
    packager::HlsPackagerManager,
};
use crate::media::{StreamId, StreamRouter};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

// Re-export for CORS
pub use tower_http::cors::CorsLayer;

/// HLS server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Server bind address
    pub bind_addr: String,
    /// Base URL for segments
    pub base_url: Option<String>,
    /// CORS allow origin
    pub cors_origin: Option<String>,
    /// Playlist cache duration
    pub playlist_cache_duration: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".to_string(),
            base_url: None,
            cors_origin: Some("*".to_string()),
            playlist_cache_duration: Duration::from_secs(1),
        }
    }
}

/// HLS HTTP server
pub struct HlsServer {
    router: Arc<StreamRouter>,
    packager_manager: Arc<HlsPackagerManager>,
    config: ServerConfig,
    hls_config: HlsConfig,
}

impl HlsServer {
    pub fn new(
        router: Arc<StreamRouter>,
        packager_manager: Arc<HlsPackagerManager>,
        config: ServerConfig,
        hls_config: HlsConfig,
    ) -> Self {
        Self {
            router,
            packager_manager,
            config,
            hls_config,
        }
    }

    /// Start the HTTP server
    pub async fn run(&self) -> HlsResult<()> {
        let app = self.create_router();

        let listener = tokio::net::TcpListener::bind(&self.config.bind_addr)
            .await
            .map_err(|e| HlsError::Io(e))?;

        info!(addr = %self.config.bind_addr, "HLS server starting");

        axum::serve(listener, app)
            .await
            .map_err(|e| HlsError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    fn create_router(&self) -> axum::Router {
        use axum::routing::get;

        let state = ServerState {
            router: Arc::clone(&self.router),
            packager_manager: Arc::clone(&self.packager_manager),
            config: self.config.clone(),
            hls_config: self.hls_config.clone(),
        };

        let cors_layer = self.create_cors_layer();

        axum::Router::new()
            .route("/hls/{stream}/master.m3u8", get(handle_master_playlist))
            .route("/hls/{stream}/index.m3u8", get(handle_media_playlist))
            .route("/hls/{stream}/segment/{idx}", get(handle_segment))
            .route("/health", get(health_check))
            .layer(cors_layer)
            .with_state(state)
    }

    /// Create CORS middleware layer
    fn create_cors_layer(&self) -> tower_http::cors::CorsLayer {
        use tower_http::cors::{Any, CorsLayer};

        let cors = if let Some(ref origin) = self.config.cors_origin {
            if origin == "*" {
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([http::Method::GET, http::Method::OPTIONS])
                    .allow_headers(Any)
                    .max_age(Duration::from_secs(86400))
            } else {
                CorsLayer::new()
                    .allow_origin(tower_http::cors::AllowOrigin::exact(
                        origin.parse().expect("Invalid CORS origin")
                    ))
                    .allow_methods([http::Method::GET, http::Method::OPTIONS])
                    .allow_headers(Any)
                    .max_age(Duration::from_secs(86400))
            }
        } else {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([http::Method::GET, http::Method::OPTIONS])
                .allow_headers(Any)
        };

        cors
    }
}

/// Server state
#[derive(Clone)]
#[allow(dead_code)]
struct ServerState {
    router: Arc<StreamRouter>,
    packager_manager: Arc<HlsPackagerManager>,
    config: ServerConfig,
    hls_config: HlsConfig,
}

/// Handle master playlist request
async fn handle_master_playlist(
    axum::extract::State(_state): axum::extract::State<ServerState>,
    axum::extract::Path(stream_name): axum::extract::Path<String>,
) -> axum::response::Response {
    let _stream_id = StreamId::new(stream_name.clone());

    // Note: In current architecture, RTMP server and StreamRouter are separate.
    // We return the master playlist regardless of stream existence,
    // allowing players to poll until segments are available.

    // Generate master playlist
    let mut master = MasterPlaylist::new();

    // Get stream stats for bandwidth estimation
    let bandwidth = 2_000_000u64; // TODO: Get actual bitrate

    // Add variant
    master.add_variant(Variant::new(bandwidth, "index.m3u8").with_codecs("avc1.42e00a,mp4a.40.2"));

    let body = master.to_string();

    axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "application/vnd.apple.mpegurl")
        .header("Cache-Control", "no-cache")
        .body(axum::body::Body::from(body))
        .unwrap()
}

/// Handle media playlist request
async fn handle_media_playlist(
    axum::extract::State(state): axum::extract::State<ServerState>,
    axum::extract::Path(stream_name): axum::extract::Path<String>,
) -> axum::response::Response {
    let stream_id = StreamId::new(stream_name.clone());

    // Check if packager exists
    let packager = match state.packager_manager.get_packager(&stream_id) {
        Some(p) => p,
        None => {
            // Stream not publishing yet
            return axum::response::Response::builder()
                .status(404)
                .header("Content-Type", "text/plain")
                .header("Cache-Control", "no-cache")
                .body(axum::body::Body::from(format!(
                    "Stream '{}' not found. Start streaming with:\n\nffmpeg -re -i input.mp4 -c:v libx264 -c:a aac -f flv rtmp://{}/live/{}",
                    stream_name,
                    state.config.bind_addr,
                    stream_name
                )))
                .unwrap();
        }
    };

    // Get playlist
    let playlist = packager.playlist_string().await;

    axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "application/vnd.apple.mpegurl")
        .header("Cache-Control", "max-age=1")
        .body(axum::body::Body::from(playlist))
        .unwrap()
}

/// Handle segment request
async fn handle_segment(
    axum::extract::State(state): axum::extract::State<ServerState>,
    axum::extract::Path((stream_name, idx)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    let stream_id = StreamId::new(stream_name.clone());

    // Parse segment index
    let segment_idx: u64 = match idx.parse() {
        Ok(n) => n,
        Err(_) => return not_found("Invalid segment index"),
    };

    // Get packager
    let packager = match state.packager_manager.get_packager(&stream_id) {
        Some(p) => p,
        None => return not_found(format!("Stream '{}' not found", stream_name)),
    };

    // Get segment
    match packager.get_segment(segment_idx).await {
        Ok(Some(segment)) => axum::response::Response::builder()
            .status(200)
            .header("Content-Type", segment.info.format.mime_type())
            .header("Cache-Control", "max-age=3600")
            .body(axum::body::Body::from(segment.data))
            .unwrap(),
        Ok(None) => not_found(format!("Segment {} not found", segment_idx)),
        Err(e) => {
            error!(error = %e, "Failed to load segment");
            server_error(e.to_string())
        }
    }
}

/// Health check
async fn health_check() -> axum::response::Response {
    axum::response::Response::builder()
        .status(200)
        .body(axum::body::Body::from("OK"))
        .unwrap()
}

/// 404 response
fn not_found(message: impl Into<String>) -> axum::response::Response {
    axum::response::Response::builder()
        .status(404)
        .body(axum::body::Body::from(message.into()))
        .unwrap()
}

/// 500 response
fn server_error(message: impl Into<String>) -> axum::response::Response {
    axum::response::Response::builder()
        .status(500)
        .body(axum::body::Body::from(message.into()))
        .unwrap()
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_hls_server() {
        // Test implementation would go here
    }
}
